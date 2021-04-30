// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use deno_core::include_js_files;
use deno_core::Extension;
use std::path::PathBuf;

/// Load and execute the javascript code.
pub fn init() -> Extension {
  Extension::builder()
    .js(include_js_files!(
      prefix "deno:extensions/web",
      "00_infra.js",
      "01_dom_exception.js",
      "01_mimesniff.js",
      "02_event.js",
      "03_abort_signal.js",
      "04_global_interfaces.js",
      "08_text_encoding.js",
      "12_location.js",
    ))
    .build()
}

pub fn get_declaration() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib.deno_web.d.ts")
}
