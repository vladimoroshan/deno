// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use crate::ast;
use crate::ast::parse;
use crate::ast::Location;
use crate::ast::ParsedModule;
use crate::import_map::ImportMap;
use crate::info::ModuleGraphInfo;
use crate::info::ModuleInfo;
use crate::info::ModuleInfoMap;
use crate::info::ModuleInfoMapItem;
use crate::lockfile::Lockfile;
use crate::media_type::MediaType;
use crate::specifier_handler::CachedModule;
use crate::specifier_handler::DependencyMap;
use crate::specifier_handler::Emit;
use crate::specifier_handler::FetchFuture;
use crate::specifier_handler::SpecifierHandler;
use crate::tsc_config::IgnoredCompilerOptions;
use crate::tsc_config::TsConfig;
use crate::version;
use crate::AnyError;

use deno_core::futures::stream::FuturesUnordered;
use deno_core::futures::stream::StreamExt;
use deno_core::serde_json::json;
use deno_core::ModuleSpecifier;
use regex::Regex;
use serde::Deserialize;
use serde::Deserializer;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::error::Error;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::result;
use std::sync::Mutex;
use std::time::Instant;
use swc_ecmascript::dep_graph::DependencyKind;

lazy_static! {
  /// Matched the `@deno-types` pragma.
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
}

/// A group of errors that represent errors that can occur when interacting with
/// a module graph.
#[allow(unused)]
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum GraphError {
  /// A module using the HTTPS protocol is trying to import a module with an
  /// HTTP schema.
  InvalidDowngrade(ModuleSpecifier, Location),
  /// A remote module is trying to import a local module.
  InvalidLocalImport(ModuleSpecifier, Location),
  /// A remote module is trying to import a local module.
  InvalidSource(ModuleSpecifier, String),
  /// A module specifier could not be resolved for a given import.
  InvalidSpecifier(String, Location),
  /// An unexpected dependency was requested for a module.
  MissingDependency(ModuleSpecifier, String),
  /// An unexpected specifier was requested.
  MissingSpecifier(ModuleSpecifier),
  /// Snapshot data was not present in a situation where it was required.
  MissingSnapshotData,
  /// The current feature is not supported.
  NotSupported(String),
}
use GraphError::*;

impl fmt::Display for GraphError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    match self {
      InvalidDowngrade(ref specifier, ref location) => write!(f, "Modules imported via https are not allowed to import http modules.\n  Importing: {}\n    at {}:{}:{}", specifier, location.filename, location.line, location.col),
      InvalidLocalImport(ref specifier, ref location) => write!(f, "Remote modules are not allowed to import local modules.\n  Importing: {}\n    at {}:{}:{}", specifier, location.filename, location.line, location.col),
      InvalidSource(ref specifier, ref lockfile) => write!(f, "The source code is invalid, as it does not match the expected hash in the lock file.\n  Specifier: {}\n  Lock file: {}", specifier, lockfile),
      InvalidSpecifier(ref specifier, ref location) => write!(f, "Unable to resolve dependency specifier.\n  Specifier: {}\n    at {}:{}:{}", specifier, location.filename, location.line, location.col),
      MissingDependency(ref referrer, specifier) => write!(
        f,
        "The graph is missing a dependency.\n  Specifier: {} from {}",
        specifier, referrer
      ),
      MissingSpecifier(ref specifier) => write!(
        f,
        "The graph is missing a specifier.\n  Specifier: {}",
        specifier
      ),
      MissingSnapshotData => write!(f, "Snapshot data was not supplied, but required."),
      NotSupported(ref msg) => write!(f, "{}", msg),
    }
  }
}

impl Error for GraphError {}

/// An enum which represents the parsed out values of references in source code.
#[derive(Debug, Clone, Eq, PartialEq)]
enum TypeScriptReference {
  Path(String),
  Types(String),
}

/// Determine if a comment contains a triple slash reference and optionally
/// return its kind and value.
fn parse_ts_reference(comment: &str) -> Option<TypeScriptReference> {
  if !TRIPLE_SLASH_REFERENCE_RE.is_match(comment) {
    None
  } else if let Some(captures) = PATH_REFERENCE_RE.captures(comment) {
    Some(TypeScriptReference::Path(
      captures.get(1).unwrap().as_str().to_string(),
    ))
  } else if let Some(captures) = TYPES_REFERENCE_RE.captures(comment) {
    Some(TypeScriptReference::Types(
      captures.get(1).unwrap().as_str().to_string(),
    ))
  } else {
    None
  }
}

/// Determine if a comment contains a `@deno-types` pragma and optionally return
/// its value.
fn parse_deno_types(comment: &str) -> Option<String> {
  if let Some(captures) = DENO_TYPES_RE.captures(comment) {
    if let Some(m) = captures.get(1) {
      Some(m.as_str().to_string())
    } else if let Some(m) = captures.get(2) {
      Some(m.as_str().to_string())
    } else {
      panic!("unreachable");
    }
  } else {
    None
  }
}

/// A hashing function that takes the source code, version and optionally a
/// user provided config and generates a string hash which can be stored to
/// determine if the cached emit is valid or not.
fn get_version(source: &str, version: &str, config: &[u8]) -> String {
  crate::checksum::gen(&[source.as_bytes(), version.as_bytes(), config])
}

