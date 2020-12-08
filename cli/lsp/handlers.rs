// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use super::lsp_extensions;
use super::state::ServerState;
use super::state::ServerStateSnapshot;
use super::text;
use super::tsc;
use super::utils;

use deno_core::error::custom_error;
use deno_core::error::AnyError;
use deno_core::serde_json;
use deno_core::ModuleSpecifier;
use dprint_plugin_typescript as dprint;
use lsp_types::CompletionParams;
use lsp_types::CompletionResponse;
use lsp_types::DocumentFormattingParams;
use lsp_types::DocumentHighlight;
use lsp_types::DocumentHighlightParams;
use lsp_types::GotoDefinitionParams;
use lsp_types::GotoDefinitionResponse;
use lsp_types::Hover;
use lsp_types::HoverParams;
use lsp_types::Location;
use lsp_types::ReferenceParams;
use lsp_types::TextEdit;
use std::path::PathBuf;

fn get_line_index(
  state: &mut ServerState,
  specifier: &ModuleSpecifier,
) -> Result<Vec<u32>, AnyError> {
  let line_index = if specifier.as_url().scheme() == "asset" {
    if let Some(source) = tsc::get_asset(specifier.as_url().path()) {
      text::index_lines(source)
    } else {
      return Err(custom_error(
        "NotFound",
        format!("asset source missing: {}", specifier),
      ));
    }
  } else {
    let file_cache = state.file_cache.read().unwrap();
    if let Some(file_id) = file_cache.lookup(specifier) {
      let file_text = file_cache.get_contents(file_id)?;
      text::index_lines(&file_text)
    } else {
      let mut sources = state.sources.write().unwrap();
      if let Some(line_index) = sources.get_line_index(specifier) {
        line_index
      } else {
        return Err(custom_error(
          "NotFound",
          format!("source for specifier not found: {}", specifier),
        ));
      }
    }
  };
  Ok(line_index)
}

pub fn handle_formatting(
  state: ServerStateSnapshot,
  params: DocumentFormattingParams,
) -> Result<Option<Vec<TextEdit>>, AnyError> {
  let specifier = utils::normalize_url(params.text_document.uri.clone());
  let file_cache = state.file_cache.read().unwrap();
  let file_id = file_cache.lookup(&specifier).unwrap();
  let file_text = file_cache.get_contents(file_id)?;

  let file_path = if let Ok(file_path) = params.text_document.uri.to_file_path()
  {
    file_path
  } else {
    PathBuf::from(params.text_document.uri.path())
  };
  let config = dprint::configuration::ConfigurationBuilder::new()
    .deno()
    .build();

  // TODO(@kitsonk) this could be handled better in `cli/tools/fmt.rs` in the
  // future.
  let new_text = dprint::format_text(&file_path, &file_text, &config)
    .map_err(|e| custom_error("FormatError", e))?;

  let text_edits = text::get_edits(&file_text, &new_text);
  if text_edits.is_empty() {
    Ok(None)
  } else {
    Ok(Some(text_edits))
  }
}

pub fn handle_document_highlight(
  state: &mut ServerState,
  params: DocumentHighlightParams,
) -> Result<Option<Vec<DocumentHighlight>>, AnyError> {
  let specifier = utils::normalize_url(
    params.text_document_position_params.text_document.uri,
  );
  let line_index = get_line_index(state, &specifier)?;
  let server_state = state.snapshot();
  let files_to_search = vec![specifier.clone()];
  let maybe_document_highlights: Option<Vec<tsc::DocumentHighlights>> =
    serde_json::from_value(tsc::request(
      &mut state.ts_runtime,
      &server_state,
      tsc::RequestMethod::GetDocumentHighlights((
        specifier,
        text::to_char_pos(
          &line_index,
          params.text_document_position_params.position,
        ),
        files_to_search,
      )),
    )?)?;

  if let Some(document_highlights) = maybe_document_highlights {
    Ok(Some(
      document_highlights
        .into_iter()
        .map(|dh| dh.to_highlight(&line_index))
        .flatten()
        .collect(),
    ))
  } else {
    Ok(None)
  }
}

