// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use std::sync::Arc;

use deno_ast::MediaType;
use deno_ast::ModuleSpecifier;
use deno_core::error::AnyError;
use deno_graph::ModuleGraph;
use deno_graph::ModuleKind;
use deno_runtime::colors;
use once_cell::sync::Lazy;
use regex::Regex;

use crate::args::TsConfig;
use crate::args::TypeCheckMode;
use crate::cache::FastInsecureHasher;
use crate::cache::TypeCheckCache;
use crate::npm::NpmPackageResolver;
use crate::tsc;
use crate::tsc::Diagnostics;
use crate::tsc::Stats;
use crate::version;

/// Options for performing a check of a module graph. Note that the decision to
/// emit or not is determined by the `ts_config` settings.
pub struct CheckOptions {
  /// The check flag from the option which can effect the filtering of
  /// diagnostics in the emit result.
  pub type_check_mode: TypeCheckMode,
  /// Set the debug flag on the TypeScript type checker.
  pub debug: bool,
  /// The module specifier to the configuration file, passed to tsc so that
  /// configuration related diagnostics are properly formed.
  pub maybe_config_specifier: Option<ModuleSpecifier>,
  /// The derived tsconfig that should be used when checking.
  pub ts_config: TsConfig,
  /// If true, `Check <specifier>` will be written to stdout for each root.
  pub log_checks: bool,
  /// If true, valid `.tsbuildinfo` files will be ignored and type checking
  /// will always occur.
  pub reload: bool,
  /// If the graph has a node built-in specifier.
  ///
  /// Although this could be derived from the graph, this helps
  /// speed things up.
  pub has_node_builtin_specifier: bool,
}

/// The result of a check of a module graph.
#[derive(Debug, Default)]
pub struct CheckResult {
  pub diagnostics: Diagnostics,
  pub stats: Stats,
}

/// Given a set of roots and graph data, type check the module graph.
///
/// It is expected that it is determined if a check and/or emit is validated
/// before the function is called.
pub fn check(
  graph: Arc<ModuleGraph>,
  cache: &TypeCheckCache,
  npm_resolver: &NpmPackageResolver,
  options: CheckOptions,
) -> Result<CheckResult, AnyError> {
  let check_js = options.ts_config.get_check_js();
  let check_hash = match get_check_hash(&graph, &options) {
    CheckHashResult::NoFiles => return Ok(Default::default()),
    CheckHashResult::Hash(hash) => hash,
  };

  // do not type check if we know this is type checked
  if !options.reload && cache.has_check_hash(check_hash) {
    return Ok(Default::default());
  }

  if options.log_checks {
    for root in &graph.roots {
      let root_str = root.as_str();
      log::info!("{} {}", colors::green("Check"), root_str);
    }
  }

  let root_names =
    get_tsc_roots(&graph, options.has_node_builtin_specifier, check_js);
  // while there might be multiple roots, we can't "merge" the build info, so we
  // try to retrieve the build info for first root, which is the most common use
  // case.
  let maybe_tsbuildinfo = if options.reload {
    None
  } else {
    cache.get_tsbuildinfo(&graph.roots[0])
  };
  // to make tsc build info work, we need to consistently hash modules, so that
  // tsc can better determine if an emit is still valid or not, so we provide
  // that data here.
  let hash_data = vec![
    options.ts_config.as_bytes(),
    version::deno().as_bytes().to_owned(),
  ];

  let response = tsc::exec(tsc::Request {
    config: options.ts_config,
    debug: options.debug,
    graph: graph.clone(),
    hash_data,
    maybe_config_specifier: options.maybe_config_specifier,
    maybe_npm_resolver: Some(npm_resolver.clone()),
    maybe_tsbuildinfo,
    root_names,
  })?;

  let diagnostics = if options.type_check_mode == TypeCheckMode::Local {
    response.diagnostics.filter(|d| {
      if let Some(file_name) = &d.file_name {
        if !file_name.starts_with("http") {
          if ModuleSpecifier::parse(file_name)
            .map(|specifier| !npm_resolver.in_npm_package(&specifier))
            .unwrap_or(true)
          {
            Some(d.clone())
          } else {
            None
          }
        } else {
          None
        }
      } else {
        Some(d.clone())
      }
    })
  } else {
    response.diagnostics
  };

  if let Some(tsbuildinfo) = response.maybe_tsbuildinfo {
    cache.set_tsbuildinfo(&graph.roots[0], &tsbuildinfo);
  }

  if diagnostics.is_empty() {
    cache.add_check_hash(check_hash);
  }

  Ok(CheckResult {
    diagnostics,
    stats: response.stats,
  })
}

enum CheckHashResult {
  Hash(u64),
  NoFiles,
}

/// Gets a hash of the inputs for type checking. This can then
/// be used to tell
fn get_check_hash(
  graph: &ModuleGraph,
  options: &CheckOptions,
) -> CheckHashResult {
  let mut hasher = FastInsecureHasher::new();
  hasher.write_u8(match options.type_check_mode {
    TypeCheckMode::All => 0,
    TypeCheckMode::Local => 1,
    TypeCheckMode::None => 2,
  });
  hasher.write(&options.ts_config.as_bytes());

  let check_js = options.ts_config.get_check_js();
  let mut sorted_modules = graph.modules().collect::<Vec<_>>();
  sorted_modules.sort_by_key(|m| m.specifier.as_str()); // make it deterministic
  let mut has_file = false;
  let mut has_file_to_type_check = false;
  for module in sorted_modules {
    let ts_check =
      has_ts_check(module.media_type, module.maybe_source.as_deref());
    if ts_check {
      has_file_to_type_check = true;
    }

    match module.media_type {
      MediaType::TypeScript
      | MediaType::Dts
      | MediaType::Dmts
      | MediaType::Dcts
      | MediaType::Mts
      | MediaType::Cts
      | MediaType::Tsx => {
        has_file = true;
        has_file_to_type_check = true;
      }
      MediaType::JavaScript
      | MediaType::Mjs
      | MediaType::Cjs
      | MediaType::Jsx => {
        has_file = true;
        if !check_js && !ts_check {
          continue;
        }
      }
      MediaType::Json
      | MediaType::TsBuildInfo
      | MediaType::SourceMap
      | MediaType::Wasm
      | MediaType::Unknown => continue,
    }
    hasher.write_str(module.specifier.as_str());
    if let Some(code) = &module.maybe_source {
      hasher.write_str(code);
    }
  }

  if !has_file || !check_js && !has_file_to_type_check {
    // no files to type check
    CheckHashResult::NoFiles
  } else {
    CheckHashResult::Hash(hasher.finish())
  }
}