/// A logical representation of a module within a graph.
#[derive(Debug, Clone)]
struct Module {
  dependencies: DependencyMap,
  is_dirty: bool,
  is_parsed: bool,
  maybe_emit: Option<Emit>,
  maybe_emit_path: Option<(PathBuf, Option<PathBuf>)>,
  maybe_import_map: Option<Rc<RefCell<ImportMap>>>,
  maybe_parsed_module: Option<ParsedModule>,
  maybe_types: Option<(String, ModuleSpecifier)>,
  maybe_version: Option<String>,
  media_type: MediaType,
  specifier: ModuleSpecifier,
  source: String,
  source_path: PathBuf,
}

impl Default for Module {
  fn default() -> Self {
    Module {
      dependencies: HashMap::new(),
      is_dirty: false,
      is_parsed: false,
      maybe_emit: None,
      maybe_emit_path: None,
      maybe_import_map: None,
      maybe_parsed_module: None,
      maybe_types: None,
      maybe_version: None,
      media_type: MediaType::Unknown,
      specifier: ModuleSpecifier::resolve_url("file:///example.js").unwrap(),
      source: "".to_string(),
      source_path: PathBuf::new(),
    }
  }
}

impl Module {
  pub fn new(
    cached_module: CachedModule,
    maybe_import_map: Option<Rc<RefCell<ImportMap>>>,
  ) -> Self {
    let mut module = Module {
      specifier: cached_module.specifier,
      maybe_import_map,
      media_type: cached_module.media_type,
      source: cached_module.source,
      source_path: cached_module.source_path,
      maybe_emit: cached_module.maybe_emit,
      maybe_emit_path: cached_module.maybe_emit_path,
      maybe_version: cached_module.maybe_version,
      is_dirty: false,
      ..Self::default()
    };
    if module.maybe_import_map.is_none() {
      if let Some(dependencies) = cached_module.maybe_dependencies {
        module.dependencies = dependencies;
        module.is_parsed = true;
      }
    }
    module.maybe_types = if let Some(ref specifier) = cached_module.maybe_types
    {
      Some((
        specifier.clone(),
        module
          .resolve_import(&specifier, None)
          .expect("could not resolve module"),
      ))
    } else {
      None
    };
    module
  }

  /// Return `true` if the current hash of the module matches the stored
  /// version.
  pub fn is_emit_valid(&self, config: &[u8]) -> bool {
    if let Some(version) = self.maybe_version.clone() {
      version == get_version(&self.source, version::DENO, config)
    } else {
      false
    }
  }

  pub fn parse(&mut self) -> Result<(), AnyError> {
    let parsed_module = parse(&self.specifier, &self.source, &self.media_type)?;

    // parse out any triple slash references
    for comment in parsed_module.get_leading_comments().iter() {
      if let Some(ts_reference) = parse_ts_reference(&comment.text) {
        let location: Location = parsed_module.get_location(&comment.span);
        match ts_reference {
          TypeScriptReference::Path(import) => {
            let specifier = self.resolve_import(&import, Some(location))?;
            let dep = self.dependencies.entry(import).or_default();
            dep.maybe_code = Some(specifier);
          }
          TypeScriptReference::Types(import) => {
            let specifier = self.resolve_import(&import, Some(location))?;
            if self.media_type == MediaType::JavaScript
              || self.media_type == MediaType::JSX
            {
              // TODO(kitsonk) we need to specifically update the cache when
              // this value changes
              self.maybe_types = Some((import.clone(), specifier));
            } else {
              let dep = self.dependencies.entry(import).or_default();
              dep.maybe_type = Some(specifier);
            }
          }
        }
      }
    }

    // Parse out all the syntactical dependencies for a module
    let dependencies = parsed_module.analyze_dependencies();
    for desc in dependencies
      .iter()
      .filter(|desc| desc.kind != DependencyKind::Require)
    {
      let location = Location {
        filename: self.specifier.to_string(),
        col: desc.col,
        line: desc.line,
      };
      let specifier =
        self.resolve_import(&desc.specifier, Some(location.clone()))?;

      // Parse out any `@deno-types` pragmas and modify dependency
      let maybe_types_specifier = if !desc.leading_comments.is_empty() {
        let comment = desc.leading_comments.last().unwrap();
        if let Some(deno_types) = parse_deno_types(&comment.text).as_ref() {
          Some(self.resolve_import(deno_types, Some(location))?)
        } else {
          None
        }
      } else {
        None
      };

      let dep = self
        .dependencies
        .entry(desc.specifier.to_string())
        .or_default();
      if desc.kind == DependencyKind::ExportType
        || desc.kind == DependencyKind::ImportType
      {
        dep.maybe_type = Some(specifier);
      } else {
        dep.maybe_code = Some(specifier);
      }
      if let Some(types_specifier) = maybe_types_specifier {
        dep.maybe_type = Some(types_specifier);
      }
    }

    self.maybe_parsed_module = Some(parsed_module);
    Ok(())
  }

