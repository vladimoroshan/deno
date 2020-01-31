// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

//! This module provides file formating utilities using
//! [`dprint`](https://github.com/dsherret/dprint).
//!
//! At the moment it is only consumed using CLI but in
//! the future it can be easily extended to provide
//! the same functions as ops available in JS runtime.

use dprint_plugin_typescript as dprint;
use glob;
use regex::Regex;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

lazy_static! {
  static ref TYPESCRIPT_LIB: Regex = Regex::new(".d.ts$").unwrap();
  static ref TYPESCRIPT: Regex = Regex::new(".tsx?$").unwrap();
  static ref JAVASCRIPT: Regex = Regex::new(".jsx?$").unwrap();
}

fn is_supported(path: &Path) -> bool {
  let path_str = path.to_string_lossy();
  !TYPESCRIPT_LIB.is_match(&path_str)
    && (TYPESCRIPT.is_match(&path_str) || JAVASCRIPT.is_match(&path_str))
}

fn get_config() -> dprint::Configuration {
  dprint::ConfigurationBuilder::new()
    .line_width(80)
    .indent_width(2)
    .next_control_flow_position(dprint::NextControlFlowPosition::SameLine)
    .binary_expression_operator_position(dprint::OperatorPosition::SameLine)
    .build()
}

fn get_supported_files(paths: Vec<PathBuf>) -> Vec<PathBuf> {
  let mut files_to_check = vec![];

  for path in paths {
    if is_supported(&path) {
      files_to_check.push(path.to_owned());
    }
  }

  files_to_check
}

fn check_source_files(config: dprint::Configuration, paths: Vec<PathBuf>) {
  let start = Instant::now();
  let mut not_formatted_files = vec![];

  for file_path in paths {
    let file_path_str = file_path.to_string_lossy();
    let file_contents = fs::read_to_string(&file_path).unwrap();
    match dprint::format_text(&file_path_str, &file_contents, &config) {
      Ok(None) => {
        // nothing to format, pass
      }
      Ok(Some(formatted_text)) => {
        if formatted_text != file_contents {
          println!("Not formatted {}", file_path_str);
          not_formatted_files.push(file_path);
        }
      }
      Err(e) => {
        eprintln!("Error checking: {}", &file_path_str);
        eprintln!("   {}", e);
      }
    }
  }

  let duration = Instant::now() - start;

  if !not_formatted_files.is_empty() {
    let f = if not_formatted_files.len() == 1 {
      "file"
    } else {
      "files"
    };

    eprintln!(
      "Found {} not formatted {} in {:?}",
      not_formatted_files.len(),
      f,
      duration
    );
    std::process::exit(1);
  }
}

fn format_source_files(config: dprint::Configuration, paths: Vec<PathBuf>) {
  let start = Instant::now();
  let mut not_formatted_files = vec![];

  for file_path in paths {
    let file_path_str = file_path.to_string_lossy();
    let file_contents = fs::read_to_string(&file_path).unwrap();
    match dprint::format_text(&file_path_str, &file_contents, &config) {
      Ok(None) => {
        // nothing to format, pass
      }
      Ok(Some(formatted_text)) => {
        if formatted_text != file_contents {
          println!("Formatting {}", file_path_str);
          fs::write(&file_path, formatted_text).unwrap();
          not_formatted_files.push(file_path);
        }
      }
      Err(e) => {
        eprintln!("Error formatting: {}", &file_path_str);
        eprintln!("   {}", e);
      }
    }
  }

  let duration = Instant::now() - start;
  let f = if not_formatted_files.len() == 1 {
    "file"
  } else {
    "files"
  };
  eprintln!(
    "Formatted {} {} in {:?}",
    not_formatted_files.len(),
    f,
    duration
  );
}

fn get_matching_files(glob_paths: Vec<String>) -> Vec<PathBuf> {
  let mut target_files = Vec::with_capacity(128);

  for path in glob_paths {
    let files = glob::glob(&path)
      .expect("Failed to execute glob.")
      .filter_map(Result::ok);
    target_files.extend(files);
  }

  target_files
}

/// Format JavaScript/TypeScript files.
///
/// First argument supports globs, and if it is `None`
/// then the current directory is recursively walked.
pub fn format_files(maybe_files: Option<Vec<String>>, check: bool) {
  // TODO: improve glob to look for tsx?/jsx? files only
  let glob_paths = maybe_files.unwrap_or_else(|| vec!["**/*".to_string()]);
  let matching_files = get_matching_files(glob_paths);
  let matching_files = get_supported_files(matching_files);
  let config = get_config();

  if check {
    check_source_files(config, matching_files);
  } else {
    format_source_files(config, matching_files);
  }
}
