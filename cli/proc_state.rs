// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use crate::cache;
use crate::cache::Cacher;
use crate::colors;
use crate::compat;
use crate::compat::NodeEsmResolver;
use crate::config_file::ConfigFile;
use crate::config_file::MaybeImportsResult;
use crate::deno_dir;
use crate::emit;
use crate::errors::get_error_class_name;
use crate::file_fetcher::CacheSetting;
use crate::file_fetcher::FileFetcher;
use crate::flags;
use crate::http_cache;
use crate::lockfile::as_maybe_locker;
use crate::lockfile::Lockfile;
use crate::resolver::ImportMapResolver;
use crate::resolver::JsxResolver;
use crate::source_maps::SourceMapGetter;
use crate::version;

use deno_core::anyhow::anyhow;
use deno_core::anyhow::Context;
use deno_core::error::custom_error;
use deno_core::error::AnyError;
use deno_core::parking_lot::Mutex;
use deno_core::resolve_url;
use deno_core::url::Url;
use deno_core::CompiledWasmModuleStore;
use deno_core::ModuleSource;
use deno_core::ModuleSpecifier;
use deno_core::SharedArrayBufferStore;
use deno_graph::create_graph;
use deno_graph::Dependency;
use deno_graph::MediaType;
use deno_graph::ModuleGraphError;
use deno_graph::Range;
use deno_runtime::deno_broadcast_channel::InMemoryBroadcastChannel;
use deno_runtime::deno_web::BlobStore;
use deno_runtime::inspector_server::InspectorServer;
use deno_runtime::permissions::Permissions;
use deno_tls::rustls::RootCertStore;
use deno_tls::rustls_native_certs::load_native_certs;
use deno_tls::webpki_roots::TLS_SERVER_ROOTS;
use import_map::ImportMap;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::env;
use std::fs::File;
use std::io::BufReader;
use std::ops::Deref;
use std::sync::Arc;

/// This structure represents state of single "deno" program.
///
/// It is shared by all created workers (thus V8 isolates).
#[derive(Clone)]
pub struct ProcState(Arc<Inner>);

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
enum ModuleEntry {
  Module {
    code: String,
    dependencies: BTreeMap<String, Dependency>,
  },
  Error(ModuleGraphError),
  Redirect(ModuleSpecifier),
}

#[derive(Default)]
struct GraphData {
  modules: HashMap<ModuleSpecifier, ModuleEntry>,
  /// A set of type libs that each module has passed a type check with this
  /// session. This would consist of window, worker or both.
  checked_libs_map: HashMap<ModuleSpecifier, HashSet<emit::TypeLib>>,
  /// Map of first known referrer locations for each module. Used to enhance
  /// error messages.
  referrer_map: HashMap<ModuleSpecifier, Range>,
}

impl GraphData {
  /// Check if `roots` are ready to be loaded by V8. Returns `Some(Ok(()))` if
  /// prepared. Returns `Some(Err(_))` if there is a known module graph error
  /// statically reachable from `roots`. Returns `None` if sufficient graph data
  /// is yet to be supplied.
  fn check_if_prepared(
    &self,
    roots: &[ModuleSpecifier],
  ) -> Option<Result<(), AnyError>> {
    let mut seen = HashSet::<&ModuleSpecifier>::new();
    let mut visiting = VecDeque::<&ModuleSpecifier>::new();
    for root in roots {
      visiting.push_back(root);
    }
    while let Some(specifier) = visiting.pop_front() {
      match self.modules.get(specifier) {
        Some(ModuleEntry::Module { dependencies, .. }) => {
          for (_, dep) in dependencies.iter().rev() {
            for resolved in [&dep.maybe_code, &dep.maybe_type] {
              if !dep.is_dynamic {
                match resolved {
                  Some(Ok((dep_specifier, _))) => {
                    if !dep.is_dynamic && !seen.contains(dep_specifier) {
                      seen.insert(dep_specifier);
                      visiting.push_front(dep_specifier);
                    }
                  }
                  Some(Err(error)) => {
                    let range = error.range();
                    if !range.specifier.as_str().contains("$deno") {
                      return Some(Err(custom_error(
                        get_error_class_name(&error.clone().into()),
                        format!("{}\n    at {}", error.to_string(), range),
                      )));
                    }
                    return Some(Err(error.clone().into()));
                  }
                  None => {}
                }
              }
            }
          }
        }
        Some(ModuleEntry::Error(error)) => {
          if !roots.contains(specifier) {
            if let Some(range) = self.referrer_map.get(specifier) {
              if !range.specifier.as_str().contains("$deno") {
                let message = error.to_string();
                return Some(Err(custom_error(
                  get_error_class_name(&error.clone().into()),
                  format!("{}\n    at {}", message, range),
                )));
              }
            }
          }
          return Some(Err(error.clone().into()));
        }
        Some(ModuleEntry::Redirect(specifier)) => {
          seen.insert(specifier);
          visiting.push_front(specifier);
        }
        None => return None,
      }
    }
    Some(Ok(()))
  }
}

