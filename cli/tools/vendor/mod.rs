// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

use std::path::Path;
use std::path::PathBuf;

use deno_ast::ModuleSpecifier;
use deno_core::anyhow::bail;
use deno_core::anyhow::Context;
use deno_core::error::AnyError;
use deno_core::resolve_url_or_path;
use deno_runtime::permissions::Permissions;
use log::warn;

use crate::args::FmtOptionsConfig;
use crate::args::VendorFlags;
use crate::fs_util;
use crate::fs_util::relative_specifier;
use crate::fs_util::specifier_to_file_path;
use crate::lockfile;
use crate::proc_state::ProcState;
use crate::resolver::ImportMapResolver;
use crate::resolver::JsxResolver;
use crate::tools::fmt::format_json;

mod analyze;
mod build;
mod import_map;
mod mappings;
mod specifiers;
#[cfg(test)]
mod test;

pub async fn vendor(ps: ProcState, flags: VendorFlags) -> Result<(), AnyError> {
  let raw_output_dir = match &flags.output_path {
    Some(output_path) => output_path.to_owned(),
    None => PathBuf::from("vendor/"),
  };
  let output_dir = fs_util::resolve_from_cwd(&raw_output_dir)?;
  validate_output_dir(&output_dir, &flags, &ps)?;
  let graph = create_graph(&ps, &flags).await?;
  let vendored_count = build::build(
    graph,
    &output_dir,
    ps.maybe_import_map.as_deref(),
    &build::RealVendorEnvironment,
  )?;

  eprintln!(
    concat!("Vendored {} {} into {} directory.",),
    vendored_count,
    if vendored_count == 1 {
      "module"
    } else {
      "modules"
    },
    raw_output_dir.display(),
  );
  if vendored_count > 0 {
    let import_map_path = raw_output_dir.join("import_map.json");
    if maybe_update_config_file(&output_dir, &ps) {
      eprintln!(
        concat!(
          "\nUpdated your local Deno configuration file with a reference to the ",
          "new vendored import map at {}. Invoking Deno subcommands will now ",
          "automatically resolve using the vendored modules. You may override ",
          "this by providing the `--import-map <other-import-map>` flag or by ",
          "manually editing your Deno configuration file.",
        ),
        import_map_path.display(),
      );
    } else {
      eprintln!(
        concat!(
          "\nTo use vendored modules, specify the `--import-map {}` flag when ",
          r#"invoking Deno subcommands or add an `"importMap": "<path_to_vendored_import_map>"` "#,
          "entry to a deno.json file.",
        ),
        import_map_path.display(),
      );
    }
  }

  Ok(())
}

fn validate_output_dir(
  output_dir: &Path,
  flags: &VendorFlags,
  ps: &ProcState,
) -> Result<(), AnyError> {
  if !flags.force && !is_dir_empty(output_dir)? {
    bail!(concat!(
      "Output directory was not empty. Please specify an empty directory or use ",
      "--force to ignore this error and potentially overwrite its contents.",
    ));
  }

  // check the import map
  if let Some(import_map_path) = ps
    .maybe_import_map
    .as_ref()
    .and_then(|m| specifier_to_file_path(m.base_url()).ok())
    .and_then(|p| fs_util::canonicalize_path(&p).ok())
  {
    // make the output directory in order to canonicalize it for the check below
    std::fs::create_dir_all(&output_dir)?;
    let output_dir =
      fs_util::canonicalize_path(output_dir).with_context(|| {
        format!("Failed to canonicalize: {}", output_dir.display())
      })?;

    if import_map_path.starts_with(&output_dir) {
      // canonicalize to make the test for this pass on the CI
      let cwd = fs_util::canonicalize_path(&std::env::current_dir()?)?;
      // We don't allow using the output directory to help generate the
      // new state because this may lead to cryptic error messages.
      bail!(
        concat!(
          "Specifying an import map file ({}) in the deno vendor output ",
          "directory is not supported. Please specify no import map or one ",
          "located outside this directory."
        ),
        import_map_path
          .strip_prefix(&cwd)
          .unwrap_or(&import_map_path)
          .display()
          .to_string(),
      );
    }
  }

  Ok(())
}

fn maybe_update_config_file(output_dir: &Path, ps: &ProcState) -> bool {
  assert!(output_dir.is_absolute());
  let config_file_specifier = match ps.options.maybe_config_file_specifier() {
    Some(f) => f,
    None => return false,
  };
  let fmt_config = ps
    .options
    .to_fmt_config()
    .unwrap_or_default()
    .unwrap_or_default();
  let result = update_config_file(
    &config_file_specifier,
    &ModuleSpecifier::from_file_path(output_dir.join("import_map.json"))
      .unwrap(),
    &fmt_config.options,
  );
  match result {
    Ok(()) => true,
    Err(err) => {
      warn!("Error updating config file. {:#}", err);
      false
    }
  }
}

