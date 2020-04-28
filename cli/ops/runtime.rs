// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{JsonOp, Value};
use crate::colors;
use crate::op_error::OpError;
use crate::state::State;
use crate::version;
use crate::DenoSubcommand;
use deno_core::CoreIsolate;
use deno_core::ZeroCopyBuf;
use std::env;

pub fn init(i: &mut CoreIsolate, s: &State) {
  i.register_op("op_start", s.stateful_json_op(op_start));
  i.register_op("op_metrics", s.stateful_json_op(op_metrics));
}

fn op_start(
  state: &State,
  _args: Value,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<JsonOp, OpError> {
  let state = state.borrow();
  let gs = &state.global_state;

  Ok(JsonOp::Sync(json!({
    // TODO(bartlomieju): `cwd` field is not used in JS, remove?
    "cwd": &env::current_dir().unwrap(),
    "pid": std::process::id(),
    "args": gs.flags.argv.clone(),
    "repl": gs.flags.subcommand == DenoSubcommand::Repl,
    "location": state.main_module.to_string(),
    "debugFlag": gs.flags.log_level.map_or(false, |l| l == log::Level::Debug),
    "versionFlag": gs.flags.version,
    "v8Version": version::v8(),
    "denoVersion": version::DENO,
    "tsVersion": version::TYPESCRIPT,
    "noColor": !colors::use_color(),
    "target": env!("TARGET"),
  })))
}

fn op_metrics(
  state: &State,
  _args: Value,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<JsonOp, OpError> {
  let state = state.borrow();
  let m = &state.metrics;

  Ok(JsonOp::Sync(json!({
    "opsDispatched": m.ops_dispatched,
    "opsDispatchedSync": m.ops_dispatched_sync,
    "opsDispatchedAsync": m.ops_dispatched_async,
    "opsDispatchedAsyncUnref": m.ops_dispatched_async_unref,
    "opsCompleted": m.ops_completed,
    "opsCompletedSync": m.ops_completed_sync,
    "opsCompletedAsync": m.ops_completed_async,
    "opsCompletedAsyncUnref": m.ops_completed_async_unref,
    "bytesSentControl": m.bytes_sent_control,
    "bytesSentData": m.bytes_sent_data,
    "bytesReceived": m.bytes_received
  })))
}
