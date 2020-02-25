// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{JsonOp, Value};
use crate::op_error::OpError;
use crate::state::State;
use deno_core::*;

pub fn init(i: &mut Isolate, s: &State) {
  i.register_op("op_resources", s.stateful_json_op(op_resources));
}

fn op_resources(
  state: &State,
  _args: Value,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<JsonOp, OpError> {
  let state = state.borrow();
  let serialized_resources = state.resource_table.entries();
  Ok(JsonOp::Sync(json!(serialized_resources)))
}
