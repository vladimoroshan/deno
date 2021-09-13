// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

mod ast;
mod auth_tokens;
mod checksum;
mod config_file;
mod deno_dir;
mod diagnostics;
mod diff;
mod disk_cache;
mod errors;
mod file_fetcher;
mod file_watcher;
mod flags;
mod flags_allow_net;
mod fmt_errors;
mod fs_util;
mod http_cache;
mod http_util;
mod info;
mod lockfile;
mod logger;
mod lsp;
mod module_graph;
mod module_loader;
mod ops;
mod program_state;
mod source_maps;
mod specifier_handler;
mod standalone;
mod text_encoding;
mod tokio_util;
mod tools;
mod tsc;
mod unix_util;
mod version;

use crate::file_fetcher::File;
use crate::file_watcher::ResolutionResult;
use crate::flags::BundleFlags;
use crate::flags::CacheFlags;
use crate::flags::CompileFlags;
use crate::flags::CompletionsFlags;
use crate::flags::CoverageFlags;
use crate::flags::DenoSubcommand;
use crate::flags::DocFlags;
use crate::flags::EvalFlags;
use crate::flags::Flags;
use crate::flags::FmtFlags;
use crate::flags::InfoFlags;
use crate::flags::InstallFlags;
use crate::flags::LintFlags;
use crate::flags::ReplFlags;
use crate::flags::RunFlags;
use crate::flags::TestFlags;
use crate::flags::UpgradeFlags;
use crate::fmt_errors::PrettyJsError;
use crate::module_loader::CliModuleLoader;
use crate::program_state::ProgramState;
use crate::source_maps::apply_source_map;
use crate::specifier_handler::FetchHandler;
use crate::tools::installer::infer_name_from_url;
use deno_ast::MediaType;
use deno_core::error::generic_error;
use deno_core::error::AnyError;
use deno_core::futures::future::FutureExt;
use deno_core::futures::Future;
use deno_core::located_script_name;
use deno_core::parking_lot::Mutex;
use deno_core::resolve_url_or_path;
use deno_core::serde_json;
use deno_core::serde_json::json;
use deno_core::v8_set_flags;
use deno_core::JsRuntime;
use deno_core::ModuleSpecifier;
use deno_runtime::colors;
use deno_runtime::ops::worker_host::CreateWebWorkerCb;
use deno_runtime::permissions::Permissions;
use deno_runtime::web_worker::WebWorker;
use deno_runtime::web_worker::WebWorkerOptions;
use deno_runtime::worker::MainWorker;
use deno_runtime::worker::WorkerOptions;
use log::debug;
use log::info;
use std::env;
use std::io::Read;
use std::io::Write;
use std::iter::once;
use std::path::PathBuf;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

fn create_web_worker_callback(
  program_state: Arc<ProgramState>,
) -> Arc<CreateWebWorkerCb> {
  Arc::new(move |args| {
    let global_state_ = program_state.clone();
    let js_error_create_fn = Rc::new(move |core_js_error| {
      let source_mapped_error =
        apply_source_map(&core_js_error, global_state_.clone());
      PrettyJsError::create(source_mapped_error)
    });

    let maybe_inspector_server = program_state.maybe_inspector_server.clone();

    let module_loader = CliModuleLoader::new_for_worker(
      program_state.clone(),
      args.parent_permissions.clone(),
    );
    let create_web_worker_cb =
      create_web_worker_callback(program_state.clone());

    let options = WebWorkerOptions {
      args: program_state.flags.argv.clone(),
      apply_source_maps: true,
      debug_flag: program_state
        .flags
        .log_level
        .map_or(false, |l| l == log::Level::Debug),
      unstable: program_state.flags.unstable,
      enable_testing_features: program_state.flags.enable_testing_features,
      unsafely_ignore_certificate_errors: program_state
        .flags
        .unsafely_ignore_certificate_errors
        .clone(),
      root_cert_store: program_state.root_cert_store.clone(),
      user_agent: version::get_user_agent(),
      seed: program_state.flags.seed,
      module_loader,
      create_web_worker_cb,
      js_error_create_fn: Some(js_error_create_fn),
      use_deno_namespace: args.use_deno_namespace,
      worker_type: args.worker_type,
      maybe_inspector_server,
      runtime_version: version::deno(),
      ts_version: version::TYPESCRIPT.to_string(),
      no_color: !colors::use_color(),
      get_error_class_fn: Some(&crate::errors::get_error_class_name),
      blob_store: program_state.blob_store.clone(),
      broadcast_channel: program_state.broadcast_channel.clone(),
      shared_array_buffer_store: Some(
        program_state.shared_array_buffer_store.clone(),
      ),
      cpu_count: num_cpus::get(),
    };

    let (mut worker, external_handle) = WebWorker::from_options(
      args.name,
      args.permissions,
      args.main_module,
      args.worker_id,
      &options,
    );

    // This block registers additional ops and state that
    // are only available in the CLI
    {
      let js_runtime = &mut worker.js_runtime;
      js_runtime
        .op_state()
        .borrow_mut()
        .put::<Arc<ProgramState>>(program_state.clone());
      // Applies source maps - works in conjuction with `js_error_create_fn`
      // above
      ops::errors::init(js_runtime);
      if args.use_deno_namespace {
        ops::runtime_compiler::init(js_runtime);
      }
      js_runtime.sync_ops_cache();
    }
    worker.bootstrap(&options);

    (worker, external_handle)
  })
}

