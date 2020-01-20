// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{Deserialize, JsonOp, Value};
use crate::deno_error::other_error;
use crate::fs as deno_fs;
use crate::ops::json_op;
use crate::state::ThreadSafeState;
use deno_core::*;
use std::path::Path;

pub fn init(i: &mut Isolate, s: &ThreadSafeState) {
  i.register_op(
    "query_permission",
    s.core_op(json_op(s.stateful_op(op_query_permission))),
  );
  i.register_op(
    "revoke_permission",
    s.core_op(json_op(s.stateful_op(op_revoke_permission))),
  );
  i.register_op(
    "request_permission",
    s.core_op(json_op(s.stateful_op(op_request_permission))),
  );
}

#[derive(Deserialize)]
struct PermissionArgs {
  name: String,
  url: Option<String>,
  path: Option<String>,
}

fn resolve_path(path: &str) -> String {
  deno_fs::resolve_from_cwd(Path::new(path))
    .unwrap()
    .to_str()
    .unwrap()
    .to_string()
}

pub fn op_query_permission(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: PermissionArgs = serde_json::from_value(args)?;
  let permissions = state.permissions.lock().unwrap();
  let resolved_path = args.path.as_ref().map(String::as_str).map(resolve_path);
  let perm = permissions.get_permission_state(
    &args.name,
    &args.url.as_ref().map(String::as_str),
    &resolved_path.as_ref().map(String::as_str).map(Path::new),
  )?;
  Ok(JsonOp::Sync(json!({ "state": perm.to_string() })))
}

pub fn op_revoke_permission(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: PermissionArgs = serde_json::from_value(args)?;
  let mut permissions = state.permissions.lock().unwrap();
  match args.name.as_ref() {
    "run" => permissions.allow_run.revoke(),
    "read" => permissions.allow_read.revoke(),
    "write" => permissions.allow_write.revoke(),
    "net" => permissions.allow_net.revoke(),
    "env" => permissions.allow_env.revoke(),
    "plugin" => permissions.allow_plugin.revoke(),
    "hrtime" => permissions.allow_hrtime.revoke(),
    _ => {}
  };
  let resolved_path = args.path.as_ref().map(String::as_str).map(resolve_path);
  let perm = permissions.get_permission_state(
    &args.name,
    &args.url.as_ref().map(String::as_str),
    &resolved_path.as_ref().map(String::as_str).map(Path::new),
  )?;
  Ok(JsonOp::Sync(json!({ "state": perm.to_string() })))
}

pub fn op_request_permission(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: PermissionArgs = serde_json::from_value(args)?;
  let mut permissions = state.permissions.lock().unwrap();
  let resolved_path = args.path.as_ref().map(String::as_str).map(resolve_path);
  let perm = match args.name.as_ref() {
    "run" => Ok(permissions.request_run()),
    "read" => Ok(permissions.request_read(
      &resolved_path.as_ref().map(String::as_str).map(Path::new),
    )),
    "write" => Ok(permissions.request_write(
      &resolved_path.as_ref().map(String::as_str).map(Path::new),
    )),
    "net" => permissions.request_net(&args.url.as_ref().map(String::as_str)),
    "env" => Ok(permissions.request_env()),
    "plugin" => Ok(permissions.request_plugin()),
    "hrtime" => Ok(permissions.request_hrtime()),
    n => Err(other_error(format!("No such permission name: {}", n))),
  }?;
  Ok(JsonOp::Sync(json!({ "state": perm.to_string() })))
}