pub struct Inner {
  /// Flags parsed from `argv` contents.
  pub flags: flags::Flags,
  pub dir: deno_dir::DenoDir,
  pub coverage_dir: Option<String>,
  pub file_fetcher: FileFetcher,
  graph_data: Arc<Mutex<GraphData>>,
  pub lockfile: Option<Arc<Mutex<Lockfile>>>,
  pub maybe_config_file: Option<ConfigFile>,
  pub maybe_import_map: Option<Arc<ImportMap>>,
  pub maybe_inspector_server: Option<Arc<InspectorServer>>,
  pub root_cert_store: Option<RootCertStore>,
  pub blob_store: BlobStore,
  pub broadcast_channel: InMemoryBroadcastChannel,
  pub shared_array_buffer_store: SharedArrayBufferStore,
  pub compiled_wasm_module_store: CompiledWasmModuleStore,
  maybe_resolver: Option<Arc<dyn deno_graph::source::Resolver + Send + Sync>>,
}

impl Deref for ProcState {
  type Target = Arc<Inner>;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl ProcState {
  pub async fn build(flags: flags::Flags) -> Result<Self, AnyError> {
    let maybe_custom_root = flags
      .cache_path
      .clone()
      .or_else(|| env::var("DENO_DIR").map(String::into).ok());
    let dir = deno_dir::DenoDir::new(maybe_custom_root)?;
    let deps_cache_location = dir.root.join("deps");
    let http_cache = http_cache::HttpCache::new(&deps_cache_location);

    let mut root_cert_store = RootCertStore::empty();
    let ca_stores: Vec<String> = flags
      .ca_stores
      .clone()
      .or_else(|| {
        let env_ca_store = env::var("DENO_TLS_CA_STORE").ok()?;
        Some(
          env_ca_store
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        )
      })
      .unwrap_or_else(|| vec!["mozilla".to_string()]);

    for store in ca_stores.iter() {
      match store.as_str() {
        "mozilla" => {
          root_cert_store.add_server_trust_anchors(&TLS_SERVER_ROOTS);
        }
        "system" => {
          let roots = load_native_certs()
            .expect("could not load platform certs")
            .roots;
          root_cert_store.roots.extend(roots);
        }
        _ => {
          return Err(anyhow!("Unknown certificate store \"{}\" specified (allowed: \"system,mozilla\")", store));
        }
      }
    }

    let ca_file = flags.ca_file.clone().or_else(|| env::var("DENO_CERT").ok());
    if let Some(ca_file) = ca_file {
      let certfile = File::open(&ca_file)?;
      let mut reader = BufReader::new(certfile);

      // This function does not return specific errors, if it fails give a generic message.
      if let Err(_err) = root_cert_store.add_pem_file(&mut reader) {
        return Err(anyhow!("Unable to add pem file to certificate store"));
      }
    }

    if let Some(insecure_allowlist) =
      flags.unsafely_ignore_certificate_errors.as_ref()
    {
      let domains = if insecure_allowlist.is_empty() {
        "for all hostnames".to_string()
      } else {
        format!("for: {}", insecure_allowlist.join(", "))
      };
      let msg =
        format!("DANGER: TLS certificate validation is disabled {}", domains);
      eprintln!("{}", colors::yellow(msg));
    }

    let cache_usage = if flags.cached_only {
      CacheSetting::Only
    } else if !flags.cache_blocklist.is_empty() {
      CacheSetting::ReloadSome(flags.cache_blocklist.clone())
    } else if flags.reload {
      CacheSetting::ReloadAll
    } else {
      CacheSetting::Use
    };

    let blob_store = BlobStore::default();
    let broadcast_channel = InMemoryBroadcastChannel::default();
    let shared_array_buffer_store = SharedArrayBufferStore::default();
    let compiled_wasm_module_store = CompiledWasmModuleStore::default();

    let file_fetcher = FileFetcher::new(
      http_cache,
      cache_usage,
      !flags.no_remote,
      Some(root_cert_store.clone()),
      blob_store.clone(),
      flags.unsafely_ignore_certificate_errors.clone(),
    )?;

    let lockfile = if let Some(filename) = &flags.lock {
      let lockfile = Lockfile::new(filename.clone(), flags.lock_write)?;
      Some(Arc::new(Mutex::new(lockfile)))
    } else {
      None
    };

    let maybe_config_file =
      if let Some(config_path) = flags.config_path.as_ref() {
        Some(ConfigFile::read(config_path)?)
      } else {
        None
      };

    let maybe_import_map: Option<Arc<ImportMap>> =
      match flags.import_map_path.as_ref() {
        None => None,
        Some(import_map_url) => {
          let import_map_specifier =
            deno_core::resolve_url_or_path(import_map_url).context(format!(
              "Bad URL (\"{}\") for import map.",
              import_map_url
            ))?;
          let file = file_fetcher
            .fetch(&import_map_specifier, &mut Permissions::allow_all())
            .await
            .context(format!(
              "Unable to load '{}' import map",
              import_map_specifier
            ))?;
          let import_map =
            ImportMap::from_json(import_map_specifier.as_str(), &file.source)?;
          Some(Arc::new(import_map))
        }
      };

    let maybe_inspect_host = flags.inspect.or(flags.inspect_brk);
    let maybe_inspector_server = maybe_inspect_host.map(|host| {
      Arc::new(InspectorServer::new(host, version::get_user_agent()))
    });

    let coverage_dir = flags
      .coverage_dir
      .clone()
      .or_else(|| env::var("DENO_UNSTABLE_COVERAGE_DIR").ok());

    // FIXME(bartlomieju): `NodeEsmResolver` is not aware of JSX resolver
    // created below
    let node_resolver = NodeEsmResolver::new(
      maybe_import_map.clone().map(ImportMapResolver::new),
    );
    let maybe_import_map_resolver =
      maybe_import_map.clone().map(ImportMapResolver::new);
    let maybe_jsx_resolver = maybe_config_file
      .as_ref()
      .map(|cf| {
        cf.to_maybe_jsx_import_source_module()
          .map(|im| JsxResolver::new(im, maybe_import_map_resolver.clone()))
      })
      .flatten();
    let maybe_resolver: Option<
      Arc<dyn deno_graph::source::Resolver + Send + Sync>,
    > = if flags.compat {
      Some(Arc::new(node_resolver))
    } else if let Some(jsx_resolver) = maybe_jsx_resolver {
      // the JSX resolver offloads to the import map if present, otherwise uses
      // the default Deno explicit import resolution.
      Some(Arc::new(jsx_resolver))
    } else if let Some(import_map_resolver) = maybe_import_map_resolver {
      Some(Arc::new(import_map_resolver))
    } else {
      None
    };

    Ok(ProcState(Arc::new(Inner {
      dir,
      coverage_dir,
      flags,
      file_fetcher,
      graph_data: Default::default(),
      lockfile,
      maybe_config_file,
      maybe_import_map,
      maybe_inspector_server,
      root_cert_store: Some(root_cert_store.clone()),
      blob_store,
      broadcast_channel,
      shared_array_buffer_store,
      compiled_wasm_module_store,
      maybe_resolver,
    })))
  }

  /// Return any imports that should be brought into the scope of the module
  /// graph.
  fn get_maybe_imports(&self) -> MaybeImportsResult {
    let mut imports = Vec::new();
    if let Some(config_file) = &self.maybe_config_file {
      if let Some(config_imports) = config_file.to_maybe_imports()? {
        imports.extend(config_imports);
      }
    }
    if self.flags.compat {
      imports.extend(compat::get_node_imports());
    }
    if imports.is_empty() {
      Ok(None)
    } else {
      Ok(Some(imports))
    }
  }

  /// This method must be called for a module or a static importer of that
  /// module before attempting to `load()` it from a `JsRuntime`. It will
  /// populate `self.graph_data` in memory with the necessary source code or
  /// report any module graph / type checking errors.
  pub(crate) async fn prepare_module_load(
    &self,
    roots: Vec<ModuleSpecifier>,
    is_dynamic: bool,
    lib: emit::TypeLib,
    root_permissions: Permissions,
    dynamic_permissions: Permissions,
    reload_on_watch: bool,
  ) -> Result<(), AnyError> {
    // TODO(bartlomieju): this is very make-shift, is there an existing API
    // that we could include it like with "maybe_imports"?
    let roots = if self.flags.compat {
      let mut r = vec![compat::GLOBAL_URL.clone()];
      r.extend(roots);
      r
    } else {
      roots
    };
    if !reload_on_watch {
      let graph_data = self.graph_data.lock();
      if self.flags.no_check
        || roots.iter().all(|root| {
          graph_data
            .checked_libs_map
            .get(root)
            .map_or(false, |checked_libs| checked_libs.contains(&lib))
        })
      {
        if let Some(result) = graph_data.check_if_prepared(&roots) {
          return result;
        }
      }
    }
    let mut cache = cache::FetchCacher::new(
      self.dir.gen_cache.clone(),
      self.file_fetcher.clone(),
      root_permissions.clone(),
      dynamic_permissions.clone(),
    );
    let maybe_locker = as_maybe_locker(self.lockfile.clone());
    let maybe_imports = self.get_maybe_imports()?;
    let maybe_resolver: Option<&dyn deno_graph::source::Resolver> =
      if let Some(resolver) = &self.maybe_resolver {
        Some(resolver.as_ref())
      } else {
        None
      };
    let graph = create_graph(
      roots.clone(),
      is_dynamic,
      maybe_imports,
      &mut cache,
      maybe_resolver,
      maybe_locker,
      None,
    )
    .await;
    // If there was a locker, validate the integrity of all the modules in the
    // locker.
    emit::lock(&graph);

    // Determine any modules that have already been emitted this session and
    // should be skipped.
    let reload_exclusions: HashSet<ModuleSpecifier> = {
      let graph_data = self.graph_data.lock();
      graph_data.modules.keys().cloned().collect()
    };

    let config_type = if self.flags.no_check {
      emit::ConfigType::Emit
    } else {
      emit::ConfigType::Check {
        tsc_emit: true,
        lib: lib.clone(),
      }
    };

    let (ts_config, maybe_ignored_options) =
      emit::get_ts_config(config_type, self.maybe_config_file.as_ref(), None)?;
    let graph = Arc::new(graph);

    let mut type_check_result = Ok(());

    if emit::valid_emit(
      graph.as_ref(),
      &cache,
      &ts_config,
      self.flags.reload,
      &reload_exclusions,
    ) {
      if let Some(root) = graph.roots.get(0) {
        log::debug!("specifier \"{}\" and dependencies have valid emit, skipping checking and emitting", root);
      } else {
        log::debug!("rootless graph, skipping checking and emitting");
      }
    } else {
      if let Some(ignored_options) = maybe_ignored_options {
        log::warn!("{}", ignored_options);
      }
      let emit_result = if self.flags.no_check {
        let options = emit::EmitOptions {
          ts_config,
          reload_exclusions,
          reload: self.flags.reload,
        };
        emit::emit(graph.as_ref(), &mut cache, options)?
      } else {
        // here, we are type checking, so we want to error here if any of the
        // type only dependencies are missing or we have other errors with them
        // where as if we are not type checking, we shouldn't care about these
        // errors, and they don't get returned in `graph.valid()` above.
        graph.valid_types_only()?;

        let maybe_config_specifier = self
          .maybe_config_file
          .as_ref()
          .map(|cf| ModuleSpecifier::from_file_path(&cf.path).unwrap());
        let options = emit::CheckOptions {
          debug: self.flags.log_level == Some(log::Level::Debug),
          emit_with_diagnostics: false,
          maybe_config_specifier,
          ts_config,
          reload: self.flags.reload,
        };
        for root in &graph.roots {
          let root_str = root.to_string();
          // `$deno` specifiers are internal specifiers, printing out that
          // they are being checked is confusing to a user, since they don't
          // actually exist, so we will simply indicate that a generated module
          // is being checked instead of the cryptic internal module
          if !root_str.contains("$deno") {
            log::info!("{} {}", colors::green("Check"), root);
          } else {
            log::info!("{} a generated module", colors::green("Check"))
          }
        }
        emit::check_and_maybe_emit(graph.clone(), &mut cache, options)?
      };
      log::debug!("{}", emit_result.stats);
      if !emit_result.diagnostics.is_empty() {
        type_check_result = Err(anyhow!(emit_result.diagnostics));
      }
    }

    {
      let mut graph_data = self.graph_data.lock();
      let mut specifiers = graph.specifiers();
      // Set specifier results for redirects.
      // TODO(nayeemrmn): This should be done in `ModuleGraph::specifiers()`.
      for (specifier, found) in &graph.redirects {
        let actual = specifiers.get(found).unwrap().clone();
        specifiers.insert(specifier.clone(), actual);
      }
      for (specifier, result) in &specifiers {
        if let Some(found) = graph.redirects.get(specifier) {
          let module_entry = ModuleEntry::Redirect(found.clone());
          graph_data.modules.insert(specifier.clone(), module_entry);
          continue;
        }
        match result {
          Ok((_, media_type)) => {
            let module = graph.get(specifier).unwrap();
            // If there was a type check error, supply dummy code. It shouldn't
            // be used since preparation will fail.
            let code = if type_check_result.is_err() {
              "".to_string()
            // Check to see if there is an emitted file in the cache.
            } else if let Some(code) =
              cache.get(cache::CacheType::Emit, specifier)
            {
              code
            // Then if the file is JavaScript (or unknown) and wasn't emitted,
            // we will load the original source code in the module.
            } else if matches!(
              media_type,
              MediaType::JavaScript
                | MediaType::Unknown
                | MediaType::Cjs
                | MediaType::Mjs
            ) {
              module.source.as_str().to_string()
            // The emit may also be missing when a `.dts` file is in the
            // graph. There shouldn't be any runtime statements in the source
            // file and if there was, users would be shown a `TS1036`
            // diagnostic. So just return an empty emit.
            } else if media_type == &MediaType::Dts {
              "".to_string()
            } else {
              unreachable!("unexpected missing emit: {}", specifier)
            };
            let dependencies = module.dependencies.clone();
            let module_entry = ModuleEntry::Module { code, dependencies };
            graph_data.modules.insert(specifier.clone(), module_entry);
            for dep in module.dependencies.values() {
              #[allow(clippy::manual_flatten)]
              for resolved in [&dep.maybe_code, &dep.maybe_type] {
                if let Some(Ok((specifier, referrer_range))) = resolved {
                  let specifier =
                    graph.redirects.get(specifier).unwrap_or(specifier);
                  let entry = graph_data.referrer_map.entry(specifier.clone());
                  entry.or_insert_with(|| referrer_range.clone());
                }
              }
            }
          }
          Err(error) => {
            let module_entry = ModuleEntry::Error(error.clone());
            graph_data.modules.insert(specifier.clone(), module_entry);
          }
        }
      }

      graph_data.check_if_prepared(&roots).unwrap()?;
      type_check_result?;

      if !self.flags.no_check {
        for specifier in specifiers.keys() {
          let checked_libs = graph_data
            .checked_libs_map
            .entry(specifier.clone())
            .or_default();
          checked_libs.insert(lib.clone());
        }
      }
    }

    // any updates to the lockfile should be updated now
    if let Some(ref lockfile) = self.lockfile {
      let g = lockfile.lock();
      g.write()?;
    }

    Ok(())
  }

  pub(crate) fn resolve(
    &self,
    specifier: &str,
    referrer: &str,
  ) -> Result<ModuleSpecifier, AnyError> {
    if let Ok(referrer) = deno_core::resolve_url_or_path(referrer) {
      let graph_data = self.graph_data.lock();
      let found_referrer = match graph_data.modules.get(&referrer) {
        Some(ModuleEntry::Redirect(r)) => r,
        _ => &referrer,
      };
      let maybe_resolved = match graph_data.modules.get(found_referrer) {
        Some(ModuleEntry::Module { dependencies, .. }) => dependencies
          .get(specifier)
          .and_then(|dep| dep.maybe_code.clone()),
        _ => None,
      };

      match maybe_resolved {
        Some(Ok((specifier, _))) => return Ok(specifier),
        Some(Err(err)) => {
          return Err(custom_error(
            "TypeError",
            format!("{}\n", err.to_string_with_range()),
          ))
        }
        None => {}
      }
    }

    // FIXME(bartlomieju): this is a hacky way to provide compatibility with REPL
    // and `Deno.core.evalContext` API. Ideally we should always have a referrer filled
    // but sadly that's not the case due to missing APIs in V8.
    let referrer = if referrer.is_empty() && self.flags.repl {
      deno_core::resolve_url_or_path("./$deno$repl.ts").unwrap()
    } else {
      deno_core::resolve_url_or_path(referrer).unwrap()
    };

    let maybe_resolver: Option<&dyn deno_graph::source::Resolver> =
      if let Some(resolver) = &self.maybe_resolver {
        Some(resolver.as_ref())
      } else {
        None
      };
    if let Some(resolver) = &maybe_resolver {
      resolver.resolve(specifier, &referrer)
    } else {
      deno_core::resolve_import(specifier, referrer.as_str())
        .map_err(|err| err.into())
    }
  }

  pub fn load(
    &self,
    specifier: ModuleSpecifier,
    maybe_referrer: Option<ModuleSpecifier>,
    is_dynamic: bool,
  ) -> Result<ModuleSource, AnyError> {
    log::debug!(
      "specifier: {} maybe_referrer: {} is_dynamic: {}",
      specifier,
      maybe_referrer
        .as_ref()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "<none>".to_string()),
      is_dynamic
    );

    let graph_data = self.graph_data.lock();
    let found_specifier = match graph_data.modules.get(&specifier) {
      Some(ModuleEntry::Redirect(s)) => s,
      _ => &specifier,
    };
    match graph_data.modules.get(found_specifier) {
      Some(ModuleEntry::Module { code, .. }) => Ok(ModuleSource {
        code: code.clone(),
        module_url_specified: specifier.to_string(),
        module_url_found: found_specifier.to_string(),
      }),
      _ => Err(anyhow!(
        "Loading unprepared module: {}",
        specifier.to_string()
      )),
    }
  }