pub fn create_main_worker(
  program_state: &Arc<ProgramState>,
  main_module: ModuleSpecifier,
  permissions: Permissions,
  maybe_op_init: Option<&dyn Fn(&mut JsRuntime)>,
) -> MainWorker {
  let module_loader = CliModuleLoader::new(program_state.clone());

  let global_state_ = program_state.clone();

  let js_error_create_fn = Rc::new(move |core_js_error| {
    let source_mapped_error =
      apply_source_map(&core_js_error, global_state_.clone());
    PrettyJsError::create(source_mapped_error)
  });

  let maybe_inspector_server = program_state.maybe_inspector_server.clone();
  let should_break_on_first_statement =
    program_state.flags.inspect_brk.is_some();

  let create_web_worker_cb = create_web_worker_callback(program_state.clone());

  let options = WorkerOptions {
    apply_source_maps: true,
    args: program_state.flags.argv.clone(),
    debug_flag: program_state
      .flags
      .log_level
      .map_or(false, |l| l == log::Level::Debug),
    unstable: program_state.flags.unstable,
    enable_testing_features: program_state.flags.enable_testing_features,
    unsafely_ignore_certificate_errors: program_state
      .flags
      .unsafely_ignore_certificate_errors
      .clone(),
    root_cert_store: program_state.root_cert_store.clone(),
    user_agent: version::get_user_agent(),
    seed: program_state.flags.seed,
    js_error_create_fn: Some(js_error_create_fn),
    create_web_worker_cb,
    maybe_inspector_server,
    should_break_on_first_statement,
    module_loader,
    runtime_version: version::deno(),
    ts_version: version::TYPESCRIPT.to_string(),
    no_color: !colors::use_color(),
    get_error_class_fn: Some(&crate::errors::get_error_class_name),
    location: program_state.flags.location.clone(),
    origin_storage_dir: program_state.flags.location.clone().map(|loc| {
      program_state
        .dir
        .root
        .clone()
        // TODO(@crowlKats): change to origin_data for 2.0
        .join("location_data")
        .join(checksum::gen(&[loc.to_string().as_bytes()]))
    }),
    blob_store: program_state.blob_store.clone(),
    broadcast_channel: program_state.broadcast_channel.clone(),
    shared_array_buffer_store: Some(
      program_state.shared_array_buffer_store.clone(),
    ),
    cpu_count: num_cpus::get(),
  };

  let mut worker = MainWorker::from_options(main_module, permissions, &options);

  // This block registers additional ops and state that
  // are only available in the CLI
  {
    let js_runtime = &mut worker.js_runtime;
    js_runtime
      .op_state()
      .borrow_mut()
      .put::<Arc<ProgramState>>(program_state.clone());
    // Applies source maps - works in conjuction with `js_error_create_fn`
    // above
    ops::errors::init(js_runtime);
    ops::runtime_compiler::init(js_runtime);

    if let Some(op_init) = maybe_op_init {
      op_init(js_runtime);
    }

    js_runtime.sync_ops_cache();
  }
  worker.bootstrap(&options);

  worker
}

pub fn write_to_stdout_ignore_sigpipe(
  bytes: &[u8],
) -> Result<(), std::io::Error> {
  use std::io::ErrorKind;

  match std::io::stdout().write_all(bytes) {
    Ok(()) => Ok(()),
    Err(e) => match e.kind() {
      ErrorKind::BrokenPipe => Ok(()),
      _ => Err(e),
    },
  }
}

pub fn write_json_to_stdout<T>(value: &T) -> Result<(), AnyError>
where
  T: ?Sized + serde::ser::Serialize,
{
  let mut writer = std::io::BufWriter::new(std::io::stdout());
  serde_json::to_writer_pretty(&mut writer, value)?;
  writeln!(&mut writer)?;
  Ok(())
}

