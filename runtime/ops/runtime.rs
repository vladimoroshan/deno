// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use crate::metrics::OpMetrics;
use crate::metrics::RuntimeMetrics;
use crate::ops::UnstableChecker;
use crate::permissions::Permissions;
use deno_core::error::AnyError;
use deno_core::error::Context;
use deno_core::serde_json::json;
use deno_core::serde_json::Value;
use deno_core::ModuleSpecifier;
use deno_core::OpState;
use deno_core::ZeroCopyBuf;

pub fn init(rt: &mut deno_core::JsRuntime, main_module: ModuleSpecifier) {
  {
    let op_state = rt.op_state();
    let mut state = op_state.borrow_mut();
    state.put::<ModuleSpecifier>(main_module);
  }
  super::reg_sync(rt, "op_main_module", op_main_module);
  super::reg_sync(rt, "op_metrics", op_metrics);
}

fn op_main_module(
  state: &mut OpState,
  _args: (),
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<String, AnyError> {
  let main = state.borrow::<ModuleSpecifier>().to_string();
  let main_url = deno_core::resolve_url_or_path(&main)?;
  if main_url.scheme() == "file" {
    let main_path = std::env::current_dir()
      .context("Failed to get current working directory")?
      .join(main_url.to_string());
    state
      .borrow_mut::<Permissions>()
      .read
      .check_blind(&main_path, "main_module")?;
  }
  Ok(main)
}

#[derive(serde::Serialize)]
struct MetricsReturn {
  combined: OpMetrics,
  ops: Value,
}

#[allow(clippy::unnecessary_wraps)]
fn op_metrics(
  state: &mut OpState,
  _args: (),
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<MetricsReturn, AnyError> {
  let m = state.borrow::<RuntimeMetrics>();
  let combined = m.combined_metrics();
  let unstable_checker = state.borrow::<UnstableChecker>();
  let maybe_ops = if unstable_checker.unstable {
    Some(&m.ops)
  } else {
    None
  };
  Ok(MetricsReturn {
    combined,
    ops: json!(maybe_ops),
  })
}

pub fn ppid() -> i64 {
  #[cfg(windows)]
  {
    // Adopted from rustup:
    // https://github.com/rust-lang/rustup/blob/1.21.1/src/cli/self_update.rs#L1036
    // Copyright Diggory Blake, the Mozilla Corporation, and rustup contributors.
    // Licensed under either of
    // - Apache License, Version 2.0
    // - MIT license
    use std::mem;
    use winapi::shared::minwindef::DWORD;
    use winapi::um::handleapi::{CloseHandle, INVALID_HANDLE_VALUE};
    use winapi::um::processthreadsapi::GetCurrentProcessId;
    use winapi::um::tlhelp32::{
      CreateToolhelp32Snapshot, Process32First, Process32Next, PROCESSENTRY32,
      TH32CS_SNAPPROCESS,
    };
    unsafe {
      // Take a snapshot of system processes, one of which is ours
      // and contains our parent's pid
      let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
      if snapshot == INVALID_HANDLE_VALUE {
        return -1;
      }

      let mut entry: PROCESSENTRY32 = mem::zeroed();
      entry.dwSize = mem::size_of::<PROCESSENTRY32>() as DWORD;

      // Iterate over system processes looking for ours
      let success = Process32First(snapshot, &mut entry);
      if success == 0 {
        CloseHandle(snapshot);
        return -1;
      }

      let this_pid = GetCurrentProcessId();
      while entry.th32ProcessID != this_pid {
        let success = Process32Next(snapshot, &mut entry);
        if success == 0 {
          CloseHandle(snapshot);
          return -1;
        }
      }
      CloseHandle(snapshot);

      // FIXME: Using the process ID exposes a race condition
      // wherein the parent process already exited and the OS
      // reassigned its ID.
      let parent_id = entry.th32ParentProcessID;
      parent_id.into()
    }
  }
  #[cfg(not(windows))]
  {
    use std::os::unix::process::parent_id;
    parent_id().into()
  }
}
