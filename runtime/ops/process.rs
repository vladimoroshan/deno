// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use super::io::ChildStderrResource;
use super::io::ChildStdinResource;
use super::io::ChildStdoutResource;
use super::io::StdFileResource;
use crate::permissions::Permissions;
use deno_core::error::bad_resource_id;
use deno_core::error::type_error;
use deno_core::error::AnyError;
use deno_core::AsyncMutFuture;
use deno_core::AsyncRefCell;
use deno_core::OpState;
use deno_core::RcRef;
use deno_core::Resource;
use deno_core::ResourceId;
use deno_core::ZeroCopyBuf;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use std::cell::RefCell;
use std::rc::Rc;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

pub fn init(rt: &mut deno_core::JsRuntime) {
  super::reg_sync(rt, "op_run", op_run);
  super::reg_async(rt, "op_run_status", op_run_status);
  super::reg_sync(rt, "op_kill", op_kill);
}

fn clone_file(
  state: &mut OpState,
  rid: ResourceId,
) -> Result<std::fs::File, AnyError> {
  StdFileResource::with(state, rid, move |r| match r {
    Ok(std_file) => std_file.try_clone().map_err(AnyError::from),
    Err(_) => Err(bad_resource_id()),
  })
}

fn subprocess_stdio_map(s: &str) -> Result<std::process::Stdio, AnyError> {
  match s {
    "inherit" => Ok(std::process::Stdio::inherit()),
    "piped" => Ok(std::process::Stdio::piped()),
    "null" => Ok(std::process::Stdio::null()),
    _ => Err(type_error("Invalid resource for stdio")),
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunArgs {
  cmd: Vec<String>,
  cwd: Option<String>,
  env: Vec<(String, String)>,
  stdin: String,
  stdout: String,
  stderr: String,
  stdin_rid: ResourceId,
  stdout_rid: ResourceId,
  stderr_rid: ResourceId,
}

struct ChildResource {
  child: AsyncRefCell<tokio::process::Child>,
}

impl Resource for ChildResource {
  fn name(&self) -> Cow<str> {
    "child".into()
  }
}

impl ChildResource {
  fn borrow_mut(self: Rc<Self>) -> AsyncMutFuture<tokio::process::Child> {
    RcRef::map(self, |r| &r.child).borrow_mut()
  }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
// TODO(@AaronO): maybe find a more descriptive name or a convention for return structs
struct RunInfo {
  rid: ResourceId,
  pid: Option<u32>,
  stdin_rid: Option<ResourceId>,
  stdout_rid: Option<ResourceId>,
  stderr_rid: Option<ResourceId>,
}

fn op_run(
  state: &mut OpState,
  run_args: RunArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<RunInfo, AnyError> {
  let args = run_args.cmd;
  state.borrow_mut::<Permissions>().run.check(&args[0])?;
  let env = run_args.env;
  let cwd = run_args.cwd;

  let mut c = Command::new(args.get(0).unwrap());
  (1..args.len()).for_each(|i| {
    let arg = args.get(i).unwrap();
    c.arg(arg);
  });
  cwd.map(|d| c.current_dir(d));
  for (key, value) in &env {
    c.env(key, value);
  }

  // TODO: make this work with other resources, eg. sockets
  if !run_args.stdin.is_empty() {
    c.stdin(subprocess_stdio_map(run_args.stdin.as_ref())?);
  } else {
    let file = clone_file(state, run_args.stdin_rid)?;
    c.stdin(file);
  }

  if !run_args.stdout.is_empty() {
    c.stdout(subprocess_stdio_map(run_args.stdout.as_ref())?);
  } else {
    let file = clone_file(state, run_args.stdout_rid)?;
    c.stdout(file);
  }

  if !run_args.stderr.is_empty() {
    c.stderr(subprocess_stdio_map(run_args.stderr.as_ref())?);
  } else {
    let file = clone_file(state, run_args.stderr_rid)?;
    c.stderr(file);
  }

  // We want to kill child when it's closed
  c.kill_on_drop(true);

  // Spawn the command.
  let mut child = c.spawn()?;
  let pid = child.id();

  let stdin_rid = match child.stdin.take() {
    Some(child_stdin) => {
      let rid = state
        .resource_table
        .add(ChildStdinResource::from(child_stdin));
      Some(rid)
    }
    None => None,
  };

  let stdout_rid = match child.stdout.take() {
    Some(child_stdout) => {
      let rid = state
        .resource_table
        .add(ChildStdoutResource::from(child_stdout));
      Some(rid)
    }
    None => None,
  };

  let stderr_rid = match child.stderr.take() {
    Some(child_stderr) => {
      let rid = state
        .resource_table
        .add(ChildStderrResource::from(child_stderr));
      Some(rid)
    }
    None => None,
  };

  let child_resource = ChildResource {
    child: AsyncRefCell::new(child),
  };
  let child_rid = state.resource_table.add(child_resource);

  Ok(RunInfo {
    rid: child_rid,
    pid,
    stdin_rid,
    stdout_rid,
    stderr_rid,
  })
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RunStatus {
  got_signal: bool,
  exit_code: i32,
  exit_signal: i32,
}

async fn op_run_status(
  state: Rc<RefCell<OpState>>,
  rid: ResourceId,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<RunStatus, AnyError> {
  let resource = state
    .borrow_mut()
    .resource_table
    .get::<ChildResource>(rid)
    .ok_or_else(bad_resource_id)?;
  let mut child = resource.borrow_mut().await;
  let run_status = child.wait().await?;
  let code = run_status.code();

  #[cfg(unix)]
  let signal = run_status.signal();
  #[cfg(not(unix))]
  let signal = None;

  code
    .or(signal)
    .expect("Should have either an exit code or a signal.");
  let got_signal = signal.is_some();

  Ok(RunStatus {
    got_signal,
    exit_code: code.unwrap_or(-1),
    exit_signal: signal.unwrap_or(-1),
  })
}

#[cfg(unix)]
pub fn kill(pid: i32, signo: i32) -> Result<(), AnyError> {
  use nix::sys::signal::{kill as unix_kill, Signal};
  use nix::unistd::Pid;
  use std::convert::TryFrom;
  let sig = Signal::try_from(signo)?;
  unix_kill(Pid::from_raw(pid), Option::Some(sig)).map_err(AnyError::from)
}

#[cfg(not(unix))]
pub fn kill(pid: i32, signal: i32) -> Result<(), AnyError> {
  use std::io::Error;
  use std::io::ErrorKind::NotFound;
  use winapi::shared::minwindef::DWORD;
  use winapi::shared::minwindef::FALSE;
  use winapi::shared::minwindef::TRUE;
  use winapi::shared::winerror::ERROR_INVALID_PARAMETER;
  use winapi::um::errhandlingapi::GetLastError;
  use winapi::um::handleapi::CloseHandle;
  use winapi::um::processthreadsapi::OpenProcess;
  use winapi::um::processthreadsapi::TerminateProcess;
  use winapi::um::winnt::PROCESS_TERMINATE;

  const SIGINT: i32 = 2;
  const SIGKILL: i32 = 9;
  const SIGTERM: i32 = 15;

  if !matches!(signal, SIGINT | SIGKILL | SIGTERM) {
    Err(type_error("unsupported signal"))
  } else if pid <= 0 {
    Err(type_error("unsupported pid"))
  } else {
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, FALSE, pid as DWORD) };
    if handle.is_null() {
      let err = match unsafe { GetLastError() } {
        ERROR_INVALID_PARAMETER => Error::from(NotFound), // Invalid `pid`.
        errno => Error::from_raw_os_error(errno as i32),
      };
      Err(err.into())
    } else {
      let r = unsafe { TerminateProcess(handle, 1) };
      unsafe { CloseHandle(handle) };
      match r {
        FALSE => Err(Error::last_os_error().into()),
        TRUE => Ok(()),
        _ => unreachable!(),
      }
    }
  }
}

#[derive(Deserialize)]
struct KillArgs {
  pid: i32,
  signo: i32,
}

fn op_kill(
  state: &mut OpState,
  args: KillArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<(), AnyError> {
  super::check_unstable(state, "Deno.kill");
  state.borrow_mut::<Permissions>().run.check_all()?;

  kill(args.pid, args.signo)?;
  Ok(())
}