fn print_cache_info(
  state: &Arc<ProgramState>,
  json: bool,
  location: Option<deno_core::url::Url>,
) -> Result<(), AnyError> {
  let deno_dir = &state.dir.root;
  let modules_cache = &state.file_fetcher.get_http_cache_location();
  let typescript_cache = &state.dir.gen_cache.location;
  let registry_cache =
    &state.dir.root.join(lsp::language_server::REGISTRIES_PATH);
  let mut origin_dir = state.dir.root.join("location_data");

  if let Some(location) = &location {
    origin_dir =
      origin_dir.join(&checksum::gen(&[location.to_string().as_bytes()]));
  }

  if json {
    let mut output = json!({
      "denoDir": deno_dir,
      "modulesCache": modules_cache,
      "typescriptCache": typescript_cache,
      "registryCache": registry_cache,
      "originStorage": origin_dir,
    });

    if location.is_some() {
      output["localStorage"] =
        serde_json::to_value(origin_dir.join("local_storage"))?;
    }

    write_json_to_stdout(&output)
  } else {
    println!("{} {:?}", colors::bold("DENO_DIR location:"), deno_dir);
    println!(
      "{} {:?}",
      colors::bold("Remote modules cache:"),
      modules_cache
    );
    println!(
      "{} {:?}",
      colors::bold("Emitted modules cache:"),
      typescript_cache
    );
    println!(
      "{} {:?}",
      colors::bold("Language server registries cache:"),
      registry_cache,
    );
    println!("{} {:?}", colors::bold("Origin storage:"), origin_dir);
    if location.is_some() {
      println!(
        "{} {:?}",
        colors::bold("Local Storage:"),
        origin_dir.join("local_storage"),
      );
    }
    Ok(())
  }
}

pub fn get_types(unstable: bool) -> String {
  let mut types = vec![
    crate::tsc::DENO_NS_LIB,
    crate::tsc::DENO_CONSOLE_LIB,
    crate::tsc::DENO_URL_LIB,
    crate::tsc::DENO_WEB_LIB,
    crate::tsc::DENO_FETCH_LIB,
    crate::tsc::DENO_WEBGPU_LIB,
    crate::tsc::DENO_WEBSOCKET_LIB,
    crate::tsc::DENO_WEBSTORAGE_LIB,
    crate::tsc::DENO_CRYPTO_LIB,
    crate::tsc::DENO_BROADCAST_CHANNEL_LIB,
    crate::tsc::DENO_NET_LIB,
    crate::tsc::SHARED_GLOBALS_LIB,
    crate::tsc::WINDOW_LIB,
  ];

  if unstable {
    types.push(crate::tsc::UNSTABLE_NS_LIB);
  }

  types.join("\n")
}

async fn compile_command(
  flags: Flags,
  compile_flags: CompileFlags,
) -> Result<(), AnyError> {
  let debug = flags.log_level == Some(log::Level::Debug);

  let run_flags = tools::standalone::compile_to_runtime_flags(
    flags.clone(),
    compile_flags.args,
  )?;

  let module_specifier = resolve_url_or_path(&compile_flags.source_file)?;
  let program_state = ProgramState::build(flags.clone()).await?;
  let deno_dir = &program_state.dir;

  let output = compile_flags.output.or_else(|| {
    infer_name_from_url(&module_specifier).map(PathBuf::from)
  }).ok_or_else(|| generic_error(
    "An executable name was not provided. One could not be inferred from the URL. Aborting.",
  ))?;

  let module_graph = create_module_graph_and_maybe_check(
    module_specifier.clone(),
    program_state.clone(),
    debug,
  )
  .await?;

  info!(
    "{} {}",
    colors::green("Bundle"),
    module_specifier.to_string()
  );
  let bundle_str =
    bundle_module_graph(module_graph, program_state.clone(), flags, debug)?;

  info!(
    "{} {}",
    colors::green("Compile"),
    module_specifier.to_string()
  );

  // Select base binary based on target
  let original_binary =
    tools::standalone::get_base_binary(deno_dir, compile_flags.target.clone())
      .await?;

  let final_bin = tools::standalone::create_standalone_binary(
    original_binary,
    bundle_str,
    run_flags,
  )?;

  info!("{} {}", colors::green("Emit"), output.display());

  tools::standalone::write_standalone_binary(
    output.clone(),
    compile_flags.target,
    final_bin,
  )
  .await?;

  Ok(())
}

async fn info_command(
  flags: Flags,
  info_flags: InfoFlags,
) -> Result<(), AnyError> {
  let location = flags.location.clone();
  let program_state = ProgramState::build(flags).await?;
  if let Some(specifier) = info_flags.file {
    let specifier = resolve_url_or_path(&specifier)?;
    let handler = Arc::new(Mutex::new(specifier_handler::FetchHandler::new(
      &program_state,
      // info accesses dynamically imported modules just for their information
      // so we allow access to all of them.
      Permissions::allow_all(),
      Permissions::allow_all(),
    )?));
    let mut builder = module_graph::GraphBuilder::new(
      handler,
      program_state.maybe_import_map.clone(),
      program_state.lockfile.clone(),
    );
    builder.add(&specifier, false).await?;
    builder
      .analyze_config_file(&program_state.maybe_config_file)
      .await?;
    let graph = builder.get_graph();
    let info = graph.info()?;

    if info_flags.json {
      write_json_to_stdout(&json!(info))
    } else {
      write_to_stdout_ignore_sigpipe(info.to_string().as_bytes())
        .map_err(|err| err.into())
    }
  } else {
    // If it was just "deno info" print location of caches and exit
    print_cache_info(&program_state, info_flags.json, location)
  }
}

