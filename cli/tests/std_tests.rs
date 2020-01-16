// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

// TODO: fix tests in debug mode
// Runs only on release build
#[cfg(not(debug_assertions))]
mod tests {
  extern crate lazy_static;
  extern crate tempfile;
  use deno::test_util::*;
  use std::process::Command;
  use tempfile::TempDir;

  #[test]
  fn std_tests() {
    let dir = TempDir::new().expect("tempdir fail");
    let mut deno_cmd = Command::new(deno_exe_path());
    deno_cmd.env("DENO_DIR", dir.path());

    let mut cwd = root_path();
    cwd.push("std");
    let mut deno = deno_cmd
      .current_dir(cwd) // note: std tests expect to run from "std" dir
      .arg("-A")
      // .arg("-Ldebug")
      .arg("./testing/runner.ts")
      .arg("--exclude=testing/testdata")
      .spawn()
      .expect("failed to spawn script");
    let status = deno.wait().expect("failed to wait for the child process");
    assert!(status.success());
  }
}
