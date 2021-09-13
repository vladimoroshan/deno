// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

//! This module provides file formatting utilities using
//! [`dprint-plugin-typescript`](https://github.com/dprint/dprint-plugin-typescript).
//!
//! At the moment it is only consumed using CLI but in
//! the future it can be easily extended to provide
//! the same functions as ops available in JS runtime.

use crate::colors;
use crate::config_file::FmtConfig;
use crate::config_file::FmtOptionsConfig;
use crate::config_file::ProseWrap;
use crate::diff::diff;
use crate::file_watcher;
use crate::file_watcher::ResolutionResult;
use crate::fs_util::{collect_files, get_extension, is_supported_ext_fmt};
use crate::text_encoding;
use deno_ast::ParsedSource;
use deno_core::error::generic_error;
use deno_core::error::AnyError;
use deno_core::futures;
use log::debug;
use log::info;
use std::fs;
use std::io::stdin;
use std::io::stdout;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Format JavaScript/TypeScript files.
pub async fn format(
  args: Vec<PathBuf>,
  ignore: Vec<PathBuf>,
  check: bool,
  watch: bool,
  maybe_fmt_config: Option<FmtConfig>,
) -> Result<(), AnyError> {
  // First, prepare final configuration.
  // Collect included and ignored files. CLI flags take precendence
  // over config file, ie. if there's `files.ignore` in config file
  // and `--ignore` CLI flag, only the flag value is taken into account.
  let mut include_files = args.clone();
  let mut exclude_files = ignore;

  if let Some(fmt_config) = maybe_fmt_config.as_ref() {
    if include_files.is_empty() {
      include_files = fmt_config
        .files
        .include
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>();
    }

    if exclude_files.is_empty() {
      exclude_files = fmt_config
        .files
        .exclude
        .iter()
        .map(PathBuf::from)
        .collect::<Vec<PathBuf>>();
    }
  }

  let fmt_options = maybe_fmt_config.map(|c| c.options).unwrap_or_default();

  let resolver = |changed: Option<Vec<PathBuf>>| {
    let files_changed = changed.is_some();
    let result =
      collect_files(&include_files, &exclude_files, is_supported_ext_fmt).map(
        |files| {
          let collected_files = if let Some(paths) = changed {
            files
              .into_iter()
              .filter(|path| paths.contains(path))
              .collect::<Vec<_>>()
          } else {
            files
          };
          (collected_files, fmt_options.clone())
        },
      );
    let paths_to_watch = include_files.clone();
    async move {
      if (files_changed || !watch)
        && matches!(result, Ok((ref files, _)) if files.is_empty())
      {
        ResolutionResult::Ignore
      } else {
        ResolutionResult::Restart {
          paths_to_watch,
          result,
        }
      }
    }
  };
  let operation = |(paths, fmt_options): (Vec<PathBuf>, FmtOptionsConfig)| async move {
    if check {
      check_source_files(paths, fmt_options).await?;
    } else {
      format_source_files(paths, fmt_options).await?;
    }
    Ok(())
  };

  if watch {
    file_watcher::watch_func(resolver, operation, "Fmt").await?;
  } else {
    let (files, fmt_options) =
      if let ResolutionResult::Restart { result, .. } = resolver(None).await {
        result?
      } else {
        return Err(generic_error("No target files found."));
      };
    operation((files, fmt_options)).await?;
  }

  Ok(())
}