async fn install_command(
  flags: Flags,
  install_flags: InstallFlags,
) -> Result<(), AnyError> {
  let mut preload_flags = flags.clone();
  preload_flags.inspect = None;
  preload_flags.inspect_brk = None;
  let permissions = Permissions::from_options(&preload_flags.clone().into());
  let program_state = ProgramState::build(preload_flags).await?;
  let main_module = resolve_url_or_path(&install_flags.module_url)?;
  let mut worker =
    create_main_worker(&program_state, main_module.clone(), permissions, None);
  // First, fetch and compile the module; this step ensures that the module exists.
  worker.preload_module(&main_module).await?;
  tools::installer::install(
    flags,
    &install_flags.module_url,
    install_flags.args,
    install_flags.name,
    install_flags.root,
    install_flags.force,
  )
}

async fn lsp_command() -> Result<(), AnyError> {
  lsp::start().await
}

#[allow(clippy::too_many_arguments)]
async fn lint_command(
  flags: Flags,
  lint_flags: LintFlags,
) -> Result<(), AnyError> {
  if lint_flags.rules {
    tools::lint::print_rules_list(lint_flags.json);
    return Ok(());
  }

  let program_state = ProgramState::build(flags.clone()).await?;
  let maybe_lint_config =
    if let Some(config_file) = &program_state.maybe_config_file {
      config_file.to_lint_config()?
    } else {
      None
    };

  tools::lint::lint_files(
    maybe_lint_config,
    lint_flags.rules_tags,
    lint_flags.rules_include,
    lint_flags.rules_exclude,
    lint_flags.files,
    lint_flags.ignore,
    lint_flags.json,
  )
  .await
}

async fn cache_command(
  flags: Flags,
  cache_flags: CacheFlags,
) -> Result<(), AnyError> {
  let lib = if flags.unstable {
    module_graph::TypeLib::UnstableDenoWindow
  } else {
    module_graph::TypeLib::DenoWindow
  };
  let program_state = ProgramState::build(flags).await?;

  for file in cache_flags.files {
    let specifier = resolve_url_or_path(&file)?;
    program_state
      .prepare_module_load(
        specifier,
        lib.clone(),
        Permissions::allow_all(),
        Permissions::allow_all(),
        false,
        program_state.maybe_import_map.clone(),
      )
      .await?;
  }

  Ok(())
}

async fn eval_command(
  flags: Flags,
  eval_flags: EvalFlags,
) -> Result<(), AnyError> {
  // Force TypeScript compile.
  let main_module = resolve_url_or_path("./$deno$eval.ts").unwrap();
  let permissions = Permissions::from_options(&flags.clone().into());
  let program_state = ProgramState::build(flags).await?;
  let mut worker =
    create_main_worker(&program_state, main_module.clone(), permissions, None);
  // Create a dummy source file.
  let source_code = if eval_flags.print {
    format!("console.log({})", eval_flags.code)
  } else {
    eval_flags.code
  }
  .into_bytes();

  let file = File {
    local: main_module.clone().to_file_path().unwrap(),
    maybe_types: None,
    media_type: if eval_flags.ext.as_str() == "ts" {
      MediaType::TypeScript
    } else if eval_flags.ext.as_str() == "tsx" {
      MediaType::Tsx
    } else if eval_flags.ext.as_str() == "js" {
      MediaType::JavaScript
    } else {
      MediaType::Jsx
    },
    source: Arc::new(String::from_utf8(source_code)?),
    specifier: main_module.clone(),
    maybe_headers: None,
  };

  // Save our fake file into file fetcher cache
  // to allow module access by TS compiler.
  program_state.file_fetcher.insert_cached(file);
  debug!("main_module {}", &main_module);
  worker.execute_module(&main_module).await?;
  worker.execute_script(
    &located_script_name!(),
    "window.dispatchEvent(new Event('load'))",
  )?;
  worker.run_event_loop(false).await?;
  worker.execute_script(
    &located_script_name!(),
    "window.dispatchEvent(new Event('unload'))",
  )?;
  Ok(())
}