  fn resolve_import(
    &self,
    specifier: &str,
    maybe_location: Option<Location>,
  ) -> Result<ModuleSpecifier, AnyError> {
    let maybe_resolve = if let Some(import_map) = self.maybe_import_map.clone()
    {
      import_map
        .borrow()
        .resolve(specifier, self.specifier.as_str())?
    } else {
      None
    };
    let specifier = if let Some(module_specifier) = maybe_resolve {
      module_specifier
    } else {
      ModuleSpecifier::resolve_import(specifier, self.specifier.as_str())?
    };

    let referrer_scheme = self.specifier.as_url().scheme();
    let specifier_scheme = specifier.as_url().scheme();
    let location = maybe_location.unwrap_or(Location {
      filename: self.specifier.to_string(),
      line: 0,
      col: 0,
    });

    // Disallow downgrades from HTTPS to HTTP
    if referrer_scheme == "https" && specifier_scheme == "http" {
      return Err(InvalidDowngrade(specifier.clone(), location).into());
    }

    // Disallow a remote URL from trying to import a local URL
    if (referrer_scheme == "https" || referrer_scheme == "http")
      && !(specifier_scheme == "https" || specifier_scheme == "http")
    {
      return Err(InvalidLocalImport(specifier.clone(), location).into());
    }

    Ok(specifier)
  }

  /// Calculate the hashed version of the module and update the `maybe_version`.
  pub fn set_version(&mut self, config: &[u8]) {
    self.maybe_version = Some(get_version(&self.source, version::DENO, config))
  }

  pub fn size(&self) -> usize {
    self.source.as_bytes().len()
  }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Stats(pub Vec<(String, u128)>);

impl<'de> Deserialize<'de> for Stats {
  fn deserialize<D>(deserializer: D) -> result::Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let items: Vec<(String, u128)> = Deserialize::deserialize(deserializer)?;
    Ok(Stats(items))
  }
}

impl fmt::Display for Stats {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    for (key, value) in self.0.clone() {
      write!(f, "{}: {}", key, value)?;
    }

    Ok(())
  }
}

/// A structure which provides options when transpiling modules.
#[derive(Debug, Default)]
pub struct TranspileOptions {
  /// If `true` then debug logging will be output from the isolate.
  pub debug: bool,
  /// An optional string that points to a user supplied TypeScript configuration
  /// file that augments the the default configuration passed to the TypeScript
  /// compiler.
  pub maybe_config_path: Option<String>,
}

/// A dependency graph of modules, were the modules that have been inserted via
/// the builder will be loaded into the graph.  Also provides an interface to
/// be able to manipulate and handle the graph.
#[derive(Debug)]
pub struct Graph2 {
  handler: Rc<RefCell<dyn SpecifierHandler>>,
  maybe_ts_build_info: Option<String>,
  modules: HashMap<ModuleSpecifier, Module>,
  redirects: HashMap<ModuleSpecifier, ModuleSpecifier>,
  roots: Vec<ModuleSpecifier>,
}

impl Graph2 {
  /// Create a new instance of a graph, ready to have modules loaded it.
  ///
  /// The argument `handler` is an instance of a structure that implements the
  /// `SpecifierHandler` trait.
  ///
  pub fn new(handler: Rc<RefCell<dyn SpecifierHandler>>) -> Self {
    Graph2 {
      handler,
      maybe_ts_build_info: None,
      modules: HashMap::new(),
      redirects: HashMap::new(),
      roots: Vec::new(),
    }
  }

  fn contains_module(&self, specifier: &ModuleSpecifier) -> bool {
    let s = self.resolve_specifier(specifier);
    self.modules.contains_key(s)
  }

  /// Update the handler with any modules that are marked as _dirty_ and update
  /// any build info if present.
  fn flush(&mut self) -> Result<(), AnyError> {
    let mut handler = self.handler.borrow_mut();
    for (_, module) in self.modules.iter_mut() {
      if module.is_dirty {
        if let Some(emit) = &module.maybe_emit {
          handler.set_cache(&module.specifier, emit)?;
        }
        if let Some(version) = &module.maybe_version {
          handler.set_version(&module.specifier, version.clone())?;
        }
        module.is_dirty = false;
      }
    }
    for root_specifier in self.roots.iter() {
      if let Some(ts_build_info) = &self.maybe_ts_build_info {
        handler.set_ts_build_info(root_specifier, ts_build_info.to_owned())?;
      }
    }

    Ok(())
  }

  fn get_info(
    &self,
    specifier: &ModuleSpecifier,
    seen: &mut HashSet<ModuleSpecifier>,
    totals: &mut HashMap<ModuleSpecifier, usize>,
  ) -> ModuleInfo {
    let not_seen = seen.insert(specifier.clone());
    let module = self.get_module(specifier).unwrap();
    let mut deps = Vec::new();
    let mut total_size = None;

    if not_seen {
      let mut seen_deps = HashSet::new();
      // TODO(@kitsonk) https://github.com/denoland/deno/issues/7927
      for (_, dep) in module.dependencies.iter() {
        // Check the runtime code dependency
        if let Some(code_dep) = &dep.maybe_code {
          if seen_deps.insert(code_dep.clone()) {
            deps.push(self.get_info(code_dep, seen, totals));
          }
        }
      }
      deps.sort();
      total_size = if let Some(total) = totals.get(specifier) {
        Some(total.to_owned())
      } else {
        let mut total = deps
          .iter()
          .map(|d| {
            if let Some(total_size) = d.total_size {
              total_size
            } else {
              0
            }
          })
          .sum();
        total += module.size();
        totals.insert(specifier.clone(), total);
        Some(total)
      };
    }

    ModuleInfo {
      deps,
      name: specifier.clone(),
      size: module.size(),
      total_size,
    }
  }

