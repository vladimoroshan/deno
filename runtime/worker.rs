// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use crate::inspector_server::InspectorServer;
use crate::js;
use crate::metrics;
use crate::ops;
use crate::permissions::Permissions;
use deno_broadcast_channel::InMemoryBroadcastChannel;
use deno_core::error::AnyError;
use deno_core::error::Context as ErrorContext;
use deno_core::futures::future::poll_fn;
use deno_core::futures::stream::StreamExt;
use deno_core::futures::Future;
use deno_core::serde_json;
use deno_core::serde_json::json;
use deno_core::url::Url;
use deno_core::Extension;
use deno_core::GetErrorClassFn;
use deno_core::JsErrorCreateFn;
use deno_core::JsRuntime;
use deno_core::LocalInspectorSession;
use deno_core::ModuleId;
use deno_core::ModuleLoader;
use deno_core::ModuleSpecifier;
use deno_core::RuntimeOptions;
use deno_file::BlobUrlStore;
use log::debug;
use std::env;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

/// This worker is created and used by almost all
/// subcommands in Deno executable.
///
/// It provides ops available in the `Deno` namespace.
///
/// All `WebWorker`s created during program execution
/// are descendants of this worker.
pub struct MainWorker {
  pub js_runtime: JsRuntime,
  should_break_on_first_statement: bool,
}

pub struct WorkerOptions {
  pub apply_source_maps: bool,
  /// Sets `Deno.args` in JS runtime.
  pub args: Vec<String>,
  pub debug_flag: bool,
  pub unstable: bool,
  pub ca_data: Option<Vec<u8>>,
  pub user_agent: String,
  pub seed: Option<u64>,
  pub module_loader: Rc<dyn ModuleLoader>,
  // Callback that will be invoked when creating new instance
  // of WebWorker
  pub create_web_worker_cb: Arc<ops::worker_host::CreateWebWorkerCb>,
  pub js_error_create_fn: Option<Rc<JsErrorCreateFn>>,
  pub attach_inspector: bool,
  pub maybe_inspector_server: Option<Arc<InspectorServer>>,
  pub should_break_on_first_statement: bool,
  /// Sets `Deno.version.deno` in JS runtime.
  pub runtime_version: String,
  /// Sets `Deno.version.typescript` in JS runtime.
  pub ts_version: String,
  /// Sets `Deno.noColor` in JS runtime.
  pub no_color: bool,
  pub get_error_class_fn: Option<GetErrorClassFn>,
  pub location: Option<Url>,
  pub origin_storage_dir: Option<std::path::PathBuf>,
  pub blob_url_store: BlobUrlStore,
  pub broadcast_channel: InMemoryBroadcastChannel,
}