async fn create_module_graph_and_maybe_check(
  module_specifier: ModuleSpecifier,
  program_state: Arc<ProgramState>,
  debug: bool,
) -> Result<module_graph::Graph, AnyError> {
  let handler = Arc::new(Mutex::new(FetchHandler::new(
    &program_state,
    // when bundling, dynamic imports are only access for their type safety,
    // therefore we will allow the graph to access any module.
    Permissions::allow_all(),
    Permissions::allow_all(),
  )?));
  let mut builder = module_graph::GraphBuilder::new(
    handler,
    program_state.maybe_import_map.clone(),
    program_state.lockfile.clone(),
  );
  builder.add(&module_specifier, false).await?;
  builder
    .analyze_config_file(&program_state.maybe_config_file)
    .await?;
  let module_graph = builder.get_graph();

  if !program_state.flags.no_check {
    // TODO(@kitsonk) support bundling for workers
    let lib = if program_state.flags.unstable {
      module_graph::TypeLib::UnstableDenoWindow
    } else {
      module_graph::TypeLib::DenoWindow
    };
    let result_info =
      module_graph.clone().check(module_graph::CheckOptions {
        debug,
        emit: false,
        lib,
        maybe_config_file: program_state.maybe_config_file.clone(),
        reload: program_state.flags.reload,
        ..Default::default()
      })?;

    debug!("{}", result_info.stats);
    if let Some(ignored_options) = result_info.maybe_ignored_options {
      eprintln!("{}", ignored_options);
    }
    if !result_info.diagnostics.is_empty() {
      return Err(generic_error(result_info.diagnostics.to_string()));
    }
  }

  Ok(module_graph)
}

fn bundle_module_graph(
  module_graph: module_graph::Graph,
  program_state: Arc<ProgramState>,
  flags: Flags,
  debug: bool,
) -> Result<String, AnyError> {
  let (bundle, stats, maybe_ignored_options) =
    module_graph.bundle(module_graph::BundleOptions {
      debug,
      maybe_config_file: program_state.maybe_config_file.clone(),
    })?;
  match maybe_ignored_options {
    Some(ignored_options) if flags.no_check => {
      eprintln!("{}", ignored_options);
    }
    _ => {}
  }
  debug!("{}", stats);
  Ok(bundle)
}

async fn bundle_command(
  flags: Flags,
  bundle_flags: BundleFlags,
) -> Result<(), AnyError> {
  let debug = flags.log_level == Some(log::Level::Debug);

  let resolver = |_| {
    let flags = flags.clone();
    let source_file1 = bundle_flags.source_file.clone();
    let source_file2 = bundle_flags.source_file.clone();
    async move {
      let module_specifier = resolve_url_or_path(&source_file1)?;

      debug!(">>>>> bundle START");
      let program_state = ProgramState::build(flags.clone()).await?;

      let module_graph = create_module_graph_and_maybe_check(
        module_specifier,
        program_state.clone(),
        debug,
      )
      .await?;

      let mut paths_to_watch: Vec<PathBuf> = module_graph
        .get_modules()
        .iter()
        .filter_map(|specifier| specifier.to_file_path().ok())
        .collect();

      if let Some(import_map) = program_state.flags.import_map_path.as_ref() {
        paths_to_watch
          .push(fs_util::resolve_from_cwd(std::path::Path::new(import_map))?);
      }

      Ok((paths_to_watch, module_graph, program_state))
    }
    .map(move |result| match result {
      Ok((paths_to_watch, module_graph, program_state)) => {
        ResolutionResult::Restart {
          paths_to_watch,
          result: Ok((program_state, module_graph)),
        }
      }
      Err(e) => ResolutionResult::Restart {
        paths_to_watch: vec![PathBuf::from(source_file2)],
        result: Err(e),
      },
    })
  };

  let operation = |(program_state, module_graph): (
    Arc<ProgramState>,
    module_graph::Graph,
  )| {
    let flags = flags.clone();
    let out_file = bundle_flags.out_file.clone();
    async move {
      info!("{} {}", colors::green("Bundle"), module_graph.info()?.root);

      let output =
        bundle_module_graph(module_graph, program_state, flags, debug)?;

      debug!(">>>>> bundle END");

      if let Some(out_file) = out_file.as_ref() {
        let output_bytes = output.as_bytes();
        let output_len = output_bytes.len();
        fs_util::write_file(out_file, output_bytes, 0o644)?;
        info!(
          "{} {:?} ({})",
          colors::green("Emit"),
          out_file,
          colors::gray(&info::human_size(output_len as f64))
        );
      } else {
        println!("{}", output);
      }

      Ok(())
    }
  };

  if flags.watch {
    file_watcher::watch_func(resolver, operation, "Bundle").await?;
  } else {
    let module_graph =
      if let ResolutionResult::Restart { result, .. } = resolver(None).await {
        result?
      } else {
        unreachable!();
      };
    operation(module_graph).await?;
  }

  Ok(())
}

async fn doc_command(
  flags: Flags,
  doc_flags: DocFlags,
) -> Result<(), AnyError> {
  tools::doc::print_docs(
    flags,
    doc_flags.source_file,
    doc_flags.json,
    doc_flags.filter,
    doc_flags.private,
  )
  .await
}

async fn format_command(
  flags: Flags,
  fmt_flags: FmtFlags,
) -> Result<(), AnyError> {
  let program_state = ProgramState::build(flags.clone()).await?;
  let maybe_fmt_config =
    if let Some(config_file) = &program_state.maybe_config_file {
      config_file.to_fmt_config()?
    } else {
      None
    };

  if fmt_flags.files.len() == 1 && fmt_flags.files[0].to_string_lossy() == "-" {
    return tools::fmt::format_stdin(
      fmt_flags.check,
      fmt_flags.ext,
      maybe_fmt_config.map(|c| c.options).unwrap_or_default(),
    );
  }

  tools::fmt::format(
    fmt_flags.files,
    fmt_flags.ignore,
    fmt_flags.check,
    flags.watch,
    maybe_fmt_config,
  )
  .await?;
  Ok(())
}