/// Formats markdown (using <https://github.com/dprint/dprint-plugin-markdown>) and its code blocks
/// (ts/tsx, js/jsx).
fn format_markdown(
  file_text: &str,
  fmt_options: &FmtOptionsConfig,
) -> Result<String, String> {
  let markdown_config = get_resolved_markdown_config(fmt_options);
  dprint_plugin_markdown::format_text(
    file_text,
    &markdown_config,
    move |tag, text, line_width| {
      let tag = tag.to_lowercase();
      if matches!(
        tag.as_str(),
        "ts"
          | "tsx"
          | "js"
          | "jsx"
          | "javascript"
          | "typescript"
          | "json"
          | "jsonc"
      ) {
        // It's important to tell dprint proper file extension, otherwise
        // it might parse the file twice.
        let extension = match tag.as_str() {
          "javascript" => "js",
          "typescript" => "ts",
          rest => rest,
        };

        if matches!(extension, "json" | "jsonc") {
          let mut json_config = get_resolved_json_config(fmt_options);
          json_config.line_width = line_width;
          dprint_plugin_json::format_text(text, &json_config)
        } else {
          let fake_filename =
            PathBuf::from(format!("deno_fmt_stdin.{}", extension));
          let mut codeblock_config =
            get_resolved_typescript_config(fmt_options);
          codeblock_config.line_width = line_width;
          dprint_plugin_typescript::format_text(
            &fake_filename,
            text,
            &codeblock_config,
          )
        }
      } else {
        Ok(text.to_string())
      }
    },
  )
  .map_err(|e| e.to_string())
}

/// Formats JSON and JSONC using the rules provided by .deno()
/// of configuration builder of <https://github.com/dprint/dprint-plugin-json>.
/// See <https://git.io/Jt4ht> for configuration.
fn format_json(
  file_text: &str,
  fmt_options: &FmtOptionsConfig,
) -> Result<String, String> {
  let config = get_resolved_json_config(fmt_options);
  dprint_plugin_json::format_text(file_text, &config).map_err(|e| e.to_string())
}

/// Formats a single TS, TSX, JS, JSX, JSONC, JSON, or MD file.
pub fn format_file(
  file_path: &Path,
  file_text: &str,
  fmt_options: FmtOptionsConfig,
) -> Result<String, String> {
  let ext = get_extension(file_path).unwrap_or_else(String::new);
  if ext == "md" {
    format_markdown(file_text, &fmt_options)
  } else if matches!(ext.as_str(), "json" | "jsonc") {
    format_json(file_text, &fmt_options)
  } else {
    let config = get_resolved_typescript_config(&fmt_options);
    dprint_plugin_typescript::format_text(file_path, file_text, &config)
      .map_err(|e| e.to_string())
  }
}

pub fn format_parsed_module(
  parsed_source: &ParsedSource,
  fmt_options: FmtOptionsConfig,
) -> String {
  dprint_plugin_typescript::format_parsed_file(
    &dprint_plugin_typescript::SourceFileInfo {
      is_jsx: matches!(
        parsed_source.media_type(),
        deno_ast::MediaType::Jsx | deno_ast::MediaType::Tsx
      ),
      info: parsed_source.source(),
      leading_comments: parsed_source.comments().leading_map(),
      trailing_comments: parsed_source.comments().trailing_map(),
      module: parsed_source.module(),
      tokens: parsed_source.tokens(),
    },
    &get_resolved_typescript_config(&fmt_options),
  )
}

async fn check_source_files(
  paths: Vec<PathBuf>,
  fmt_options: FmtOptionsConfig,
) -> Result<(), AnyError> {
  let not_formatted_files_count = Arc::new(AtomicUsize::new(0));
  let checked_files_count = Arc::new(AtomicUsize::new(0));

  // prevent threads outputting at the same time
  let output_lock = Arc::new(Mutex::new(0));

  run_parallelized(paths, {
    let not_formatted_files_count = not_formatted_files_count.clone();
    let checked_files_count = checked_files_count.clone();
    move |file_path| {
      checked_files_count.fetch_add(1, Ordering::Relaxed);
      let file_text = read_file_contents(&file_path)?.text;

      match format_file(&file_path, &file_text, fmt_options.clone()) {
        Ok(formatted_text) => {
          if formatted_text != file_text {
            not_formatted_files_count.fetch_add(1, Ordering::Relaxed);
            let _g = output_lock.lock().unwrap();
            let diff = diff(&file_text, &formatted_text);
            info!("");
            info!("{} {}:", colors::bold("from"), file_path.display());
            info!("{}", diff);
          }
        }
        Err(e) => {
          let _g = output_lock.lock().unwrap();
          eprintln!("Error checking: {}", file_path.to_string_lossy());
          eprintln!("   {}", e);
        }
      }
      Ok(())
    }
  })
  .await?;

  let not_formatted_files_count =
    not_formatted_files_count.load(Ordering::Relaxed);
  let checked_files_count = checked_files_count.load(Ordering::Relaxed);
  let checked_files_str =
    format!("{} {}", checked_files_count, files_str(checked_files_count));
  if not_formatted_files_count == 0 {
    info!("Checked {}", checked_files_str);
    Ok(())
  } else {
    let not_formatted_files_str = files_str(not_formatted_files_count);
    Err(generic_error(format!(
      "Found {} not formatted {} in {}",
      not_formatted_files_count, not_formatted_files_str, checked_files_str,
    )))
  }
}