fn update_config_file(
  config_specifier: &ModuleSpecifier,
  import_map_specifier: &ModuleSpecifier,
  fmt_options: &FmtOptionsConfig,
) -> Result<(), AnyError> {
  if config_specifier.scheme() != "file" {
    return Ok(());
  }

  let config_path = specifier_to_file_path(config_specifier)?;
  let config_text = std::fs::read_to_string(&config_path)?;
  let relative_text =
    match relative_specifier(config_specifier, import_map_specifier) {
      Some(text) => text,
      None => return Ok(()), // ignore
    };
  if let Some(new_text) =
    update_config_text(&config_text, &relative_text, fmt_options)
  {
    std::fs::write(config_path, new_text)?;
  }

  Ok(())
}

fn update_config_text(
  text: &str,
  import_map_specifier: &str,
  fmt_options: &FmtOptionsConfig,
) -> Option<String> {
  use jsonc_parser::ast::ObjectProp;
  use jsonc_parser::ast::Value;
  let ast = jsonc_parser::parse_to_ast(text, &Default::default()).ok()?;
  let obj = match ast.value {
    Some(Value::Object(obj)) => obj,
    _ => return None, // shouldn't happen, so ignore
  };
  let import_map_specifier = import_map_specifier.replace('\"', "\\\"");

  match obj.get("importMap") {
    Some(ObjectProp {
      value: Value::StringLit(lit),
      ..
    }) => Some(format!(
      "{}{}{}",
      &text[..lit.range.start + 1],
      import_map_specifier,
      &text[lit.range.end - 1..],
    )),
    None => {
      // insert it crudely at a position that won't cause any issues
      // with comments and format after to make it look nice
      let insert_position = obj.range.end - 1;
      let insert_text = format!(
        r#"{}"importMap": "{}""#,
        if obj.properties.is_empty() { "" } else { "," },
        import_map_specifier
      );
      let new_text = format!(
        "{}{}{}",
        &text[..insert_position],
        insert_text,
        &text[insert_position..],
      );
      format_json(&new_text, fmt_options)
        .ok()
        .map(|formatted_text| formatted_text.unwrap_or(new_text))
    }
    // shouldn't happen, so ignore
    Some(_) => None,
  }
}

fn is_dir_empty(dir_path: &Path) -> Result<bool, AnyError> {
  match std::fs::read_dir(&dir_path) {
    Ok(mut dir) => Ok(dir.next().is_none()),
    Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
    Err(err) => {
      bail!("Error reading directory {}: {}", dir_path.display(), err)
    }
  }
}

async fn create_graph(
  ps: &ProcState,
  flags: &VendorFlags,
) -> Result<deno_graph::ModuleGraph, AnyError> {
  let entry_points = flags
    .specifiers
    .iter()
    .map(|p| {
      let url = resolve_url_or_path(p)?;
      Ok((url, deno_graph::ModuleKind::Esm))
    })
    .collect::<Result<Vec<_>, AnyError>>()?;

  // todo(dsherret): there is a lot of copy and paste here from
  // other parts of the codebase. We should consolidate this.
  let mut cache = crate::cache::FetchCacher::new(
    ps.dir.gen_cache.clone(),
    ps.file_fetcher.clone(),
    Permissions::allow_all(),
    Permissions::allow_all(),
  );
  let maybe_locker = lockfile::as_maybe_locker(ps.lockfile.clone());
  let maybe_imports = ps.options.to_maybe_imports()?;
  let maybe_import_map_resolver =
    ps.maybe_import_map.clone().map(ImportMapResolver::new);
  let maybe_jsx_resolver = ps
    .options
    .to_maybe_jsx_import_source_module()
    .map(|im| JsxResolver::new(im, maybe_import_map_resolver.clone()));
  let maybe_resolver = if maybe_jsx_resolver.is_some() {
    maybe_jsx_resolver.as_ref().map(|jr| jr.as_resolver())
  } else {
    maybe_import_map_resolver
      .as_ref()
      .map(|im| im.as_resolver())
  };

  Ok(
    deno_graph::create_graph(
      entry_points,
      false,
      maybe_imports,
      &mut cache,
      maybe_resolver,
      maybe_locker,
      None,
      None,
    )
    .await,
  )
}

#[cfg(test)]
mod internal_test {
  use super::*;
  use pretty_assertions::assert_eq;

  #[test]
  fn update_config_text_no_existing_props_add_prop() {
    let text = update_config_text(
      "{\n}",
      "./vendor/import_map.json",
      &Default::default(),
    )
    .unwrap();
    assert_eq!(
      text,
      r#"{
  "importMap": "./vendor/import_map.json"
}
"#
    );
  }

  #[test]
  fn update_config_text_existing_props_add_prop() {
    let text = update_config_text(
      r#"{
  "tasks": {
    "task1": "other"
  }
}
"#,
      "./vendor/import_map.json",
      &Default::default(),
    )
    .unwrap();
    assert_eq!(
      text,
      r#"{
  "tasks": {
    "task1": "other"
  },
  "importMap": "./vendor/import_map.json"
}
"#
    );
  }

  #[test]
  fn update_config_text_update_prop() {
    let text = update_config_text(
      r#"{
  "importMap": "./local.json"
}
"#,
      "./vendor/import_map.json",
      &Default::default(),
    )
    .unwrap();
    assert_eq!(
      text,
      r#"{
  "importMap": "./vendor/import_map.json"
}
"#
    );
  }
}