async fn run_repl(flags: Flags, repl_flags: ReplFlags) -> Result<(), AnyError> {
  let main_module = resolve_url_or_path("./$deno$repl.ts").unwrap();
  let permissions = Permissions::from_options(&flags.clone().into());
  let program_state = ProgramState::build(flags).await?;
  let mut worker =
    create_main_worker(&program_state, main_module.clone(), permissions, None);
  worker.run_event_loop(false).await?;

  tools::repl::run(&program_state, worker, repl_flags.eval).await
}

async fn run_from_stdin(flags: Flags) -> Result<(), AnyError> {
  let program_state = ProgramState::build(flags.clone()).await?;
  let permissions = Permissions::from_options(&flags.clone().into());
  let main_module = resolve_url_or_path("./$deno$stdin.ts").unwrap();
  let mut worker = create_main_worker(
    &program_state.clone(),
    main_module.clone(),
    permissions,
    None,
  );

  let mut source = Vec::new();
  std::io::stdin().read_to_end(&mut source)?;
  // Create a dummy source file.
  let source_file = File {
    local: main_module.clone().to_file_path().unwrap(),
    maybe_types: None,
    media_type: MediaType::TypeScript,
    source: Arc::new(String::from_utf8(source)?),
    specifier: main_module.clone(),
    maybe_headers: None,
  };
  // Save our fake file into file fetcher cache
  // to allow module access by TS compiler
  program_state.file_fetcher.insert_cached(source_file);

  debug!("main_module {}", main_module);
  worker.execute_module(&main_module).await?;
  worker.execute_script(
    &located_script_name!(),
    "window.dispatchEvent(new Event('load'))",
  )?;
  worker.run_event_loop(false).await?;
  worker.execute_script(
    &located_script_name!(),
    "window.dispatchEvent(new Event('unload'))",
  )?;
  Ok(())
}

async fn run_with_watch(flags: Flags, script: String) -> Result<(), AnyError> {
  let resolver = |_| {
    let script1 = script.clone();
    let script2 = script.clone();
    let flags = flags.clone();
    async move {
      let main_module = resolve_url_or_path(&script1)?;
      let program_state = ProgramState::build(flags).await?;
      let handler = Arc::new(Mutex::new(FetchHandler::new(
        &program_state,
        Permissions::allow_all(),
        Permissions::allow_all(),
      )?));
      let mut builder = module_graph::GraphBuilder::new(
        handler,
        program_state.maybe_import_map.clone(),
        program_state.lockfile.clone(),
      );
      builder.add(&main_module, false).await?;
      builder
        .analyze_config_file(&program_state.maybe_config_file)
        .await?;
      let module_graph = builder.get_graph();

      // Find all local files in graph
      let mut paths_to_watch: Vec<PathBuf> = module_graph
        .get_modules()
        .iter()
        .filter_map(|specifier| specifier.to_file_path().ok())
        .collect();

      if let Some(import_map) = program_state.flags.import_map_path.as_ref() {
        paths_to_watch
          .push(fs_util::resolve_from_cwd(std::path::Path::new(import_map))?);
      }

      Ok((paths_to_watch, main_module, program_state))
    }
    .map(move |result| match result {
      Ok((paths_to_watch, module_info, program_state)) => {
        ResolutionResult::Restart {
          paths_to_watch,
          result: Ok((program_state, module_info)),
        }
      }
      Err(e) => ResolutionResult::Restart {
        paths_to_watch: vec![PathBuf::from(script2)],
        result: Err(e),
      },
    })
  };

  /// The FileWatcherModuleExecutor provides module execution with safe dispatching of life-cycle events by tracking the
  /// state of any pending events and emitting accordingly on drop in the case of a future
  /// cancellation.
  struct FileWatcherModuleExecutor {
    worker: MainWorker,
    pending_unload: bool,
  }

  impl FileWatcherModuleExecutor {
    pub fn new(worker: MainWorker) -> FileWatcherModuleExecutor {
      FileWatcherModuleExecutor {
        worker,
        pending_unload: false,
      }
    }

    /// Execute the given main module emitting load and unload events before and after execution
    /// respectively.
    pub async fn execute(
      &mut self,
      main_module: &ModuleSpecifier,
    ) -> Result<(), AnyError> {
      self.worker.execute_module(main_module).await?;
      self.worker.execute_script(
        &located_script_name!(),
        "window.dispatchEvent(new Event('load'))",
      )?;
      self.pending_unload = true;

      let result = self.worker.run_event_loop(false).await;
      self.pending_unload = false;

      if let Err(err) = result {
        return Err(err);
      }

      self.worker.execute_script(
        &located_script_name!(),
        "window.dispatchEvent(new Event('unload'))",
      )?;

      Ok(())
    }
  }

  impl Drop for FileWatcherModuleExecutor {
    fn drop(&mut self) {
      if self.pending_unload {
        self
          .worker
          .execute_script(
            &located_script_name!(),
            "window.dispatchEvent(new Event('unload'))",
          )
          .unwrap();
      }
    }
  }

  let operation =
    |(program_state, main_module): (Arc<ProgramState>, ModuleSpecifier)| {
      let flags = flags.clone();
      let permissions = Permissions::from_options(&flags.into());
      async move {
        // We make use an module executor guard to ensure that unload is always fired when an
        // operation is called.
        let mut executor = FileWatcherModuleExecutor::new(create_main_worker(
          &program_state,
          main_module.clone(),
          permissions,
          None,
        ));

        executor.execute(&main_module).await?;

        Ok(())
      }
    };

  file_watcher::watch_func(resolver, operation, "Process").await
}