async fn format_source_files(
  paths: Vec<PathBuf>,
  fmt_options: FmtOptionsConfig,
) -> Result<(), AnyError> {
  let formatted_files_count = Arc::new(AtomicUsize::new(0));
  let checked_files_count = Arc::new(AtomicUsize::new(0));
  let output_lock = Arc::new(Mutex::new(0)); // prevent threads outputting at the same time

  run_parallelized(paths, {
    let formatted_files_count = formatted_files_count.clone();
    let checked_files_count = checked_files_count.clone();
    move |file_path| {
      checked_files_count.fetch_add(1, Ordering::Relaxed);
      let file_contents = read_file_contents(&file_path)?;

      match format_file(&file_path, &file_contents.text, fmt_options.clone()) {
        Ok(formatted_text) => {
          if formatted_text != file_contents.text {
            write_file_contents(
              &file_path,
              FileContents {
                had_bom: file_contents.had_bom,
                text: formatted_text,
              },
            )?;
            formatted_files_count.fetch_add(1, Ordering::Relaxed);
            let _g = output_lock.lock().unwrap();
            info!("{}", file_path.to_string_lossy());
          }
        }
        Err(e) => {
          let _g = output_lock.lock().unwrap();
          eprintln!("Error formatting: {}", file_path.to_string_lossy());
          eprintln!("   {}", e);
        }
      }
      Ok(())
    }
  })
  .await?;

  let formatted_files_count = formatted_files_count.load(Ordering::Relaxed);
  debug!(
    "Formatted {} {}",
    formatted_files_count,
    files_str(formatted_files_count),
  );

  let checked_files_count = checked_files_count.load(Ordering::Relaxed);
  info!(
    "Checked {} {}",
    checked_files_count,
    files_str(checked_files_count)
  );

  Ok(())
}

/// Format stdin and write result to stdout.
/// Treats input as TypeScript or as set by `--ext` flag.
/// Compatible with `--check` flag.
pub fn format_stdin(
  check: bool,
  ext: String,
  fmt_options: FmtOptionsConfig,
) -> Result<(), AnyError> {
  let mut source = String::new();
  if stdin().read_to_string(&mut source).is_err() {
    return Err(generic_error("Failed to read from stdin"));
  }
  let file_path = PathBuf::from(format!("_stdin.{}", ext));

  match format_file(&file_path, &source, fmt_options) {
    Ok(formatted_text) => {
      if check {
        if formatted_text != source {
          println!("Not formatted stdin");
        }
      } else {
        stdout().write_all(formatted_text.as_bytes())?;
      }
    }
    Err(e) => {
      return Err(generic_error(e));
    }
  }
  Ok(())
}

fn files_str(len: usize) -> &'static str {
  if len <= 1 {
    "file"
  } else {
    "files"
  }
}