  // TODO(@kitsonk) this should be refactored to get it from the module graph
  fn get_emit(&self, url: &Url) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let emit_path = self
      .dir
      .gen_cache
      .get_cache_filename_with_extension(url, "js")?;
    let emit_map_path = self
      .dir
      .gen_cache
      .get_cache_filename_with_extension(url, "js.map")?;
    if let Ok(code) = self.dir.gen_cache.get(&emit_path) {
      let maybe_map = if let Ok(map) = self.dir.gen_cache.get(&emit_map_path) {
        Some(map)
      } else {
        None
      };
      Some((code, maybe_map))
    } else {
      None
    }
  }
}

// TODO(@kitsonk) this is only temporary, but should be refactored to somewhere
// else, like a refactored file_fetcher.
impl SourceMapGetter for ProcState {
  fn get_source_map(&self, file_name: &str) -> Option<Vec<u8>> {
    if let Ok(specifier) = resolve_url(file_name) {
      match specifier.scheme() {
        // we should only be looking for emits for schemes that denote external
        // modules, which the disk_cache supports
        "wasm" | "file" | "http" | "https" | "data" | "blob" => (),
        _ => return None,
      }
      if let Some((code, maybe_map)) = self.get_emit(&specifier) {
        let code = String::from_utf8(code).unwrap();
        source_map_from_code(code).or(maybe_map)
      } else if let Ok(source) = self.load(specifier, None, false) {
        source_map_from_code(source.code)
      } else {
        None
      }
    } else {
      None
    }
  }