async fn run_command(
  flags: Flags,
  run_flags: RunFlags,
) -> Result<(), AnyError> {
  // Read script content from stdin
  if run_flags.script == "-" {
    return run_from_stdin(flags).await;
  }

  if flags.watch {
    return run_with_watch(flags, run_flags.script).await;
  }

  let main_module = resolve_url_or_path(&run_flags.script)?;
  let program_state = ProgramState::build(flags.clone()).await?;
  let permissions = Permissions::from_options(&flags.clone().into());
  let mut worker =
    create_main_worker(&program_state, main_module.clone(), permissions, None);

  let mut maybe_coverage_collector =
    if let Some(ref coverage_dir) = program_state.coverage_dir {
      let session = worker.create_inspector_session().await;

      let coverage_dir = PathBuf::from(coverage_dir);
      let mut coverage_collector =
        tools::coverage::CoverageCollector::new(coverage_dir, session);
      worker
        .with_event_loop(coverage_collector.start_collecting().boxed_local())
        .await?;
      Some(coverage_collector)
    } else {
      None
    };

  debug!("main_module {}", main_module);
  worker.execute_module(&main_module).await?;
  worker.execute_script(
    &located_script_name!(),
    "window.dispatchEvent(new Event('load'))",
  )?;
  worker
    .run_event_loop(maybe_coverage_collector.is_none())
    .await?;
  worker.execute_script(
    &located_script_name!(),
    "window.dispatchEvent(new Event('unload'))",
  )?;

  if let Some(coverage_collector) = maybe_coverage_collector.as_mut() {
    worker
      .with_event_loop(coverage_collector.stop_collecting().boxed_local())
      .await?;
  }
  Ok(())
}

async fn coverage_command(
  flags: Flags,
  coverage_flags: CoverageFlags,
) -> Result<(), AnyError> {
  if coverage_flags.files.is_empty() {
    return Err(generic_error("No matching coverage profiles found"));
  }

  tools::coverage::cover_files(
    flags.clone(),
    coverage_flags.files,
    coverage_flags.ignore,
    coverage_flags.include,
    coverage_flags.exclude,
    coverage_flags.lcov,
  )
  .await
}

async fn test_command(
  flags: Flags,
  test_flags: TestFlags,
) -> Result<(), AnyError> {
  if let Some(ref coverage_dir) = flags.coverage_dir {
    std::fs::create_dir_all(&coverage_dir)?;
    env::set_var(
      "DENO_UNSTABLE_COVERAGE_DIR",
      PathBuf::from(coverage_dir).canonicalize()?,
    );
  }

  if flags.watch {
    tools::test::run_tests_with_watch(
      flags,
      test_flags.include,
      test_flags.ignore,
      test_flags.doc,
      test_flags.no_run,
      test_flags.fail_fast,
      test_flags.filter,
      test_flags.shuffle,
      test_flags.concurrent_jobs,
    )
    .await?;

    return Ok(());
  }

  tools::test::run_tests(
    flags,
    test_flags.include,
    test_flags.ignore,
    test_flags.doc,
    test_flags.no_run,
    test_flags.fail_fast,
    test_flags.allow_none,
    test_flags.filter,
    test_flags.shuffle,
    test_flags.concurrent_jobs,
  )
  .await?;

  Ok(())
}

fn init_v8_flags(v8_flags: &[String]) {
  let v8_flags_includes_help = v8_flags
    .iter()
    .any(|flag| flag == "-help" || flag == "--help");
  // Keep in sync with `standalone.rs`.
  let v8_flags = once("UNUSED_BUT_NECESSARY_ARG0".to_owned())
    .chain(v8_flags.iter().cloned())
    .collect::<Vec<_>>();
  let unrecognized_v8_flags = v8_set_flags(v8_flags)
    .into_iter()
    .skip(1)
    .collect::<Vec<_>>();
  if !unrecognized_v8_flags.is_empty() {
    for f in unrecognized_v8_flags {
      eprintln!("error: V8 did not recognize flag '{}'", f);
    }
    eprintln!("\nFor a list of V8 flags, use '--v8-flags=--help'");
    std::process::exit(1);
  }
  if v8_flags_includes_help {
    std::process::exit(0);
  }
}