  fn get_info_map(&self) -> ModuleInfoMap {
    let map = self
      .modules
      .iter()
      .map(|(specifier, module)| {
        let mut deps = HashSet::new();
        for (_, dep) in module.dependencies.iter() {
          if let Some(code_dep) = &dep.maybe_code {
            deps.insert(code_dep.clone());
          }
          if let Some(type_dep) = &dep.maybe_type {
            deps.insert(type_dep.clone());
          }
        }
        if let Some((_, types_dep)) = &module.maybe_types {
          deps.insert(types_dep.clone());
        }
        let item = ModuleInfoMapItem {
          deps: deps.into_iter().collect(),
          size: module.size(),
        };
        (specifier.clone(), item)
      })
      .collect();

    ModuleInfoMap::new(map)
  }

  pub fn get_media_type(
    &self,
    specifier: &ModuleSpecifier,
  ) -> Option<MediaType> {
    if let Some(module) = self.get_module(specifier) {
      Some(module.media_type)
    } else {
      None
    }
  }

  fn get_module(&self, specifier: &ModuleSpecifier) -> Option<&Module> {
    let s = self.resolve_specifier(specifier);
    self.modules.get(s)
  }

  /// Get the source for a given module specifier.  If the module is not part
  /// of the graph, the result will be `None`.
  pub fn get_source(&self, specifier: &ModuleSpecifier) -> Option<String> {
    if let Some(module) = self.get_module(specifier) {
      Some(module.source.clone())
    } else {
      None
    }
  }

  /// Return a structure which provides information about the module graph and
  /// the relationship of the modules in the graph.  This structure is used to
  /// provide information for the `info` subcommand.
  pub fn info(&self) -> Result<ModuleGraphInfo, AnyError> {
    if self.roots.is_empty() || self.roots.len() > 1 {
      return Err(NotSupported(format!("Info is only supported when there is a single root module in the graph.  Found: {}", self.roots.len())).into());
    }

    let module = self.roots[0].clone();
    let m = self.get_module(&module).unwrap();

    let mut seen = HashSet::new();
    let mut totals = HashMap::new();
    let info = self.get_info(&module, &mut seen, &mut totals);

    let files = self.get_info_map();
    let total_size = totals.get(&module).unwrap_or(&m.size()).to_owned();
    let (compiled, map) =
      if let Some((emit_path, maybe_map_path)) = &m.maybe_emit_path {
        (Some(emit_path.clone()), maybe_map_path.clone())
      } else {
        (None, None)
      };

    Ok(ModuleGraphInfo {
      compiled,
      dep_count: self.modules.len() - 1,
      file_type: m.media_type,
      files,
      info,
      local: m.source_path.clone(),
      map,
      module,
      total_size,
    })
  }

  /// Verify the subresource integrity of the graph based upon the optional
  /// lockfile, updating the lockfile with any missing resources.  This will
  /// error if any of the resources do not match their lock status.
  pub fn lock(
    &self,
    maybe_lockfile: &Option<Mutex<Lockfile>>,
  ) -> Result<(), AnyError> {
    if let Some(lf) = maybe_lockfile {
      let mut lockfile = lf.lock().unwrap();
      for (ms, module) in self.modules.iter() {
        let specifier = module.specifier.to_string();
        let valid = lockfile.check_or_insert(&specifier, &module.source);
        if !valid {
          return Err(
            InvalidSource(ms.clone(), lockfile.filename.clone()).into(),
          );
        }
      }
    }

    Ok(())
  }

  /// Given a string specifier and a referring module specifier, provide the
  /// resulting module specifier and media type for the module that is part of
  /// the graph.
  pub fn resolve(
    &self,
    specifier: &str,
    referrer: &ModuleSpecifier,
  ) -> Result<ModuleSpecifier, AnyError> {
    if !self.contains_module(referrer) {
      return Err(MissingSpecifier(referrer.to_owned()).into());
    }
    let module = self.get_module(referrer).unwrap();
    if !module.dependencies.contains_key(specifier) {
      return Err(
        MissingDependency(referrer.to_owned(), specifier.to_owned()).into(),
      );
    }
    let dependency = module.dependencies.get(specifier).unwrap();
    // If there is a @deno-types pragma that impacts the dependency, then the
    // maybe_type property will be set with that specifier, otherwise we use the
    // specifier that point to the runtime code.
    let resolved_specifier =
      if let Some(type_specifier) = dependency.maybe_type.clone() {
        type_specifier
      } else if let Some(code_specifier) = dependency.maybe_code.clone() {
        code_specifier
      } else {
        return Err(
          MissingDependency(referrer.to_owned(), specifier.to_owned()).into(),
        );
      };
    if !self.contains_module(&resolved_specifier) {
      return Err(
        MissingDependency(referrer.to_owned(), resolved_specifier.to_string())
          .into(),
      );
    }
    let dep_module = self.get_module(&resolved_specifier).unwrap();
    // In the case that there is a X-TypeScript-Types or a triple-slash types,
    // then the `maybe_types` specifier will be populated and we should use that
    // instead.
    let result = if let Some((_, types)) = dep_module.maybe_types.clone() {
      types
    } else {
      resolved_specifier
    };

    Ok(result)
  }