pub fn handle_goto_definition(
  state: &mut ServerState,
  params: GotoDefinitionParams,
) -> Result<Option<GotoDefinitionResponse>, AnyError> {
  let specifier = utils::normalize_url(
    params.text_document_position_params.text_document.uri,
  );
  let line_index = get_line_index(state, &specifier)?;
  let server_state = state.snapshot();
  let maybe_definition: Option<tsc::DefinitionInfoAndBoundSpan> =
    serde_json::from_value(tsc::request(
      &mut state.ts_runtime,
      &server_state,
      tsc::RequestMethod::GetDefinition((
        specifier,
        text::to_char_pos(
          &line_index,
          params.text_document_position_params.position,
        ),
      )),
    )?)?;

  if let Some(definition) = maybe_definition {
    Ok(
      definition
        .to_definition(&line_index, |s| get_line_index(state, &s).unwrap()),
    )
  } else {
    Ok(None)
  }
}

pub fn handle_hover(
  state: &mut ServerState,
  params: HoverParams,
) -> Result<Option<Hover>, AnyError> {
  let specifier = utils::normalize_url(
    params.text_document_position_params.text_document.uri,
  );
  let line_index = get_line_index(state, &specifier)?;
  let server_state = state.snapshot();
  let maybe_quick_info: Option<tsc::QuickInfo> =
    serde_json::from_value(tsc::request(
      &mut state.ts_runtime,
      &server_state,
      tsc::RequestMethod::GetQuickInfo((
        specifier,
        text::to_char_pos(
          &line_index,
          params.text_document_position_params.position,
        ),
      )),
    )?)?;

  if let Some(quick_info) = maybe_quick_info {
    Ok(Some(quick_info.to_hover(&line_index)))
  } else {
    Ok(None)
  }
}

pub fn handle_completion(
  state: &mut ServerState,
  params: CompletionParams,
) -> Result<Option<CompletionResponse>, AnyError> {
  let specifier =
    utils::normalize_url(params.text_document_position.text_document.uri);
  let line_index = get_line_index(state, &specifier)?;
  let server_state = state.snapshot();
  let maybe_completion_info: Option<tsc::CompletionInfo> =
    serde_json::from_value(tsc::request(
      &mut state.ts_runtime,
      &server_state,
      tsc::RequestMethod::GetCompletions((
        specifier,
        text::to_char_pos(&line_index, params.text_document_position.position),
        tsc::UserPreferences {
          // TODO(lucacasonato): enable this. see https://github.com/denoland/deno/pull/8651
          include_completions_with_insert_text: Some(false),
          ..Default::default()
        },
      )),
    )?)?;

  if let Some(completions) = maybe_completion_info {
    Ok(Some(completions.into_completion_response(&line_index)))
  } else {
    Ok(None)
  }
}

pub fn handle_references(
  state: &mut ServerState,
  params: ReferenceParams,
) -> Result<Option<Vec<Location>>, AnyError> {
  let specifier =
    utils::normalize_url(params.text_document_position.text_document.uri);
  let line_index = get_line_index(state, &specifier)?;
  let server_state = state.snapshot();
  let maybe_references: Option<Vec<tsc::ReferenceEntry>> =
    serde_json::from_value(tsc::request(
      &mut state.ts_runtime,
      &server_state,
      tsc::RequestMethod::GetReferences((
        specifier,
        text::to_char_pos(&line_index, params.text_document_position.position),
      )),
    )?)?;

  if let Some(references) = maybe_references {
    let mut results = Vec::new();
    for reference in references {
      if !params.context.include_declaration && reference.is_definition {
        continue;
      }
      let reference_specifier =
        ModuleSpecifier::resolve_url(&reference.file_name).unwrap();
      let line_index = get_line_index(state, &reference_specifier)?;
      results.push(reference.to_location(&line_index));
    }

    Ok(Some(results))
  } else {
    Ok(None)
  }
}

pub fn handle_virtual_text_document(
  state: ServerStateSnapshot,
  params: lsp_extensions::VirtualTextDocumentParams,
) -> Result<String, AnyError> {
  let specifier = utils::normalize_url(params.text_document.uri);
  let url = specifier.as_url();
  let contents = if url.as_str() == "deno:///status.md" {
    let file_cache = state.file_cache.read().unwrap();
    format!(
      r#"# Deno Language Server Status

- Documents in memory: {}

"#,
      file_cache.len()
    )
  } else {
    match url.scheme() {
      "asset" => {
        if let Some(text) = tsc::get_asset(url.path()) {
          text.to_string()
        } else {
          error!("Missing asset: {}", specifier);
          "".to_string()
        }
      }
      _ => {
        let mut sources = state.sources.write().unwrap();
        if let Some(text) = sources.get_text(&specifier) {
          text
        } else {
          return Err(custom_error(
            "NotFound",
            format!("The cached sources was not found: {}", specifier),
          ));
        }
      }
    }
  };
  Ok(contents)
}