impl MainWorker {
  pub fn from_options(
    main_module: ModuleSpecifier,
    permissions: Permissions,
    options: &WorkerOptions,
  ) -> Self {
    // Permissions: many ops depend on this
    let unstable = options.unstable;
    let perm_ext = Extension::builder()
      .state(move |state| {
        state.put::<Permissions>(permissions.clone());
        state.put(ops::UnstableChecker { unstable });
        Ok(())
      })
      .build();

    // Internal modules
    let extensions: Vec<Extension> = vec![
      // Web APIs
      deno_webidl::init(),
      deno_console::init(),
      deno_url::init(),
      deno_web::init(),
      deno_file::init(options.blob_url_store.clone(), options.location.clone()),
      deno_fetch::init::<Permissions>(
        options.user_agent.clone(),
        options.ca_data.clone(),
      ),
      deno_websocket::init::<Permissions>(
        options.user_agent.clone(),
        options.ca_data.clone(),
      ),
      deno_webstorage::init(options.origin_storage_dir.clone()),
      deno_crypto::init(options.seed),
      deno_broadcast_channel::init(
        options.broadcast_channel.clone(),
        options.unstable,
      ),
      deno_webgpu::init(options.unstable),
      deno_timers::init::<Permissions>(),
      // Metrics
      metrics::init(),
      // Runtime ops
      ops::runtime::init(main_module),
      ops::worker_host::init(options.create_web_worker_cb.clone()),
      ops::fs_events::init(),
      ops::fs::init(),
      ops::http::init(),
      ops::io::init(),
      ops::io::init_stdio(),
      ops::net::init(),
      ops::os::init(),
      ops::permissions::init(),
      ops::plugin::init(),
      ops::process::init(),
      ops::signal::init(),
      ops::tls::init(),
      ops::tty::init(),
      // Permissions ext (worker specific state)
      perm_ext,
    ];

    let mut js_runtime = JsRuntime::new(RuntimeOptions {
      module_loader: Some(options.module_loader.clone()),
      startup_snapshot: Some(js::deno_isolate_init()),
      js_error_create_fn: options.js_error_create_fn.clone(),
      get_error_class_fn: options.get_error_class_fn,
      extensions,
      attach_inspector: options.attach_inspector,
      ..Default::default()
    });

    let mut should_break_on_first_statement = false;

    if let Some(inspector) = js_runtime.inspector() {
      if let Some(server) = options.maybe_inspector_server.clone() {
        let session_sender = inspector.get_session_sender();
        let deregister_rx = inspector.add_deregister_handler();
        server.register_inspector(session_sender, deregister_rx);
      }
      should_break_on_first_statement = options.should_break_on_first_statement;
    }

    Self {
      js_runtime,
      should_break_on_first_statement,
    }
  }

  pub fn bootstrap(&mut self, options: &WorkerOptions) {
    let runtime_options = json!({
      "args": options.args,
      "applySourceMaps": options.apply_source_maps,
      "debugFlag": options.debug_flag,
      "denoVersion": options.runtime_version,
      "noColor": options.no_color,
      "pid": std::process::id(),
      "ppid": ops::runtime::ppid(),
      "target": env!("TARGET"),
      "tsVersion": options.ts_version,
      "unstableFlag": options.unstable,
      "v8Version": deno_core::v8_version(),
      "location": options.location,
    });

    let script = format!(
      "bootstrap.mainRuntime({})",
      serde_json::to_string_pretty(&runtime_options).unwrap()
    );
    self
      .execute(&script)
      .expect("Failed to execute bootstrap script");
  }

  /// Same as execute2() but the filename defaults to "$CWD/__anonymous__".
  pub fn execute(&mut self, js_source: &str) -> Result<(), AnyError> {
    let path = env::current_dir()
      .context("Failed to get current working directory")?
      .join("__anonymous__");
    let url = Url::from_file_path(path).unwrap();
    self.js_runtime.execute(url.as_str(), js_source)
  }

  /// Loads and instantiates specified JavaScript module.
  pub async fn preload_module(
    &mut self,
    module_specifier: &ModuleSpecifier,
  ) -> Result<ModuleId, AnyError> {
    self.js_runtime.load_module(module_specifier, None).await
  }

  /// Loads, instantiates and executes specified JavaScript module.
  pub async fn execute_module(
    &mut self,
    module_specifier: &ModuleSpecifier,
  ) -> Result<(), AnyError> {
    let id = self.preload_module(module_specifier).await?;
    self.wait_for_inspector_session();
    let mut receiver = self.js_runtime.mod_evaluate(id);
    tokio::select! {
      maybe_result = receiver.next() => {
        debug!("received module evaluate {:#?}", maybe_result);
        let result = maybe_result.expect("Module evaluation result not provided.");
        return result;
      }

      event_loop_result = self.run_event_loop(false) => {
        event_loop_result?;
        let maybe_result = receiver.next().await;
        let result = maybe_result.expect("Module evaluation result not provided.");
        return result;
      }
    }
  }

  fn wait_for_inspector_session(&mut self) {
    if self.should_break_on_first_statement {
      self
        .js_runtime
        .inspector()
        .unwrap()
        .wait_for_session_and_break_on_next_statement()
    }
  }