  fn get_source_line(
    &self,
    file_name: &str,
    line_number: usize,
  ) -> Option<String> {
    if let Ok(specifier) = resolve_url(file_name) {
      self.file_fetcher.get_source(&specifier).map(|out| {
        // Do NOT use .lines(): it skips the terminating empty line.
        // (due to internally using .split_terminator() instead of .split())
        let lines: Vec<&str> = out.source.split('\n').collect();
        if line_number >= lines.len() {
          format!(
            "{} Couldn't format source line: Line {} is out of bounds (source may have changed at runtime)",
            crate::colors::yellow("Warning"), line_number + 1,
          )
        } else {
          lines[line_number].to_string()
        }
      })
    } else {
      None
    }
  }
}

fn source_map_from_code(code: String) -> Option<Vec<u8>> {
  let lines: Vec<&str> = code.split('\n').collect();
  if let Some(last_line) = lines.last() {
    if last_line
      .starts_with("//# sourceMappingURL=data:application/json;base64,")
    {
      let input = last_line.trim_start_matches(
        "//# sourceMappingURL=data:application/json;base64,",
      );
      let decoded_map = base64::decode(input)
        .expect("Unable to decode source map from emitted file.");
      Some(decoded_map)
    } else {
      None
    }
  } else {
    None
  }
}