  /// Takes a module specifier and returns the "final" specifier, accounting for
  /// any redirects that may have occurred.
  fn resolve_specifier<'a>(
    &'a self,
    specifier: &'a ModuleSpecifier,
  ) -> &'a ModuleSpecifier {
    let mut s = specifier;
    let mut seen = HashSet::new();
    seen.insert(s.clone());
    while let Some(redirect) = self.redirects.get(s) {
      if !seen.insert(redirect.clone()) {
        eprintln!("An infinite loop of module redirections detected.\n  Original specifier: {}", specifier);
        break;
      }
      s = redirect;
      if seen.len() > 5 {
        eprintln!("An excessive number of module redirections detected.\n  Original specifier: {}", specifier);
        break;
      }
    }
    s
  }

  /// Transpile (only transform) the graph, updating any emitted modules
  /// with the specifier handler.  The result contains any performance stats
  /// from the compiler and optionally any user provided configuration compiler
  /// options that were ignored.
  ///
  /// # Arguments
  ///
  /// - `options` - A structure of options which impact how the code is
  ///   transpiled.
  ///
  pub fn transpile(
    &mut self,
    options: TranspileOptions,
  ) -> Result<(Stats, Option<IgnoredCompilerOptions>), AnyError> {
    let start = Instant::now();

    let mut ts_config = TsConfig::new(json!({
      "checkJs": false,
      "emitDecoratorMetadata": false,
      "jsx": "react",
      "jsxFactory": "React.createElement",
      "jsxFragmentFactory": "React.Fragment",
    }));

    let maybe_ignored_options =
      ts_config.merge_user_config(options.maybe_config_path)?;

    let compiler_options = ts_config.as_transpile_config()?;
    let check_js = compiler_options.check_js;
    let transform_jsx = compiler_options.jsx == "react";
    let emit_options = ast::TranspileOptions {
      emit_metadata: compiler_options.emit_decorator_metadata,
      inline_source_map: true,
      jsx_factory: compiler_options.jsx_factory,
      jsx_fragment_factory: compiler_options.jsx_fragment_factory,
      transform_jsx,
    };

    let mut emit_count: u128 = 0;
    for (_, module) in self.modules.iter_mut() {
      // TODO(kitsonk) a lot of this logic should be refactored into `Module` as
      // we start to support other methods on the graph.  Especially managing
      // the dirty state is something the module itself should "own".

      // if the module is a Dts file we should skip it
      if module.media_type == MediaType::Dts {
        continue;
      }
      // if we don't have check_js enabled, we won't touch non TypeScript
      // modules
      if !(check_js
        || module.media_type == MediaType::TSX
        || module.media_type == MediaType::TypeScript)
      {
        continue;
      }
      let config = ts_config.as_bytes();
      // skip modules that already have a valid emit
      if module.maybe_emit.is_some() && module.is_emit_valid(&config) {
        continue;
      }
      if module.maybe_parsed_module.is_none() {
        module.parse()?;
      }
      let parsed_module = module.maybe_parsed_module.clone().unwrap();
      let emit = parsed_module.transpile(&emit_options)?;
      emit_count += 1;
      module.maybe_emit = Some(Emit::Cli(emit));
      module.set_version(&config);
      module.is_dirty = true;
    }
    self.flush()?;

    let stats = Stats(vec![
      ("Files".to_string(), self.modules.len() as u128),
      ("Emitted".to_string(), emit_count),
      ("Total time".to_string(), start.elapsed().as_millis()),
    ]);

    Ok((stats, maybe_ignored_options))
  }
}

/// A structure for building a dependency graph of modules.
pub struct GraphBuilder2 {
  fetched: HashSet<ModuleSpecifier>,
  graph: Graph2,
  maybe_import_map: Option<Rc<RefCell<ImportMap>>>,
  pending: FuturesUnordered<FetchFuture>,
}

impl GraphBuilder2 {
  pub fn new(
    handler: Rc<RefCell<dyn SpecifierHandler>>,
    maybe_import_map: Option<ImportMap>,
  ) -> Self {
    let internal_import_map = if let Some(import_map) = maybe_import_map {
      Some(Rc::new(RefCell::new(import_map)))
    } else {
      None
    };
    GraphBuilder2 {
      graph: Graph2::new(handler),
      fetched: HashSet::new(),
      maybe_import_map: internal_import_map,
      pending: FuturesUnordered::new(),
    }
  }

  /// Request a module to be fetched from the handler and queue up its future
  /// to be awaited to be resolved.
  fn fetch(&mut self, specifier: &ModuleSpecifier) -> Result<(), AnyError> {
    if self.fetched.contains(&specifier) {
      return Ok(());
    }

    self.fetched.insert(specifier.clone());
    let future = self.graph.handler.borrow_mut().fetch(specifier.clone());
    self.pending.push(future);

    Ok(())
  }