fn get_resolved_typescript_config(
  options: &FmtOptionsConfig,
) -> dprint_plugin_typescript::configuration::Configuration {
  let mut builder =
    dprint_plugin_typescript::configuration::ConfigurationBuilder::new();
  builder.deno();

  if let Some(use_tabs) = options.use_tabs {
    builder.use_tabs(use_tabs);
  }

  if let Some(line_width) = options.line_width {
    builder.line_width(line_width);
  }

  if let Some(indent_width) = options.indent_width {
    builder.indent_width(indent_width);
  }

  if let Some(single_quote) = options.single_quote {
    if single_quote {
      builder.quote_style(
        dprint_plugin_typescript::configuration::QuoteStyle::AlwaysSingle,
      );
    }
  }

  builder.build()
}

fn get_resolved_markdown_config(
  options: &FmtOptionsConfig,
) -> dprint_plugin_markdown::configuration::Configuration {
  let mut builder =
    dprint_plugin_markdown::configuration::ConfigurationBuilder::new();

  builder.deno();

  if let Some(line_width) = options.line_width {
    builder.line_width(line_width);
  }

  if let Some(prose_wrap) = options.prose_wrap {
    builder.text_wrap(match prose_wrap {
      ProseWrap::Always => {
        dprint_plugin_markdown::configuration::TextWrap::Always
      }
      ProseWrap::Never => {
        dprint_plugin_markdown::configuration::TextWrap::Never
      }
      ProseWrap::Preserve => {
        dprint_plugin_markdown::configuration::TextWrap::Maintain
      }
    });
  }

  builder.build()
}

fn get_resolved_json_config(
  options: &FmtOptionsConfig,
) -> dprint_plugin_json::configuration::Configuration {
  let mut builder =
    dprint_plugin_json::configuration::ConfigurationBuilder::new();

  builder.deno();

  if let Some(use_tabs) = options.use_tabs {
    builder.use_tabs(use_tabs);
  }

  if let Some(line_width) = options.line_width {
    builder.line_width(line_width);
  }

  if let Some(indent_width) = options.indent_width {
    builder.indent_width(indent_width);
  }

  builder.build()
}

struct FileContents {
  text: String,
  had_bom: bool,
}

fn read_file_contents(file_path: &Path) -> Result<FileContents, AnyError> {
  let file_bytes = fs::read(&file_path)?;
  let charset = text_encoding::detect_charset(&file_bytes);
  let file_text = text_encoding::convert_to_utf8(&file_bytes, charset)?;
  let had_bom = file_text.starts_with(text_encoding::BOM_CHAR);
  let text = if had_bom {
    text_encoding::strip_bom(&file_text).to_string()
  } else {
    file_text.to_string()
  };

  Ok(FileContents { text, had_bom })
}

fn write_file_contents(
  file_path: &Path,
  file_contents: FileContents,
) -> Result<(), AnyError> {
  let file_text = if file_contents.had_bom {
    // add back the BOM
    format!("{}{}", text_encoding::BOM_CHAR, file_contents.text)
  } else {
    file_contents.text
  };

  Ok(fs::write(file_path, file_text)?)
}

pub async fn run_parallelized<F>(
  file_paths: Vec<PathBuf>,
  f: F,
) -> Result<(), AnyError>
where
  F: FnOnce(PathBuf) -> Result<(), AnyError> + Send + 'static + Clone,
{
  let handles = file_paths.iter().map(|file_path| {
    let f = f.clone();
    let file_path = file_path.clone();
    tokio::task::spawn_blocking(move || f(file_path))
  });
  let join_results = futures::future::join_all(handles).await;

  // find the tasks that panicked and let the user know which files
  let panic_file_paths = join_results
    .iter()
    .enumerate()
    .filter_map(|(i, join_result)| {
      join_result
        .as_ref()
        .err()
        .map(|_| file_paths[i].to_string_lossy())
    })
    .collect::<Vec<_>>();
  if !panic_file_paths.is_empty() {
    panic!("Panic formatting: {}", panic_file_paths.join(", "))
  }

  // check for any errors and if so return the first one
  let mut errors = join_results.into_iter().filter_map(|join_result| {
    join_result
      .ok()
      .map(|handle_result| handle_result.err())
      .flatten()
  });

  if let Some(e) = errors.next() {
    Err(e)
  } else {
    Ok(())
  }
}