/// Transform the graph into root specifiers that we can feed `tsc`. We have to
/// provide the media type for root modules because `tsc` does not "resolve" the
/// media type like other modules, as well as a root specifier needs any
/// redirects resolved. We need to include all the emittable files in
/// the roots, so they get type checked and optionally emitted,
/// otherwise they would be ignored if only imported into JavaScript.
fn get_tsc_roots(
  graph: &ModuleGraph,
  has_node_builtin_specifier: bool,
  check_js: bool,
) -> Vec<(ModuleSpecifier, MediaType)> {
  let mut result = Vec::new();
  if has_node_builtin_specifier {
    // inject a specifier that will resolve node types
    result.push((
      ModuleSpecifier::parse("asset:///node_types.d.ts").unwrap(),
      MediaType::Dts,
    ));
  }
  result.extend(graph.modules().filter_map(|module| {
    if module.kind == ModuleKind::External || module.maybe_source.is_none() {
      return None;
    }
    match module.media_type {
      MediaType::TypeScript
      | MediaType::Tsx
      | MediaType::Mts
      | MediaType::Cts
      | MediaType::Jsx => Some((module.specifier.clone(), module.media_type)),
      MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs
        if check_js
          || has_ts_check(
            module.media_type,
            module.maybe_source.as_deref(),
          ) =>
      {
        Some((module.specifier.clone(), module.media_type))
      }
      _ => None,
    }
  }));
  result
}

/// Matches the `@ts-check` pragma.
static TS_CHECK_RE: Lazy<Regex> =
  Lazy::new(|| Regex::new(r#"(?i)^\s*@ts-check(?:\s+|$)"#).unwrap());

fn has_ts_check(media_type: MediaType, maybe_file_text: Option<&str>) -> bool {
  let file_text = match maybe_file_text {
    Some(text) => text,
    None => return false,
  };
  match &media_type {
    MediaType::JavaScript
    | MediaType::Mjs
    | MediaType::Cjs
    | MediaType::Jsx => get_leading_comments(file_text)
      .iter()
      .any(|text| TS_CHECK_RE.is_match(text)),
    _ => false,
  }
}

fn get_leading_comments(file_text: &str) -> Vec<String> {
  let mut chars = file_text.chars().peekable();

  // skip over the shebang
  if file_text.starts_with("#!") {
    // skip until the end of the line
    for c in chars.by_ref() {
      if c == '\n' {
        break;
      }
    }
  }

  let mut results = Vec::new();
  // now handle the comments
  while chars.peek().is_some() {
    // skip over any whitespace
    while chars
      .peek()
      .map(|c| char::is_whitespace(*c))
      .unwrap_or(false)
    {
      chars.next();
    }

    if chars.next() != Some('/') {
      break;
    }
    match chars.next() {
      Some('/') => {
        let mut text = String::new();
        for c in chars.by_ref() {
          if c == '\n' {
            break;
          } else {
            text.push(c);
          }
        }
        results.push(text);
      }
      Some('*') => {
        let mut text = String::new();
        while let Some(c) = chars.next() {
          if c == '*' && chars.peek() == Some(&'/') {
            chars.next();
            break;
          } else {
            text.push(c);
          }
        }
        results.push(text);
      }
      _ => break,
    }
  }
  results
}

#[cfg(test)]
mod test {
  use deno_ast::MediaType;

  use super::get_leading_comments;
  use super::has_ts_check;

  #[test]
  fn get_leading_comments_test() {
    assert_eq!(
      get_leading_comments(
        "#!/usr/bin/env deno\r\n// test\n/* 1 *//*2*///3\n//\n /**/  /*4 */"
      ),
      vec![
        " test".to_string(),
        " 1 ".to_string(),
        "2".to_string(),
        "3".to_string(),
        "".to_string(),
        "".to_string(),
        "4 ".to_string(),
      ]
    );
    assert_eq!(
      get_leading_comments("//1 /* */ \na;"),
      vec!["1 /* */ ".to_string(),]
    );
    assert_eq!(get_leading_comments("//"), vec!["".to_string()]);
  }

  #[test]
  fn has_ts_check_test() {
    assert!(has_ts_check(
      MediaType::JavaScript,
      Some("// @ts-check\nconsole.log(5);")
    ));
    assert!(has_ts_check(
      MediaType::JavaScript,
      Some("// deno-lint-ignore\n// @ts-check\n")
    ));
    assert!(!has_ts_check(
      MediaType::JavaScript,
      Some("test;\n// @ts-check\n")
    ));
    assert!(!has_ts_check(
      MediaType::JavaScript,
      Some("// ts-check\nconsole.log(5);")
    ));
    assert!(!has_ts_check(MediaType::TypeScript, None,));
  }
}
