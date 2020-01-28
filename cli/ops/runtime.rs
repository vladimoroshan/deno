// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{JsonOp, Value};
use crate::colors;
use crate::fs as deno_fs;
use crate::ops::json_op;
use crate::state::ThreadSafeState;
use crate::version;
use deno_core::*;
use std::env;

/// BUILD_OS and BUILD_ARCH match the values in Deno.build. See js/build.ts.
#[cfg(target_os = "macos")]
static BUILD_OS: &str = "mac";
#[cfg(target_os = "linux")]
static BUILD_OS: &str = "linux";
#[cfg(target_os = "windows")]
static BUILD_OS: &str = "win";
#[cfg(target_arch = "x86_64")]
static BUILD_ARCH: &str = "x64";

pub fn init(i: &mut Isolate, s: &ThreadSafeState) {
  i.register_op("start", s.core_op(json_op(s.stateful_op(op_start))));
}

fn op_start(
  state: &ThreadSafeState,
  _args: Value,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<JsonOp, ErrBox> {
  let gs = &state.global_state;
  let script_args = if gs.flags.argv.len() >= 2 {
    gs.flags.argv.clone().split_off(2)
  } else {
    vec![]
  };
  Ok(JsonOp::Sync(json!({
    "cwd": deno_fs::normalize_path(&env::current_dir().unwrap()),
    "pid": std::process::id(),
    "argv": script_args,
    "mainModule": gs.main_module.as_ref().map(|x| x.to_string()),
    "debugFlag": gs.flags.log_level.map_or(false, |l| l == log::Level::Debug),
    "versionFlag": gs.flags.version,
    "v8Version": version::v8(),
    "denoVersion": version::DENO,
    "tsVersion": version::TYPESCRIPT,
    "noColor": !colors::use_color(),
    "os": BUILD_OS,
    "arch": BUILD_ARCH,
  })))
}