  /// Visit a module that has been fetched, hydrating the module, analyzing its
  /// dependencies if required, fetching those dependencies, and inserting the
  /// module into the graph.
  fn visit(&mut self, cached_module: CachedModule) -> Result<(), AnyError> {
    let specifier = cached_module.specifier.clone();
    let requested_specifier = cached_module.requested_specifier.clone();
    let mut module = Module::new(cached_module, self.maybe_import_map.clone());
    if !module.is_parsed {
      let has_types = module.maybe_types.is_some();
      module.parse()?;
      if self.maybe_import_map.is_none() {
        let mut handler = self.graph.handler.borrow_mut();
        handler.set_deps(&specifier, module.dependencies.clone())?;
        if !has_types {
          if let Some((types, _)) = module.maybe_types.clone() {
            handler.set_types(&specifier, types)?;
          }
        }
      }
    }
    for (_, dep) in module.dependencies.iter() {
      if let Some(specifier) = dep.maybe_code.as_ref() {
        self.fetch(specifier)?;
      }
      if let Some(specifier) = dep.maybe_type.as_ref() {
        self.fetch(specifier)?;
      }
    }
    if let Some((_, specifier)) = module.maybe_types.as_ref() {
      self.fetch(specifier)?;
    }
    if specifier != requested_specifier {
      self
        .graph
        .redirects
        .insert(requested_specifier, specifier.clone());
    }
    self.graph.modules.insert(specifier, module);

    Ok(())
  }

  /// Insert a module into the graph based on a module specifier.  The module
  /// and any dependencies will be fetched from the handler.  The module will
  /// also be treated as a _root_ module in the graph.
  pub async fn insert(
    &mut self,
    specifier: &ModuleSpecifier,
  ) -> Result<(), AnyError> {
    self.fetch(specifier)?;

    loop {
      let cached_module = self.pending.next().await.unwrap()?;
      self.visit(cached_module)?;
      if self.pending.is_empty() {
        break;
      }
    }

    if !self.graph.roots.contains(specifier) {
      self.graph.roots.push(specifier.clone());
    }

    Ok(())
  }

  /// Move out the graph from the builder to be utilized further.  An optional
  /// lockfile can be provided, where if the sources in the graph do not match
  /// the expected lockfile, the method with error instead of returning the
  /// graph.
  ///
  /// TODO(@kitsonk) this should really be owned by the graph, but currently
  /// the lockfile is behind a mutex in program_state, which makes it really
  /// hard to not pass around as a reference, which if the Graph owned it, it
  /// would need lifetime parameters and lifetime parameters are 😭
  pub fn get_graph(
    self,
    maybe_lockfile: &Option<Mutex<Lockfile>>,
  ) -> Result<Graph2, AnyError> {
    self.graph.lock(maybe_lockfile)?;
    Ok(self.graph)
  }
}

#[cfg(test)]
pub mod tests {
  use super::*;

  use deno_core::futures::future;
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::sync::Mutex;

  /// This is a testing mock for `SpecifierHandler` that uses a special file
  /// system renaming to mock local and remote modules as well as provides
  /// "spies" for the critical methods for testing purposes.
  #[derive(Debug, Default)]
  pub struct MockSpecifierHandler {
    pub fixtures: PathBuf,
    pub maybe_ts_build_info: Option<String>,
    pub ts_build_info_calls: Vec<(ModuleSpecifier, String)>,
    pub cache_calls: Vec<(ModuleSpecifier, Emit)>,
    pub deps_calls: Vec<(ModuleSpecifier, DependencyMap)>,
    pub types_calls: Vec<(ModuleSpecifier, String)>,
    pub version_calls: Vec<(ModuleSpecifier, String)>,
  }

  impl MockSpecifierHandler {
    fn get_cache(
      &self,
      specifier: ModuleSpecifier,
    ) -> Result<CachedModule, AnyError> {
      let specifier_text = specifier
        .to_string()
        .replace(":///", "_")
        .replace("://", "_")
        .replace("/", "-");
      let source_path = self.fixtures.join(specifier_text);
      let media_type = match source_path.extension().unwrap().to_str().unwrap()
      {
        "ts" => {
          if source_path.to_string_lossy().ends_with(".d.ts") {
            MediaType::Dts
          } else {
            MediaType::TypeScript
          }
        }
        "tsx" => MediaType::TSX,
        "js" => MediaType::JavaScript,
        "jsx" => MediaType::JSX,
        _ => MediaType::Unknown,
      };
      let source = fs::read_to_string(&source_path)?;

      Ok(CachedModule {
        source,
        requested_specifier: specifier.clone(),
        source_path,
        specifier,
        media_type,
        ..CachedModule::default()
      })
    }
  }

