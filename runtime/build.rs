// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use deno_core::include_js_files_dir;
use std::env;
use std::path::PathBuf;

// This is a shim that allows to generate documentation on docs.rs
mod not_docs {
  use std::path::Path;

  use super::*;
  use deno_cache::SqliteBackedCache;
  use deno_core::snapshot_util::*;
  use deno_core::Extension;

  use deno_ast::MediaType;
  use deno_ast::ParseParams;
  use deno_ast::SourceTextInfo;
  use deno_core::error::AnyError;
  use deno_core::ExtensionFileSource;

  fn transpile_ts_for_snapshotting(
    file_source: &ExtensionFileSource,
  ) -> Result<String, AnyError> {
    let media_type = MediaType::from(Path::new(&file_source.specifier));

    let should_transpile = match media_type {
      MediaType::JavaScript => false,
      MediaType::TypeScript => true,
      _ => panic!("Unsupported media type for snapshotting {media_type:?}"),
    };

    if !should_transpile {
      return Ok(file_source.code.to_string());
    }

    let parsed = deno_ast::parse_module(ParseParams {
      specifier: file_source.specifier.to_string(),
      text_info: SourceTextInfo::from_string(file_source.code.to_string()),
      media_type,
      capture_tokens: false,
      scope_analysis: false,
      maybe_syntax: None,
    })?;
    let transpiled_source = parsed.transpile(&Default::default())?;
    Ok(transpiled_source.text)
  }

  struct Permissions;

  impl deno_fetch::FetchPermissions for Permissions {
    fn check_net_url(
      &mut self,
      _url: &deno_core::url::Url,
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }

    fn check_read(
      &mut self,
      _p: &Path,
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  impl deno_websocket::WebSocketPermissions for Permissions {
    fn check_net_url(
      &mut self,
      _url: &deno_core::url::Url,
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  impl deno_web::TimersPermission for Permissions {
    fn allow_hrtime(&mut self) -> bool {
      unreachable!("snapshotting!")
    }

    fn check_unstable(
      &self,
      _state: &deno_core::OpState,
      _api_name: &'static str,
    ) {
      unreachable!("snapshotting!")
    }
  }

  impl deno_ffi::FfiPermissions for Permissions {
    fn check(
      &mut self,
      _path: Option<&Path>,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  impl deno_napi::NapiPermissions for Permissions {
    fn check(
      &mut self,
      _path: Option<&Path>,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  impl deno_flash::FlashPermissions for Permissions {
    fn check_net<T: AsRef<str>>(
      &mut self,
      _host: &(T, Option<u16>),
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  impl deno_node::NodePermissions for Permissions {
    fn check_read(
      &mut self,
      _p: &Path,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  impl deno_net::NetPermissions for Permissions {
    fn check_net<T: AsRef<str>>(
      &mut self,
      _host: &(T, Option<u16>),
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }

    fn check_read(
      &mut self,
      _p: &Path,
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }

    fn check_write(
      &mut self,
      _p: &Path,
      _api_name: &str,
    ) -> Result<(), deno_core::error::AnyError> {
      unreachable!("snapshotting!")
    }
  }

  fn create_runtime_snapshot(
    snapshot_path: PathBuf,
    additional_extension: Extension,
  ) {
    let extensions_with_js: Vec<Extension> = vec![
      deno_webidl::init(),
      deno_console::init(),
      deno_url::init(),
      deno_tls::init(),
      deno_web::init::<Permissions>(
        deno_web::BlobStore::default(),
        Default::default(),
      ),
      deno_fetch::init::<Permissions>(Default::default()),
      deno_cache::init::<SqliteBackedCache>(None),
      deno_websocket::init::<Permissions>("".to_owned(), None, None),
      deno_webstorage::init(None),
      deno_crypto::init(None),
      deno_webgpu::init(false),
      deno_broadcast_channel::init(
        deno_broadcast_channel::InMemoryBroadcastChannel::default(),
        false, // No --unstable.
      ),
      deno_node::init::<Permissions>(None),
      deno_ffi::init::<Permissions>(false),
      deno_net::init::<Permissions>(
        None, false, // No --unstable.
        None,
      ),
      deno_napi::init::<Permissions>(false),
      deno_http::init(),
      deno_flash::init::<Permissions>(false), // No --unstable
      additional_extension,
    ];

    create_snapshot(CreateSnapshotOptions {
      cargo_manifest_dir: env!("CARGO_MANIFEST_DIR"),
      snapshot_path,
      startup_snapshot: None,
      extensions: vec![],
      extensions_with_js,
      compression_cb: Some(Box::new(|vec, snapshot_slice| {
        lzzzz::lz4_hc::compress_to_vec(
          snapshot_slice,
          vec,
          lzzzz::lz4_hc::CLEVEL_MAX,
        )
        .expect("snapshot compression failed");
      })),
      snapshot_module_load_cb: Some(Box::new(transpile_ts_for_snapshotting)),
    });
  }

  pub fn build_snapshot(runtime_snapshot_path: PathBuf) {
    #[allow(unused_mut, unused_assignments)]
    let mut esm_files = include_js_files_dir!(
      dir "js",
      "01_build.js",
      "01_errors.js",
      "01_version.ts",
      "06_util.js",
      "10_permissions.js",
      "11_workers.js",
      "12_io.js",
      "13_buffer.js",
      "30_fs.js",
      "30_os.js",
      "40_diagnostics.js",
      "40_files.js",
      "40_fs_events.js",
      "40_http.js",
      "40_process.js",
      "40_read_file.js",
      "40_signals.js",
      "40_spawn.js",
      "40_tty.js",
      "40_write_file.js",
      "41_prompt.js",
      "90_deno_ns.js",
      "98_global_scope.js",
    );

    #[cfg(not(feature = "snapshot_from_snapshot"))]
    {
      esm_files.push(ExtensionFileSource {
        specifier: "js/99_main.js".to_string(),
        code: include_str!("js/99_main.js"),
      });
    }

    let additional_extension =
      Extension::builder("runtime").esm(esm_files).build();
    create_runtime_snapshot(runtime_snapshot_path, additional_extension);
  }
}

fn main() {
  // To debug snapshot issues uncomment:
  // op_fetch_asset::trace_serializer();

  println!("cargo:rustc-env=TARGET={}", env::var("TARGET").unwrap());
  println!("cargo:rustc-env=PROFILE={}", env::var("PROFILE").unwrap());
  let o = PathBuf::from(env::var_os("OUT_DIR").unwrap());

  // Main snapshot
  let runtime_snapshot_path = o.join("RUNTIME_SNAPSHOT.bin");

  // If we're building on docs.rs we just create
  // and empty snapshot file and return, because `rusty_v8`
  // doesn't actually compile on docs.rs
  if env::var_os("DOCS_RS").is_some() {
    let snapshot_slice = &[];
    std::fs::write(&runtime_snapshot_path, snapshot_slice).unwrap();
  }

  #[cfg(not(feature = "docsrs"))]
  not_docs::build_snapshot(runtime_snapshot_path)
}
