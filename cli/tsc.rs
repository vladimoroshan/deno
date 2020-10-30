// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use crate::ast::parse;
use crate::ast::Location;
use crate::diagnostics::Diagnostics;
use crate::disk_cache::DiskCache;
use crate::file_fetcher::SourceFile;
use crate::file_fetcher::SourceFileFetcher;
use crate::flags::Flags;
use crate::fs::canonicalize_path;
use crate::js;
use crate::media_type::MediaType;
use crate::module_graph::ModuleGraph;
use crate::module_graph::ModuleGraphLoader;
use crate::permissions::Permissions;
use crate::program_state::ProgramState;
use crate::tsc_config;
use crate::version;
use deno_core::error::generic_error;
use deno_core::error::AnyError;
use deno_core::error::JsError;
use deno_core::json_op_sync;
use deno_core::serde_json;
use deno_core::serde_json::json;
use deno_core::serde_json::Value;
use deno_core::url::Url;
use deno_core::JsRuntime;
use deno_core::ModuleSpecifier;
use deno_core::RuntimeOptions;
use log::debug;
use regex::Regex;
use serde::Deserialize;
use serde::Serialize;
use serde::Serializer;
use sourcemap::SourceMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::ops::Deref;
use std::path::PathBuf;
use std::str;
use std::sync::Arc;
use std::sync::Mutex;
use swc_common::comments::Comment;
use swc_common::comments::CommentKind;
use swc_ecmascript::dep_graph;

pub const AVAILABLE_LIBS: &[&str] = &[
  "deno.ns",
  "deno.window",
  "deno.worker",
  "deno.shared_globals",
  "deno.unstable",
  "dom",
  "dom.iterable",
  "es5",
  "es6",
  "esnext",
  "es2020",
  "es2020.full",
  "es2019",
  "es2019.full",
  "es2018",
  "es2018.full",
  "es2017",
  "es2017.full",
  "es2016",
  "es2016.full",
  "es2015",
  "es2015.collection",
  "es2015.core",
  "es2015.generator",
  "es2015.iterable",
  "es2015.promise",
  "es2015.proxy",
  "es2015.reflect",
  "es2015.symbol",
  "es2015.symbol.wellknown",
  "es2016.array.include",
  "es2017.intl",
  "es2017.object",
  "es2017.sharedmemory",
  "es2017.string",
  "es2017.typedarrays",
  "es2018.asyncgenerator",
  "es2018.asynciterable",
  "es2018.intl",
  "es2018.promise",
  "es2018.regexp",
  "es2019.array",
  "es2019.object",
  "es2019.string",
  "es2019.symbol",
  "es2020.bigint",
  "es2020.promise",
  "es2020.string",
  "es2020.symbol.wellknown",
  "esnext.array",
  "esnext.asynciterable",
  "esnext.bigint",
  "esnext.intl",
  "esnext.promise",
  "esnext.string",
  "esnext.symbol",
  "esnext.weakref",
  "scripthost",
  "webworker",
  "webworker.importscripts",
];

#[derive(Debug, Clone)]
pub struct CompiledModule {
  pub code: String,
  pub name: String,
}