  impl SpecifierHandler for MockSpecifierHandler {
    fn fetch(&mut self, specifier: ModuleSpecifier) -> FetchFuture {
      Box::pin(future::ready(self.get_cache(specifier)))
    }
    fn get_ts_build_info(
      &self,
      _specifier: &ModuleSpecifier,
    ) -> Result<Option<String>, AnyError> {
      Ok(self.maybe_ts_build_info.clone())
    }
    fn set_cache(
      &mut self,
      specifier: &ModuleSpecifier,
      emit: &Emit,
    ) -> Result<(), AnyError> {
      self.cache_calls.push((specifier.clone(), emit.clone()));
      Ok(())
    }
    fn set_types(
      &mut self,
      specifier: &ModuleSpecifier,
      types: String,
    ) -> Result<(), AnyError> {
      self.types_calls.push((specifier.clone(), types));
      Ok(())
    }
    fn set_ts_build_info(
      &mut self,
      specifier: &ModuleSpecifier,
      ts_build_info: String,
    ) -> Result<(), AnyError> {
      self.maybe_ts_build_info = Some(ts_build_info.clone());
      self
        .ts_build_info_calls
        .push((specifier.clone(), ts_build_info));
      Ok(())
    }
    fn set_deps(
      &mut self,
      specifier: &ModuleSpecifier,
      dependencies: DependencyMap,
    ) -> Result<(), AnyError> {
      self.deps_calls.push((specifier.clone(), dependencies));
      Ok(())
    }
    fn set_version(
      &mut self,
      specifier: &ModuleSpecifier,
      version: String,
    ) -> Result<(), AnyError> {
      self.version_calls.push((specifier.clone(), version));
      Ok(())
    }
  }

  #[test]
  fn test_get_version() {
    let doc_a = "console.log(42);";
    let version_a = get_version(&doc_a, "1.2.3", b"");
    let doc_b = "console.log(42);";
    let version_b = get_version(&doc_b, "1.2.3", b"");
    assert_eq!(version_a, version_b);

    let version_c = get_version(&doc_a, "1.2.3", b"options");
    assert_ne!(version_a, version_c);

    let version_d = get_version(&doc_b, "1.2.3", b"options");
    assert_eq!(version_c, version_d);

    let version_e = get_version(&doc_a, "1.2.4", b"");
    assert_ne!(version_a, version_e);

    let version_f = get_version(&doc_b, "1.2.4", b"");
    assert_eq!(version_e, version_f);
  }

  #[test]
  fn test_module_emit_valid() {
    let source = "console.log(42);".to_string();
    let maybe_version = Some(get_version(&source, version::DENO, b""));
    let module = Module {
      source,
      maybe_version,
      ..Module::default()
    };
    assert!(module.is_emit_valid(b""));

    let source = "console.log(42);".to_string();
    let old_source = "console.log(43);";
    let maybe_version = Some(get_version(old_source, version::DENO, b""));
    let module = Module {
      source,
      maybe_version,
      ..Module::default()
    };
    assert!(!module.is_emit_valid(b""));

    let source = "console.log(42);".to_string();
    let maybe_version = Some(get_version(&source, "0.0.0", b""));
    let module = Module {
      source,
      maybe_version,
      ..Module::default()
    };
    assert!(!module.is_emit_valid(b""));

    let source = "console.log(42);".to_string();
    let module = Module {
      source,
      ..Module::default()
    };
    assert!(!module.is_emit_valid(b""));
  }

  #[test]
  fn test_module_set_version() {
    let source = "console.log(42);".to_string();
    let expected = Some(get_version(&source, version::DENO, b""));
    let mut module = Module {
      source,
      ..Module::default()
    };
    assert!(module.maybe_version.is_none());
    module.set_version(b"");
    assert_eq!(module.maybe_version, expected);
  }

  #[tokio::test]
  async fn test_graph_info() {
    let c = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let fixtures = c.join("tests/module_graph");
    let handler = Rc::new(RefCell::new(MockSpecifierHandler {
      fixtures,
      ..MockSpecifierHandler::default()
    }));
    let mut builder = GraphBuilder2::new(handler.clone(), None);
    let specifier =
      ModuleSpecifier::resolve_url_or_path("file:///tests/main.ts")
        .expect("could not resolve module");
    builder
      .insert(&specifier)
      .await
      .expect("module not inserted");
    let graph = builder.get_graph(&None).expect("could not get graph");
    let info = graph.info().expect("could not get info");
    assert!(info.compiled.is_none());
    assert_eq!(info.dep_count, 6);
    assert_eq!(info.file_type, MediaType::TypeScript);
    assert_eq!(info.files.0.len(), 7);
    assert!(info.local.to_string_lossy().ends_with("file_tests-main.ts"));
    assert!(info.map.is_none());
    assert_eq!(
      info.module,
      ModuleSpecifier::resolve_url_or_path("file:///tests/main.ts").unwrap()
    );
    assert_eq!(info.total_size, 344);
  }

