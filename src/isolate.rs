// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
use crate::cli::Cli;
use crate::compiler::compile_sync;
use crate::compiler::ModuleMetaData;
use crate::errors::DenoError;
use crate::errors::RustOrJsError;
use crate::isolate_state::IsolateState;
use crate::js_errors;
use crate::msg;
use deno_core;
use deno_core::deno_mod;
use deno_core::JSError;
use futures::Async;
use futures::Future;
use std::sync::Arc;

type CoreIsolate = deno_core::Isolate<Cli>;

/// Wraps deno_core::Isolate to provide source maps, ops for the CLI, and
/// high-level module loading
pub struct Isolate {
  inner: CoreIsolate,
  state: Arc<IsolateState>,
}

impl Isolate {
  pub fn new(cli: Cli) -> Isolate {
    let state = cli.state.clone();
    Self {
      inner: CoreIsolate::new(cli),
      state,
    }
  }

  /// Same as execute2() but the filename defaults to "<anonymous>".
  pub fn execute(&mut self, js_source: &str) -> Result<(), JSError> {
    self.execute2("<anonymous>", js_source)
  }

  /// Executes the provided JavaScript source code. The js_filename argument is
  /// provided only for debugging purposes.
  pub fn execute2(
    &mut self,
    js_filename: &str,
    js_source: &str,
  ) -> Result<(), JSError> {
    self.inner.execute(js_filename, js_source)
  }

  // TODO(ry) make this return a future.
  fn mod_load_deps(&self, id: deno_mod) -> Result<(), RustOrJsError> {
    // basically iterate over the imports, start loading them.

    let referrer_name = {
      let g = self.state.modules.lock().unwrap();
      g.get_name(id).unwrap().clone()
    };

    for specifier in self.inner.mod_get_imports(id) {
      let (name, _local_filename) = self
        .state
        .dir
        .resolve_module(&specifier, &referrer_name)
        .map_err(DenoError::from)
        .map_err(RustOrJsError::from)?;

      debug!("mod_load_deps {}", name);

      if !self.state.modules.lock().unwrap().is_registered(&name) {
        let out = fetch_module_meta_data_and_maybe_compile(
          &self.state,
          &specifier,
          &referrer_name,
        )?;
        let child_id = self.mod_new_and_register(
          false,
          &out.module_name.clone(),
          &out.js_source(),
        )?;

        self.mod_load_deps(child_id)?;
      }
    }

    Ok(())
  }

  /// Executes the provided JavaScript module.
  pub fn execute_mod(
    &mut self,
    js_filename: &str,
    is_prefetch: bool,
  ) -> Result<(), RustOrJsError> {
    // TODO move isolate_state::execute_mod impl here.
    self
      .execute_mod_inner(js_filename, is_prefetch)
      .map_err(|err| match err {
        RustOrJsError::Js(err) => RustOrJsError::Js(self.apply_source_map(err)),
        x => x,
      })
  }

  /// High-level way to execute modules.
  /// This will issue HTTP requests and file system calls.
  /// Blocks. TODO(ry) Don't block.
  fn execute_mod_inner(
    &mut self,
    url: &str,
    is_prefetch: bool,
  ) -> Result<(), RustOrJsError> {
    let out = fetch_module_meta_data_and_maybe_compile(&self.state, url, ".")
      .map_err(RustOrJsError::from)?;

    let id = self
      .mod_new_and_register(true, &out.module_name.clone(), &out.js_source())
      .map_err(RustOrJsError::from)?;

    self.mod_load_deps(id)?;

    self
      .inner
      .mod_instantiate(id)
      .map_err(RustOrJsError::from)?;
    if !is_prefetch {
      self.inner.mod_evaluate(id).map_err(RustOrJsError::from)?;
    }
    Ok(())
  }

  /// Wraps Isolate::mod_new but registers with modules.
  fn mod_new_and_register(
    &self,
    main: bool,
    name: &str,
    source: &str,
  ) -> Result<deno_mod, JSError> {
    let id = self.inner.mod_new(main, name, source)?;
    self.state.modules.lock().unwrap().register(id, &name);
    Ok(id)
  }

  pub fn print_file_info(&self, module: &str) {
    let m = self.state.modules.lock().unwrap();
    m.print_file_info(&self.state.dir, module.to_string());
  }

  /// Applies source map to the error.
  fn apply_source_map(&self, err: JSError) -> JSError {
    js_errors::apply_source_map(&err, &self.state.dir)
  }
}

impl Future for Isolate {
  type Item = ();
  type Error = JSError;

  fn poll(&mut self) -> Result<Async<()>, Self::Error> {
    self.inner.poll().map_err(|err| self.apply_source_map(err))
  }
}

fn fetch_module_meta_data_and_maybe_compile(
  state: &Arc<IsolateState>,
  specifier: &str,
  referrer: &str,
) -> Result<ModuleMetaData, DenoError> {
  let mut out = state.dir.fetch_module_meta_data(specifier, referrer)?;
  if (out.media_type == msg::MediaType::TypeScript
    && out.maybe_output_code.is_none())
    || state.flags.recompile
  {
    debug!(">>>>> compile_sync START");
    out = compile_sync(state, specifier, &referrer, &out);
    debug!(">>>>> compile_sync END");
    state.dir.code_cache(&out)?;
  }
  Ok(out)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::flags;
  use crate::isolate_init::IsolateInit;
  use crate::permissions::DenoPermissions;
  use crate::tokio_util;
  use futures::future::lazy;
  use std::sync::atomic::Ordering;

  #[test]
  fn execute_mod() {
    let filename = std::env::current_dir()
      .unwrap()
      .join("tests/esm_imports_a.js");
    let filename = filename.to_str().unwrap().to_string();

    let argv = vec![String::from("./deno"), filename.clone()];
    let (flags, rest_argv, _) = flags::set_flags(argv).unwrap();

    let state = Arc::new(IsolateState::new(flags, rest_argv, None));
    let state_ = state.clone();
    let init = IsolateInit {
      snapshot: None,
      init_script: None,
    };
    tokio_util::run(lazy(move || {
      let cli = Cli::new(init, state.clone(), DenoPermissions::default());
      let mut isolate = Isolate::new(cli);
      if let Err(err) = isolate.execute_mod(&filename, false) {
        eprintln!("execute_mod err {:?}", err);
      }
      tokio_util::panic_on_error(isolate)
    }));

    let metrics = &state_.metrics;
    assert_eq!(metrics.resolve_count.load(Ordering::SeqCst), 1);
  }

  #[test]
  fn execute_mod_circular() {
    let filename = std::env::current_dir().unwrap().join("tests/circular1.js");
    let filename = filename.to_str().unwrap().to_string();

    let argv = vec![String::from("./deno"), filename.clone()];
    let (flags, rest_argv, _) = flags::set_flags(argv).unwrap();

    let state = Arc::new(IsolateState::new(flags, rest_argv, None));
    let state_ = state.clone();
    let init = IsolateInit {
      snapshot: None,
      init_script: None,
    };
    tokio_util::run(lazy(move || {
      let cli = Cli::new(init, state.clone(), DenoPermissions::default());
      let mut isolate = Isolate::new(cli);
      if let Err(err) = isolate.execute_mod(&filename, false) {
        eprintln!("execute_mod err {:?}", err);
      }
      tokio_util::panic_on_error(isolate)
    }));

    let metrics = &state_.metrics;
    assert_eq!(metrics.resolve_count.load(Ordering::SeqCst), 2);
  }
}
