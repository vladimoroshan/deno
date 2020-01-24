// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{Deserialize, JsonOp, Value};
use crate::compilers::runtime_compile_async;
use crate::compilers::runtime_transpile_async;
use crate::ops::json_op;
use crate::state::ThreadSafeState;
use deno_core::*;
use std::collections::HashMap;

pub fn init(i: &mut Isolate, s: &ThreadSafeState) {
  i.register_op("compile", s.core_op(json_op(s.stateful_op(op_compile))));
  i.register_op("transpile", s.core_op(json_op(s.stateful_op(op_transpile))));
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct CompileArgs {
  root_name: String,
  sources: Option<HashMap<String, String>>,
  bundle: bool,
  options: Option<String>,
}

fn op_compile(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: CompileArgs = serde_json::from_value(args)?;
  Ok(JsonOp::Async(runtime_compile_async(
    state.global_state.clone(),
    &args.root_name,
    &args.sources,
    args.bundle,
    &args.options,
  )))
}

#[derive(Deserialize, Debug)]
struct TranspileArgs {
  sources: HashMap<String, String>,
  options: Option<String>,
}

fn op_transpile(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: TranspileArgs = serde_json::from_value(args)?;
  Ok(JsonOp::Async(runtime_transpile_async(
    state.global_state.clone(),
    &args.sources,
    &args.options,
  )))
}