  #[tokio::test]
  async fn test_graph_transpile() {
    // This is a complex scenario of transpiling, where we have TypeScript
    // importing a JavaScript file (with type definitions) which imports
    // TypeScript, JavaScript, and JavaScript with type definitions.
    // For scenarios where we transpile, we only want the TypeScript files
    // to be actually emitted.
    //
    // This also exercises "@deno-types" and type references.
    let c = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let fixtures = c.join("tests/module_graph");
    let handler = Rc::new(RefCell::new(MockSpecifierHandler {
      fixtures,
      ..MockSpecifierHandler::default()
    }));
    let mut builder = GraphBuilder2::new(handler.clone(), None);
    let specifier =
      ModuleSpecifier::resolve_url_or_path("file:///tests/main.ts")
        .expect("could not resolve module");
    builder
      .insert(&specifier)
      .await
      .expect("module not inserted");
    let mut graph = builder.get_graph(&None).expect("could not get graph");
    let (stats, maybe_ignored_options) =
      graph.transpile(TranspileOptions::default()).unwrap();
    assert_eq!(stats.0.len(), 3);
    assert_eq!(maybe_ignored_options, None);
    let h = handler.borrow();
    assert_eq!(h.cache_calls.len(), 2);
    match &h.cache_calls[0].1 {
      Emit::Cli((code, maybe_map)) => {
        assert!(
          code.contains("# sourceMappingURL=data:application/json;base64,")
        );
        assert!(maybe_map.is_none());
      }
    };
    match &h.cache_calls[1].1 {
      Emit::Cli((code, maybe_map)) => {
        assert!(
          code.contains("# sourceMappingURL=data:application/json;base64,")
        );
        assert!(maybe_map.is_none());
      }
    };
    assert_eq!(h.deps_calls.len(), 7);
    assert_eq!(
      h.deps_calls[0].0,
      ModuleSpecifier::resolve_url_or_path("file:///tests/main.ts").unwrap()
    );
    assert_eq!(h.deps_calls[0].1.len(), 1);
    assert_eq!(
      h.deps_calls[1].0,
      ModuleSpecifier::resolve_url_or_path("https://deno.land/x/lib/mod.js")
        .unwrap()
    );
    assert_eq!(h.deps_calls[1].1.len(), 3);
    assert_eq!(
      h.deps_calls[2].0,
      ModuleSpecifier::resolve_url_or_path("https://deno.land/x/lib/mod.d.ts")
        .unwrap()
    );
    assert_eq!(h.deps_calls[2].1.len(), 3, "should have 3 dependencies");
    // sometimes the calls are not deterministic, and so checking the contents
    // can cause some failures
    assert_eq!(h.deps_calls[3].1.len(), 0, "should have no dependencies");
    assert_eq!(h.deps_calls[4].1.len(), 0, "should have no dependencies");
    assert_eq!(h.deps_calls[5].1.len(), 0, "should have no dependencies");
    assert_eq!(h.deps_calls[6].1.len(), 0, "should have no dependencies");
  }

  #[tokio::test]
  async fn test_graph_transpile_user_config() {
    let c = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let fixtures = c.join("tests/module_graph");
    let handler = Rc::new(RefCell::new(MockSpecifierHandler {
      fixtures: fixtures.clone(),
      ..MockSpecifierHandler::default()
    }));
    let mut builder = GraphBuilder2::new(handler.clone(), None);
    let specifier =
      ModuleSpecifier::resolve_url_or_path("https://deno.land/x/transpile.tsx")
        .expect("could not resolve module");
    builder
      .insert(&specifier)
      .await
      .expect("module not inserted");
    let mut graph = builder.get_graph(&None).expect("could not get graph");
    let (_, maybe_ignored_options) = graph
      .transpile(TranspileOptions {
        debug: false,
        maybe_config_path: Some("tests/module_graph/tsconfig.json".to_string()),
      })
      .unwrap();
    assert_eq!(
      maybe_ignored_options.unwrap().items,
      vec!["target".to_string()],
      "the 'target' options should have been ignored"
    );
    let h = handler.borrow();
    assert_eq!(h.cache_calls.len(), 1, "only one file should be emitted");
    // FIXME(bartlomieju): had to add space in `<div>`, probably a quirk in swc_ecma_codegen
    match &h.cache_calls[0].1 {
      Emit::Cli((code, _)) => {
        assert!(
          code.contains("<div >Hello world!</div>"),
          "jsx should have been preserved"
        );
      }
    }
  }

  #[tokio::test]
  async fn test_graph_with_lockfile() {
    let c = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let fixtures = c.join("tests/module_graph");
    let lockfile_path = fixtures.join("lockfile.json");
    let lockfile =
      Lockfile::new(lockfile_path.to_string_lossy().to_string(), false)
        .expect("could not load lockfile");
    let maybe_lockfile = Some(Mutex::new(lockfile));
    let handler = Rc::new(RefCell::new(MockSpecifierHandler {
      fixtures,
      ..MockSpecifierHandler::default()
    }));
    let mut builder = GraphBuilder2::new(handler.clone(), None);
    let specifier =
      ModuleSpecifier::resolve_url_or_path("file:///tests/main.ts")
        .expect("could not resolve module");
    builder
      .insert(&specifier)
      .await
      .expect("module not inserted");
    builder
      .get_graph(&maybe_lockfile)
      .expect("could not get graph");
  }

  #[tokio::test]
  async fn test_graph_with_lockfile_fail() {
    let c = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let fixtures = c.join("tests/module_graph");
    let lockfile_path = fixtures.join("lockfile_fail.json");
    let lockfile =
      Lockfile::new(lockfile_path.to_string_lossy().to_string(), false)
        .expect("could not load lockfile");
    let maybe_lockfile = Some(Mutex::new(lockfile));
    let handler = Rc::new(RefCell::new(MockSpecifierHandler {
      fixtures,
      ..MockSpecifierHandler::default()
    }));
    let mut builder = GraphBuilder2::new(handler.clone(), None);
    let specifier =
      ModuleSpecifier::resolve_url_or_path("file:///tests/main.ts")
        .expect("could not resolve module");
    builder
      .insert(&specifier)
      .await
      .expect("module not inserted");
    builder
      .get_graph(&maybe_lockfile)
      .expect_err("expected an error");
  }
}