fn get_subcommand(
  flags: Flags,
) -> Pin<Box<dyn Future<Output = Result<(), AnyError>>>> {
  match flags.clone().subcommand {
    DenoSubcommand::Bundle(bundle_flags) => {
      bundle_command(flags, bundle_flags).boxed_local()
    }
    DenoSubcommand::Doc(doc_flags) => {
      doc_command(flags, doc_flags).boxed_local()
    }
    DenoSubcommand::Eval(eval_flags) => {
      eval_command(flags, eval_flags).boxed_local()
    }
    DenoSubcommand::Cache(cache_flags) => {
      cache_command(flags, cache_flags).boxed_local()
    }
    DenoSubcommand::Compile(compile_flags) => {
      compile_command(flags, compile_flags).boxed_local()
    }
    DenoSubcommand::Coverage(coverage_flags) => {
      coverage_command(flags, coverage_flags).boxed_local()
    }
    DenoSubcommand::Fmt(fmt_flags) => {
      format_command(flags, fmt_flags).boxed_local()
    }
    DenoSubcommand::Info(info_flags) => {
      info_command(flags, info_flags).boxed_local()
    }
    DenoSubcommand::Install(install_flags) => {
      install_command(flags, install_flags).boxed_local()
    }
    DenoSubcommand::Lsp => lsp_command().boxed_local(),
    DenoSubcommand::Lint(lint_flags) => {
      lint_command(flags, lint_flags).boxed_local()
    }
    DenoSubcommand::Repl(repl_flags) => {
      run_repl(flags, repl_flags).boxed_local()
    }
    DenoSubcommand::Run(run_flags) => {
      run_command(flags, run_flags).boxed_local()
    }
    DenoSubcommand::Test(test_flags) => {
      test_command(flags, test_flags).boxed_local()
    }
    DenoSubcommand::Completions(CompletionsFlags { buf }) => {
      if let Err(e) = write_to_stdout_ignore_sigpipe(&buf) {
        eprintln!("{}", e);
        std::process::exit(1);
      }
      std::process::exit(0);
    }
    DenoSubcommand::Types => {
      let types = get_types(flags.unstable);
      if let Err(e) = write_to_stdout_ignore_sigpipe(types.as_bytes()) {
        eprintln!("{}", e);
        std::process::exit(1);
      }
      std::process::exit(0);
    }
    DenoSubcommand::Upgrade(upgrade_flags) => {
      let UpgradeFlags {
        force,
        dry_run,
        canary,
        version,
        output,
        ca_file,
      } = upgrade_flags;
      tools::upgrade::upgrade_command(
        dry_run, force, canary, version, output, ca_file,
      )
      .boxed_local()
    }
  }
}

fn setup_exit_process_panic_hook() {
  // tokio does not exit the process when a task panics, so we
  // define a custom panic hook to implement this behaviour
  let orig_hook = std::panic::take_hook();
  std::panic::set_hook(Box::new(move |panic_info| {
    orig_hook(panic_info);
    std::process::exit(1);
  }));
}

fn unwrap_or_exit<T>(result: Result<T, AnyError>) -> T {
  match result {
    Ok(value) => value,
    Err(error) => {
      eprintln!("{}: {:?}", colors::red_bold("error"), error);
      std::process::exit(1);
    }
  }
}

pub fn main() {
  setup_exit_process_panic_hook();
  #[cfg(windows)]
  colors::enable_ansi(); // For Windows 10
  unix_util::raise_fd_limit();

  let args: Vec<String> = env::args().collect();
  let standalone_res = match standalone::extract_standalone(args.clone()) {
    Ok(Some((metadata, bundle))) => {
      tokio_util::run_basic(standalone::run(bundle, metadata))
    }
    Ok(None) => Ok(()),
    Err(err) => Err(err),
  };
  if let Err(err) = standalone_res {
    eprintln!("{}: {}", colors::red_bold("error"), err.to_string());
    std::process::exit(1);
  }

  let flags = match flags::flags_from_vec(args) {
    Ok(flags) => flags,
    Err(err @ clap::Error { .. })
      if err.kind == clap::ErrorKind::HelpDisplayed
        || err.kind == clap::ErrorKind::VersionDisplayed =>
    {
      err.write_to(&mut std::io::stdout()).unwrap();
      std::io::stdout().write_all(b"\n").unwrap();
      std::process::exit(0);
    }
    Err(err) => unwrap_or_exit(Err(AnyError::from(err))),
  };
  if !flags.v8_flags.is_empty() {
    init_v8_flags(&*flags.v8_flags);
  }

  logger::init(flags.log_level);

  unwrap_or_exit(tokio_util::run_basic(get_subcommand(flags)));
}