  /// Create new inspector session. This function panics if Worker
  /// was not configured to create inspector.
  pub async fn create_inspector_session(&mut self) -> LocalInspectorSession {
    let inspector = self.js_runtime.inspector().unwrap();
    inspector.create_local_session()
  }

  pub fn poll_event_loop(
    &mut self,
    cx: &mut Context,
    wait_for_inspector: bool,
  ) -> Poll<Result<(), AnyError>> {
    self.js_runtime.poll_event_loop(cx, wait_for_inspector)
  }

  pub async fn run_event_loop(
    &mut self,
    wait_for_inspector: bool,
  ) -> Result<(), AnyError> {
    poll_fn(|cx| self.poll_event_loop(cx, wait_for_inspector)).await
  }

  /// A utility function that runs provided future concurrently with the event loop.
  ///
  /// Useful when using a local inspector session.
  pub async fn with_event_loop<'a, T>(
    &mut self,
    mut fut: Pin<Box<dyn Future<Output = T> + 'a>>,
  ) -> T {
    loop {
      tokio::select! {
        result = &mut fut => {
          return result;
        }
        _ = self.run_event_loop(false) => {}
      };
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use deno_core::resolve_url_or_path;

  fn create_test_worker() -> MainWorker {
    let main_module = resolve_url_or_path("./hello.js").unwrap();
    let permissions = Permissions::default();

    let options = WorkerOptions {
      apply_source_maps: false,
      user_agent: "x".to_string(),
      args: vec![],
      debug_flag: false,
      unstable: false,
      ca_data: None,
      seed: None,
      js_error_create_fn: None,
      create_web_worker_cb: Arc::new(|_| unreachable!()),
      attach_inspector: false,
      maybe_inspector_server: None,
      should_break_on_first_statement: false,
      module_loader: Rc::new(deno_core::FsModuleLoader),
      runtime_version: "x".to_string(),
      ts_version: "x".to_string(),
      no_color: true,
      get_error_class_fn: None,
      location: None,
      origin_storage_dir: None,
      blob_url_store: BlobUrlStore::default(),
      broadcast_channel: InMemoryBroadcastChannel::default(),
    };

    MainWorker::from_options(main_module, permissions, &options)
  }

  #[tokio::test]
  async fn execute_mod_esm_imports_a() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .parent()
      .unwrap()
      .join("cli/tests/esm_imports_a.js");
    let module_specifier = resolve_url_or_path(&p.to_string_lossy()).unwrap();
    let mut worker = create_test_worker();
    let result = worker.execute_module(&module_specifier).await;
    if let Err(err) = result {
      eprintln!("execute_mod err {:?}", err);
    }
    if let Err(e) = worker.run_event_loop(false).await {
      panic!("Future got unexpected error: {:?}", e);
    }
  }

  #[tokio::test]
  async fn execute_mod_circular() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .parent()
      .unwrap()
      .join("tests/circular1.js");
    let module_specifier = resolve_url_or_path(&p.to_string_lossy()).unwrap();
    let mut worker = create_test_worker();
    let result = worker.execute_module(&module_specifier).await;
    if let Err(err) = result {
      eprintln!("execute_mod err {:?}", err);
    }
    if let Err(e) = worker.run_event_loop(false).await {
      panic!("Future got unexpected error: {:?}", e);
    }
  }

  #[tokio::test]
  async fn execute_mod_resolve_error() {
    // "foo" is not a valid module specifier so this should return an error.
    let mut worker = create_test_worker();
    let module_specifier = resolve_url_or_path("does-not-exist").unwrap();
    let result = worker.execute_module(&module_specifier).await;
    assert!(result.is_err());
  }

  #[tokio::test]
  async fn execute_mod_002_hello() {
    // This assumes cwd is project root (an assumption made throughout the
    // tests).
    let mut worker = create_test_worker();
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .parent()
      .unwrap()
      .join("cli/tests/001_hello.js");
    let module_specifier = resolve_url_or_path(&p.to_string_lossy()).unwrap();
    let result = worker.execute_module(&module_specifier).await;
    assert!(result.is_ok());
  }
}