lazy_static! {
  /// Matches the `@deno-types` pragma.
  static ref DENO_TYPES_RE: Regex =
    Regex::new(r#"(?i)^\s*@deno-types\s*=\s*(?:["']([^"']+)["']|(\S+))"#)
      .unwrap();
  /// Matches a `/// <reference ... />` comment reference.
  static ref TRIPLE_SLASH_REFERENCE_RE: Regex =
    Regex::new(r"(?i)^/\s*<reference\s.*?/>").unwrap();
  /// Matches a path reference, which adds a dependency to a module
  static ref PATH_REFERENCE_RE: Regex =
    Regex::new(r#"(?i)\spath\s*=\s*["']([^"']*)["']"#).unwrap();
  /// Matches a types reference, which for JavaScript files indicates the
  /// location of types to use when type checking a program that includes it as
  /// a dependency.
  static ref TYPES_REFERENCE_RE: Regex =
    Regex::new(r#"(?i)\stypes\s*=\s*["']([^"']*)["']"#).unwrap();
  /// Matches a lib reference.
  static ref LIB_REFERENCE_RE: Regex =
    Regex::new(r#"(?i)\slib\s*=\s*["']([^"']*)["']"#).unwrap();
}

#[derive(Clone, Eq, PartialEq)]
pub enum TargetLib {
  Main,
  Worker,
}

/// Struct which represents the state of the compiler
/// configuration where the first is canonical name for the configuration file,
/// second is a vector of the bytes of the contents of the configuration file,
/// third is bytes of the hash of contents.
#[derive(Clone)]
pub struct CompilerConfig {
  pub path: Option<PathBuf>,
  pub options: Value,
  pub maybe_ignored_options: Option<tsc_config::IgnoredCompilerOptions>,
  pub hash: String,
  pub compile_js: bool,
}

impl CompilerConfig {
  /// Take the passed flag and resolve the file name relative to the cwd.
  pub fn load(maybe_config_path: Option<String>) -> Result<Self, AnyError> {
    if maybe_config_path.is_none() {
      return Ok(Self {
        path: Some(PathBuf::new()),
        options: json!({}),
        maybe_ignored_options: None,
        hash: "".to_string(),
        compile_js: false,
      });
    }

    let raw_config_path = maybe_config_path.unwrap();
    debug!("Compiler config file: {}", raw_config_path);
    let cwd = std::env::current_dir().unwrap();
    let config_file = cwd.join(raw_config_path);

    // Convert the PathBuf to a canonicalized string.  This is needed by the
    // compiler to properly deal with the configuration.
    let config_path = canonicalize_path(&config_file).map_err(|_| {
      io::Error::new(
        io::ErrorKind::InvalidInput,
        format!(
          "Could not find the config file: {}",
          config_file.to_string_lossy()
        ),
      )
    })?;

    // Load the contents of the configuration file
    debug!("Attempt to load config: {}", config_path.to_str().unwrap());
    let config_bytes = fs::read(&config_file)?;
    let config_hash = crate::checksum::gen(&[&config_bytes]);
    let config_str = String::from_utf8(config_bytes)?;

    let (options, maybe_ignored_options) = if config_str.is_empty() {
      (json!({}), None)
    } else {
      tsc_config::parse_config(&config_str, &config_path)?
    };

    // If `checkJs` is set to true in `compilerOptions` then we're gonna be compiling
    // JavaScript files as well
    let compile_js = options["checkJs"].as_bool().unwrap_or(false);

    Ok(Self {
      path: Some(config_path),
      options,
      maybe_ignored_options,
      hash: config_hash,
      compile_js,
    })
  }
}

/// Information associated with compiled file in cache.
/// version_hash is used to validate versions of the file
/// and could be used to remove stale file in cache.
#[derive(Deserialize, Serialize)]
pub struct CompiledFileMetadata {
  pub version_hash: String,
}

impl CompiledFileMetadata {
  pub fn to_json_string(&self) -> Result<String, serde_json::Error> {
    serde_json::to_string(self)
  }
}

/// Emit a SHA256 hash based on source code, deno version and TS config.
/// Used to check if a recompilation for source code is needed.
fn source_code_version_hash(
  source_code: &[u8],
  version: &str,
  config_hash: &[u8],
) -> String {
  crate::checksum::gen(&[source_code, version.as_bytes(), config_hash])
}

pub struct TsCompilerInner {
  pub file_fetcher: SourceFileFetcher,
  pub flags: Flags,
  pub config: CompilerConfig,
  pub disk_cache: DiskCache,
  /// Set of all URLs that have been compiled. This prevents double
  /// compilation of module.
  pub compiled: Mutex<HashSet<Url>>,
  /// This setting is controlled by `--reload` flag. Unless the flag
  /// is provided disk cache is used.
  pub use_disk_cache: bool,
  /// This setting is controlled by `compilerOptions.checkJs`
  pub compile_js: bool,
}

#[derive(Clone)]
pub struct TsCompiler(Arc<TsCompilerInner>);

impl Deref for TsCompiler {
  type Target = TsCompilerInner;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct Stat {
  key: String,
  value: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EmittedSource {
  filename: String,
  contents: String,
}

// TODO(bartlomieju): possible deduplicate once TS refactor is stabilized
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(unused)]
struct RuntimeBundleResponse {
  diagnostics: Diagnostics,
  output: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeCompileResponse {
  diagnostics: Diagnostics,
  emit_map: HashMap<String, EmittedSource>,
}

impl TsCompiler {
  pub fn new(
    file_fetcher: SourceFileFetcher,
    flags: Flags,
    disk_cache: DiskCache,
  ) -> Result<Self, AnyError> {
    let config = CompilerConfig::load(flags.config_path.clone())?;
    let use_disk_cache = !flags.reload;

    Ok(TsCompiler(Arc::new(TsCompilerInner {
      file_fetcher,
      flags,
      disk_cache,
      compile_js: config.compile_js,
      config,
      compiled: Mutex::new(HashSet::new()),
      use_disk_cache,
    })))
  }

  /// Mark given module URL as compiled to avoid multiple compilations of same
  /// module in single run.
  fn mark_compiled(&self, url: &Url) {
    let mut c = self.compiled.lock().unwrap();
    c.insert(url.clone());
  }

  fn cache_emitted_files(
    &self,
    emit_map: HashMap<String, EmittedSource>,
  ) -> std::io::Result<()> {
    for (emitted_name, source) in emit_map.iter() {
      let specifier = ModuleSpecifier::resolve_url(&source.filename)
        .expect("Should be a valid module specifier");

      let source_file = self
        .file_fetcher
        .fetch_cached_source_file(&specifier, Permissions::allow_all())
        .expect("Source file not found");

      // NOTE: JavaScript files are only cached to disk if `checkJs`
      // option in on
      if source_file.media_type == MediaType::JavaScript && !self.compile_js {
        continue;
      }

      if emitted_name.ends_with(".map") {
        self.cache_source_map(&specifier, &source.contents)?;
      } else if emitted_name.ends_with(".js") {
        self.cache_compiled_file(&specifier, source_file, &source.contents)?;
      } else {
        panic!("Trying to cache unknown file type {}", emitted_name);
      }
    }

    Ok(())
  }

  /// Save compiled JS file for given TS module to on-disk cache.
  ///
  /// Along compiled file a special metadata file is saved as well containing
  /// hash that can be validated to avoid unnecessary recompilation.
  fn cache_compiled_file(
    &self,
    module_specifier: &ModuleSpecifier,
    source_file: SourceFile,
    contents: &str,
  ) -> std::io::Result<()> {
    let js_key = self
      .disk_cache
      .get_cache_filename_with_extension(module_specifier.as_url(), "js");
    self.disk_cache.set(&js_key, contents.as_bytes())?;
    self.mark_compiled(module_specifier.as_url());

    let version_hash = source_code_version_hash(
      &source_file.source_code.as_bytes(),
      version::DENO,
      &self.config.hash.as_bytes(),
    );

    let compiled_file_metadata = CompiledFileMetadata { version_hash };
    let meta_key = self
      .disk_cache
      .get_cache_filename_with_extension(module_specifier.as_url(), "meta");
    self.disk_cache.set(
      &meta_key,
      compiled_file_metadata.to_json_string()?.as_bytes(),
    )
  }

  /// Save source map file for given TS module to on-disk cache.
  fn cache_source_map(
    &self,
    module_specifier: &ModuleSpecifier,
    contents: &str,
  ) -> std::io::Result<()> {
    let js_key = self
      .disk_cache
      .get_cache_filename_with_extension(module_specifier.as_url(), "js");
    let js_path = self.disk_cache.location.join(js_key);
    let js_file_url =
      Url::from_file_path(js_path).expect("Bad file URL for file");

    let source_map_key = self
      .disk_cache
      .get_cache_filename_with_extension(module_specifier.as_url(), "js.map");

    let mut sm = SourceMap::from_slice(contents.as_bytes())
      .expect("Invalid source map content");
    sm.set_file(Some(&js_file_url.to_string()));
    sm.set_source(0, &module_specifier.to_string());

    let mut output: Vec<u8> = vec![];
    sm.to_writer(&mut output)
      .expect("Failed to write source map");

    self.disk_cache.set(&source_map_key, &output)
  }
}

#[derive(Debug, Deserialize)]
struct CreateHashArgs {
  data: String,
}

fn execute_in_tsc(
  program_state: Arc<ProgramState>,
  req: String,
) -> Result<String, AnyError> {
  let mut js_runtime = JsRuntime::new(RuntimeOptions {
    startup_snapshot: Some(js::compiler_isolate_init()),
    ..Default::default()
  });

  let debug_flag = program_state
    .flags
    .log_level
    .map_or(false, |l| l == log::Level::Debug);
  let response = Arc::new(Mutex::new(None));

  {
    js_runtime.register_op(
      "op_fetch_asset",
      crate::op_fetch_asset::op_fetch_asset(HashMap::default()),
    );
    let res = response.clone();
    js_runtime.register_op(
      "op_compiler_respond",
      json_op_sync(move |_state, args, _bufs| {
        let mut response_slot = res.lock().unwrap();
        let replaced_value = response_slot.replace(args.to_string());
        assert!(
          replaced_value.is_none(),
          "op_compiler_respond found unexpected existing compiler output",
        );
        Ok(json!({}))
      }),
    );
    js_runtime.register_op(
      "op_create_hash",
      json_op_sync(move |_s, args, _bufs| {
        let v: CreateHashArgs = serde_json::from_value(args)?;
        let hash = crate::checksum::gen(&[v.data.as_bytes()]);
        Ok(json!({ "hash": hash }))
      }),
    );
  }

  let bootstrap_script = format!(
    "globalThis.startup({{ debugFlag: {}, legacy: true }})",
    debug_flag
  );
  js_runtime.execute("<compiler>", &bootstrap_script)?;

  let script = format!("globalThis.tsCompilerOnMessage({{ data: {} }});", req);
  js_runtime.execute("<compiler>", &script)?;

  let maybe_response = response.lock().unwrap().take();
  assert!(
    maybe_response.is_some(),
    "Unexpected missing response from TS compiler"
  );

  Ok(maybe_response.unwrap())
}

async fn create_runtime_module_graph(
  program_state: &Arc<ProgramState>,
  permissions: Permissions,
  root_name: &str,
  sources: &Option<HashMap<String, String>>,
  type_files: Vec<String>,
) -> Result<(Vec<String>, ModuleGraph), AnyError> {
  let mut root_names = vec![];
  let mut module_graph_loader = ModuleGraphLoader::new(
    program_state.file_fetcher.clone(),
    None,
    permissions,
    false,
    false,
  );

  if let Some(s_map) = sources {
    root_names.push(root_name.to_string());
    module_graph_loader.build_local_graph(root_name, s_map)?;
  } else {
    let module_specifier =
      ModuleSpecifier::resolve_import(root_name, "<unknown>")?;
    root_names.push(module_specifier.to_string());
    module_graph_loader
      .add_to_graph(&module_specifier, None)
      .await?;
  }

  // download all additional files from TSconfig and add them to root_names
  for type_file in type_files {
    let type_specifier = ModuleSpecifier::resolve_url_or_path(&type_file)?;
    module_graph_loader
      .add_to_graph(&type_specifier, None)
      .await?;
    root_names.push(type_specifier.to_string())
  }

  Ok((root_names, module_graph_loader.get_graph()))
}

fn extract_js_error(error: AnyError) -> AnyError {
  match error.downcast::<JsError>() {
    Ok(js_error) => {
      let msg = format!("Error in TS compiler:\n{}", js_error);
      generic_error(msg)
    }
    Err(error) => error,
  }
}

/// This function is used by `Deno.compile()` API.
pub async fn runtime_compile(
  program_state: &Arc<ProgramState>,
  permissions: Permissions,
  root_name: &str,
  sources: &Option<HashMap<String, String>>,
  maybe_options: &Option<String>,
) -> Result<Value, AnyError> {
  let mut user_options = if let Some(options) = maybe_options {
    tsc_config::parse_raw_config(options)?
  } else {
    json!({})
  };

  // Intentionally calling "take()" to replace value with `null` - otherwise TSC will try to load that file
  // using `fileExists` API
  let type_files = if let Some(types) = user_options["types"].take().as_array()
  {
    types
      .iter()
      .map(|type_value| type_value.as_str().unwrap_or("").to_string())
      .filter(|type_str| !type_str.is_empty())
      .collect()
  } else {
    vec![]
  };

  let unstable = program_state.flags.unstable;

  let mut lib = vec![];
  if let Some(user_libs) = user_options["lib"].take().as_array() {
    let libs = user_libs
      .iter()
      .map(|type_value| type_value.as_str().unwrap_or("").to_string())
      .filter(|type_str| !type_str.is_empty())
      .collect::<Vec<String>>();
    lib.extend(libs);
  } else {
    lib.push("deno.window".to_string());
  }

  if unstable {
    lib.push("deno.unstable".to_string());
  }

  let mut compiler_options = json!({
    "allowJs": false,
    "allowNonTsExtensions": true,
    "checkJs": false,
    "esModuleInterop": true,
    "isolatedModules": true,
    "jsx": "react",
    "module": "esnext",
    "sourceMap": true,
    "strict": true,
    "removeComments": true,
    "target": "esnext",
  });

  tsc_config::json_merge(&mut compiler_options, &user_options);
  tsc_config::json_merge(&mut compiler_options, &json!({ "lib": lib }));

  let (root_names, module_graph) = create_runtime_module_graph(
    &program_state,
    permissions.clone(),
    root_name,
    sources,
    type_files,
  )
  .await?;
  let module_graph_json =
    serde_json::to_value(module_graph).expect("Failed to serialize data");

  let req_msg = json!({
    "type": CompilerRequestType::RuntimeCompile,
    "target": "runtime",
    "rootNames": root_names,
    "sourceFileMap": module_graph_json,
    "compilerOptions": compiler_options,
  })
  .to_string();

  let compiler = program_state.ts_compiler.clone();

  let json_str =
    execute_in_tsc(program_state.clone(), req_msg).map_err(extract_js_error)?;
  let response: RuntimeCompileResponse = serde_json::from_str(&json_str)?;

  if response.diagnostics.is_empty() && sources.is_none() {
    compiler.cache_emitted_files(response.emit_map)?;
  }

  // We're returning `Ok()` instead of `Err()` because it's not runtime
  // error if there were diagnostics produced; we want to let user handle
  // diagnostics in the runtime.
  Ok(serde_json::from_str::<Value>(&json_str).unwrap())
}

/// This function is used by `Deno.bundle()` API.
pub async fn runtime_bundle(
  program_state: &Arc<ProgramState>,
  permissions: Permissions,
  root_name: &str,
  sources: &Option<HashMap<String, String>>,
  maybe_options: &Option<String>,
) -> Result<Value, AnyError> {
  let mut user_options = if let Some(options) = maybe_options {
    tsc_config::parse_raw_config(options)?
  } else {
    json!({})
  };

  // Intentionally calling "take()" to replace value with `null` - otherwise TSC will try to load that file
  // using `fileExists` API
  let type_files = if let Some(types) = user_options["types"].take().as_array()
  {
    types
      .iter()
      .map(|type_value| type_value.as_str().unwrap_or("").to_string())
      .filter(|type_str| !type_str.is_empty())
      .collect()
  } else {
    vec![]
  };

  let (root_names, module_graph) = create_runtime_module_graph(
    &program_state,
    permissions.clone(),
    root_name,
    sources,
    type_files,
  )
  .await?;
  let module_graph_json =
    serde_json::to_value(module_graph).expect("Failed to serialize data");

  let unstable = program_state.flags.unstable;

  let mut lib = vec![];
  if let Some(user_libs) = user_options["lib"].take().as_array() {
    let libs = user_libs
      .iter()
      .map(|type_value| type_value.as_str().unwrap_or("").to_string())
      .filter(|type_str| !type_str.is_empty())
      .collect::<Vec<String>>();
    lib.extend(libs);
  } else {
    lib.push("deno.window".to_string());
  }

  if unstable {
    lib.push("deno.unstable".to_string());
  }

  let mut compiler_options = json!({
    "allowJs": false,
    "allowNonTsExtensions": true,
    "checkJs": false,
    "esModuleInterop": true,
    "jsx": "react",
    "module": "esnext",
    "outDir": null,
    "sourceMap": true,
    "strict": true,
    "removeComments": true,
    "target": "esnext",
  });

  let bundler_options = json!({
    "allowJs": true,
    "inlineSourceMap": false,
    "module": "system",
    "outDir": null,
    "outFile": "deno:///bundle.js",
    // disabled until we have effective way to modify source maps
    "sourceMap": false,
  });

  tsc_config::json_merge(&mut compiler_options, &user_options);
  tsc_config::json_merge(&mut compiler_options, &json!({ "lib": lib }));
  tsc_config::json_merge(&mut compiler_options, &bundler_options);

  let req_msg = json!({
    "type": CompilerRequestType::RuntimeBundle,
    "target": "runtime",
    "rootNames": root_names,
    "sourceFileMap": module_graph_json,
    "compilerOptions": compiler_options,
  })
  .to_string();

  let json_str =
    execute_in_tsc(program_state.clone(), req_msg).map_err(extract_js_error)?;
  let _response: RuntimeBundleResponse = serde_json::from_str(&json_str)?;
  // We're returning `Ok()` instead of `Err()` because it's not runtime
  // error if there were diagnostics produced; we want to let user handle
  // diagnostics in the runtime.
  Ok(serde_json::from_str::<Value>(&json_str).unwrap())
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImportDesc {
  pub specifier: String,
  pub deno_types: Option<String>,
  pub location: Location,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TsReferenceKind {
  Lib,
  Types,
  Path,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TsReferenceDesc {
  pub kind: TsReferenceKind,
  pub specifier: String,
  pub location: Location,
}

// TODO(bartlomieju): handle imports in ambient contexts/TS modules
/// This function is a port of `ts.preProcessFile()`
///
/// Additionally it captures `@deno-types` references directly
/// preceeding `import .. from` and `export .. from` statements.
pub fn pre_process_file(
  file_name: &str,
  media_type: MediaType,
  source_code: &str,
  analyze_dynamic_imports: bool,
) -> Result<(Vec<ImportDesc>, Vec<TsReferenceDesc>), AnyError> {
  let specifier = ModuleSpecifier::resolve_url_or_path(file_name)?;
  let module = parse(specifier.as_str(), source_code, &media_type)?;

  let dependency_descriptors = module.analyze_dependencies();

  // for each import check if there's relevant @deno-types directive
  let imports = dependency_descriptors
    .iter()
    .filter(|desc| desc.kind != dep_graph::DependencyKind::Require)
    .filter(|desc| {
      if analyze_dynamic_imports {
        return true;
      }
      !desc.is_dynamic
    })
    .map(|desc| {
      let deno_types = get_deno_types(&desc.leading_comments);
      ImportDesc {
        specifier: desc.specifier.to_string(),
        deno_types,
        location: Location {
          filename: file_name.to_string(),
          col: desc.col,
          line: desc.line,
        },
      }
    })
    .collect();

  // analyze comment from beginning of the file and find TS directives
  let comments = module.get_leading_comments();

  let mut references = vec![];
  for comment in comments {
    if comment.kind != CommentKind::Line {
      continue;
    }

    let text = comment.text.to_string();
    if let Some((kind, specifier)) = parse_ts_reference(text.trim()) {
      let location = module.get_location(&comment.span);
      references.push(TsReferenceDesc {
        kind,
        specifier,
        location,
      });
    }
  }
  Ok((imports, references))
}

fn get_deno_types(comments: &[Comment]) -> Option<String> {
  if comments.is_empty() {
    return None;
  }

  // @deno-types must directly prepend import statement - hence
  // checking last comment for span
  let last = comments.last().unwrap();
  let comment = last.text.trim_start();
  parse_deno_types(&comment)
}

fn parse_ts_reference(comment: &str) -> Option<(TsReferenceKind, String)> {
  if !TRIPLE_SLASH_REFERENCE_RE.is_match(comment) {
    return None;
  }

  let (kind, specifier) =
    if let Some(capture_groups) = PATH_REFERENCE_RE.captures(comment) {
      (TsReferenceKind::Path, capture_groups.get(1).unwrap())
    } else if let Some(capture_groups) = TYPES_REFERENCE_RE.captures(comment) {
      (TsReferenceKind::Types, capture_groups.get(1).unwrap())
    } else if let Some(capture_groups) = LIB_REFERENCE_RE.captures(comment) {
      (TsReferenceKind::Lib, capture_groups.get(1).unwrap())
    } else {
      return None;
    };

  Some((kind, specifier.as_str().to_string()))
}

fn parse_deno_types(comment: &str) -> Option<String> {
  if let Some(capture_groups) = DENO_TYPES_RE.captures(comment) {
    if let Some(specifier) = capture_groups.get(1) {
      return Some(specifier.as_str().to_string());
    }
    if let Some(specifier) = capture_groups.get(2) {
      return Some(specifier.as_str().to_string());
    }
  }

  None
}

// Warning! The values in this enum are duplicated in js/compiler.ts
// Update carefully!
#[repr(i32)]
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum CompilerRequestType {
  RuntimeCompile = 2,
  RuntimeBundle = 3,
}

impl Serialize for CompilerRequestType {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    let value: i32 = match self {
      CompilerRequestType::RuntimeCompile => 2 as i32,
      CompilerRequestType::RuntimeBundle => 3 as i32,
    };
    Serialize::serialize(&value, serializer)
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::fs as deno_fs;
  use tempfile::TempDir;

  #[test]
  fn test_parse_deno_types() {
    assert_eq!(
      parse_deno_types("@deno-types=./a/b/c.d.ts"),
      Some("./a/b/c.d.ts".to_string())
    );
    assert_eq!(
      parse_deno_types("@deno-types=\"./a/b/c.d.ts\""),
      Some("./a/b/c.d.ts".to_string())
    );
    assert_eq!(
      parse_deno_types("@deno-types = https://dneo.land/x/some/package/a.d.ts"),
      Some("https://dneo.land/x/some/package/a.d.ts".to_string())
    );
    assert_eq!(
      parse_deno_types("@deno-types = ./a/b/c.d.ts"),
      Some("./a/b/c.d.ts".to_string())
    );
    assert!(parse_deno_types("asdf").is_none());
    assert!(parse_deno_types("// deno-types = fooo").is_none());
    assert_eq!(
      parse_deno_types("@deno-types=./a/b/c.d.ts some comment"),
      Some("./a/b/c.d.ts".to_string())
    );
    assert_eq!(
      parse_deno_types(
        "@deno-types=./a/b/c.d.ts // some comment after slashes"
      ),
      Some("./a/b/c.d.ts".to_string())
    );
    assert_eq!(
      parse_deno_types(r#"@deno-types="https://deno.land/x/foo/index.d.ts";"#),
      Some("https://deno.land/x/foo/index.d.ts".to_string())
    );
  }

  #[test]
  fn test_parse_ts_reference() {
    assert_eq!(
      parse_ts_reference(r#"/ <reference lib="deno.shared_globals" />"#),
      Some((TsReferenceKind::Lib, "deno.shared_globals".to_string()))
    );
    assert_eq!(
      parse_ts_reference(r#"/ <reference path="./type/reference/dep.ts" />"#),
      Some((TsReferenceKind::Path, "./type/reference/dep.ts".to_string()))
    );
    assert_eq!(
      parse_ts_reference(r#"/ <reference types="./type/reference.d.ts" />"#),
      Some((TsReferenceKind::Types, "./type/reference.d.ts".to_string()))
    );
    assert!(parse_ts_reference("asdf").is_none());
    assert!(
      parse_ts_reference(r#"/ <reference unknown="unknown" />"#).is_none()
    );
    assert!(parse_ts_reference(r#"/ <asset path="./styles.css" />"#).is_none());
  }

  #[test]
  fn test_source_code_version_hash() {
    assert_eq!(
      "0185b42de0686b4c93c314daaa8dee159f768a9e9a336c2a5e3d5b8ca6c4208c",
      source_code_version_hash(b"1+2", "0.4.0", b"{}")
    );
    // Different source_code should result in different hash.
    assert_eq!(
      "e58631f1b6b6ce2b300b133ec2ad16a8a5ba6b7ecf812a8c06e59056638571ac",
      source_code_version_hash(b"1", "0.4.0", b"{}")
    );
    // Different version should result in different hash.
    assert_eq!(
      "307e6200347a88dbbada453102deb91c12939c65494e987d2d8978f6609b5633",
      source_code_version_hash(b"1", "0.1.0", b"{}")
    );
    // Different config should result in different hash.
    assert_eq!(
      "195eaf104a591d1d7f69fc169c60a41959c2b7a21373cd23a8f675f877ec385f",
      source_code_version_hash(b"1", "0.4.0", b"{\"compilerOptions\": {}}")
    );
  }

  #[test]
  fn test_compile_js() {
    let temp_dir = TempDir::new().expect("tempdir fail");
    let temp_dir_path = temp_dir.path();

    let test_cases = vec![
      // valid JSON
      (r#"{ "compilerOptions": { "checkJs": true } } "#, true),
      // JSON with comment
      (
        r#"{
          "compilerOptions": {
            // force .js file compilation by Deno
            "checkJs": true
          }
        }"#,
        true,
      ),
      // without content
      ("", false),
    ];

    let path = temp_dir_path.join("tsconfig.json");
    let path_str = path.to_str().unwrap().to_string();

    for (json_str, expected) in test_cases {
      deno_fs::write_file(&path, json_str.as_bytes(), 0o666).unwrap();
      let config = CompilerConfig::load(Some(path_str.clone())).unwrap();
      assert_eq!(config.compile_js, expected);
    }
  }

  #[test]
  fn test_compiler_config_load() {
    let temp_dir = TempDir::new().expect("tempdir fail");
    let temp_dir_path = temp_dir.path();
    let path = temp_dir_path.join("doesnotexist.json");
    let path_str = path.to_str().unwrap().to_string();
    let res = CompilerConfig::load(Some(path_str));
    assert!(res.is_err());
  }
}
