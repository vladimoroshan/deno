// Copyright 2018 the Deno authors. All rights reserved. MIT license.

// Run "cargo build -vv" if you want to see gn output.
// TODO For the time being you must set an env var DENO_BUILD_PATH
// which might be `pwd`/out/debug or `pwd`/out/release.
// TODO Currently DENO_BUILD_PATH must be absolute.
// TODO Combine DENO_BUILD_PATH and OUT_DIR.

use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
  let mode = env::var("PROFILE").unwrap();
  let deno_build_path = env::var("DENO_BUILD_PATH").unwrap();

  // Detect if we're being invoked by the rust language server (RLS).
  // Unfortunately we can't detect whether we're being run by `cargo check`.
  let check_only = env::var_os("CARGO")
    .map(PathBuf::from)
    .as_ref()
    .and_then(|p| p.file_stem())
    .and_then(|f| f.to_str())
    .map(|s| s.starts_with("rls"))
    .unwrap_or(false);

  // If we're being invoked by the RLS, build only the targets that are needed
  // for `cargo check` to succeed.
  let gn_target = if check_only {
    "cargo_check_deps"
  } else {
    "deno_deps"
  };

  let status = Command::new("python")
    .env("DENO_BUILD_PATH", &deno_build_path)
    .env("DENO_BUILD_MODE", &mode)
    .arg("./tools/setup.py")
    .status()
    .expect("setup.py failed");
  assert!(status.success());

  // These configurations must be outputted after tools/setup.py is run.
  println!("cargo:rustc-link-search=native={}/obj", deno_build_path);
  println!("cargo:rustc-link-lib=static=deno_deps");
  // TODO Remove this and only use OUT_DIR at some point.
  println!("cargo:rustc-env=DENO_BUILD_PATH={}", deno_build_path);

  let status = Command::new("python")
    .env("DENO_BUILD_PATH", &deno_build_path)
    .env("DENO_BUILD_MODE", &mode)
    .arg("./tools/build.py")
    .arg(gn_target)
    .arg("-v")
    .status()
    .expect("build.py failed");
  assert!(status.success());
}
