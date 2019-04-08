// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.

// Think of Resources as File Descriptors. They are integers that are allocated
// by the privileged side of Deno to refer to various resources.  The simplest
// example are standard file system files and stdio - but there will be other
// resources added in the future that might not correspond to operating system
// level File Descriptors. To avoid confusion we call them "resources" not "file
// descriptors". This module implements a global resource table. Ops (AKA
// handlers) look up resources by their integer id here.

use crate::errors;
use crate::errors::bad_resource;
use crate::errors::DenoError;
use crate::errors::DenoResult;
use crate::http_body::HttpBody;
use crate::isolate_state::WorkerChannels;
use crate::repl::Repl;

use deno::Buf;

use futures;
use futures::Future;
use futures::Poll;
use futures::Sink;
use futures::Stream;
use hyper;
use std;
use std::collections::HashMap;
use std::io::{Error, Read, Seek, SeekFrom, Write};
use std::net::{Shutdown, SocketAddr};
use std::process::ExitStatus;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use tokio;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio_process;

pub type ResourceId = u32; // Sometimes referred to RID.

// These store Deno's file descriptors. These are not necessarily the operating
// system ones.
type ResourceTable = HashMap<ResourceId, Repr>;

#[cfg(not(windows))]
use std::os::unix::io::FromRawFd;

#[cfg(windows)]
use std::os::windows::io::FromRawHandle;

#[cfg(windows)]
extern crate winapi;

lazy_static! {
  // Starts at 3 because stdio is [0-2].
  static ref NEXT_RID: AtomicUsize = AtomicUsize::new(3);
  static ref RESOURCE_TABLE: Mutex<ResourceTable> = Mutex::new({
    let mut m = HashMap::new();
    // TODO Load these lazily during lookup?
    m.insert(0, Repr::Stdin(tokio::io::stdin()));

    m.insert(1, Repr::Stdout({
      #[cfg(not(windows))]
      let stdout = unsafe { std::fs::File::from_raw_fd(1) };
      #[cfg(windows)]
      let stdout = unsafe {
        std::fs::File::from_raw_handle(winapi::um::processenv::GetStdHandle(
            winapi::um::winbase::STD_OUTPUT_HANDLE))
      };
      tokio::fs::File::from_std(stdout)
    }));

    m.insert(2, Repr::Stderr(tokio::io::stderr()));
    m
  });
}

// Internal representation of Resource.
enum Repr {
  Stdin(tokio::io::Stdin),
  Stdout(tokio::fs::File),
  Stderr(tokio::io::Stderr),
  FsFile(tokio::fs::File),
  // Since TcpListener might be closed while there is a pending accept task,
  // we need to track the task so that when the listener is closed,
  // this pending task could be notified and die.
  // Currently TcpListener itself does not take care of this issue.
  // See: https://github.com/tokio-rs/tokio/issues/846
  TcpListener(tokio::net::TcpListener, Option<futures::task::Task>),
  TcpStream(tokio::net::TcpStream),
  HttpBody(HttpBody),
  Repl(Arc<Mutex<Repl>>),
  // Enum size is bounded by the largest variant.
  // Use `Box` around large `Child` struct.
  // https://rust-lang.github.io/rust-clippy/master/index.html#large_enum_variant
  Child(Box<tokio_process::Child>),
  ChildStdin(tokio_process::ChildStdin),
  ChildStdout(tokio_process::ChildStdout),
  ChildStderr(tokio_process::ChildStderr),
  Worker(WorkerChannels),
}

/// If the given rid is open, this returns the type of resource, E.G. "worker".
/// If the rid is closed or was never open, it returns None.
pub fn get_type(rid: ResourceId) -> Option<String> {
  let table = RESOURCE_TABLE.lock().unwrap();
  table.get(&rid).map(inspect_repr)
}

pub fn table_entries() -> Vec<(u32, String)> {
  let table = RESOURCE_TABLE.lock().unwrap();

  table
    .iter()
    .map(|(key, value)| (*key, inspect_repr(&value)))
    .collect()
}

#[test]
fn test_table_entries() {
  let mut entries = table_entries();
  entries.sort();
  assert_eq!(entries[0], (0, String::from("stdin")));
  assert_eq!(entries[1], (1, String::from("stdout")));
  assert_eq!(entries[2], (2, String::from("stderr")));
}

fn inspect_repr(repr: &Repr) -> String {
  let h_repr = match repr {
    Repr::Stdin(_) => "stdin",
    Repr::Stdout(_) => "stdout",
    Repr::Stderr(_) => "stderr",
    Repr::FsFile(_) => "fsFile",
    Repr::TcpListener(_, _) => "tcpListener",
    Repr::TcpStream(_) => "tcpStream",
    Repr::HttpBody(_) => "httpBody",
    Repr::Repl(_) => "repl",
    Repr::Child(_) => "child",
    Repr::ChildStdin(_) => "childStdin",
    Repr::ChildStdout(_) => "childStdout",
    Repr::ChildStderr(_) => "childStderr",
    Repr::Worker(_) => "worker",
  };

  String::from(h_repr)
}

// Abstract async file interface.
// Ideally in unix, if Resource represents an OS rid, it will be the same.
#[derive(Clone, Debug)]
pub struct Resource {
  pub rid: ResourceId,
}

impl Resource {
  // TODO Should it return a Resource instead of net::TcpStream?
  pub fn poll_accept(&mut self) -> Poll<(TcpStream, SocketAddr), Error> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      None => Err(std::io::Error::new(
        std::io::ErrorKind::Other,
        "Listener has been closed",
      )),
      Some(repr) => match repr {
        Repr::TcpListener(ref mut s, _) => s.poll_accept(),
        _ => panic!("Cannot accept"),
      },
    }
  }

  // close(2) is done by dropping the value. Therefore we just need to remove
  // the resource from the RESOURCE_TABLE.
  pub fn close(&self) {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let r = table.remove(&self.rid);
    assert!(r.is_some());
  }

  pub fn shutdown(&mut self, how: Shutdown) -> Result<(), DenoError> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      None => panic!("bad rid"),
      Some(repr) => match repr {
        Repr::TcpStream(ref mut f) => {
          TcpStream::shutdown(f, how).map_err(DenoError::from)
        }
        _ => panic!("Cannot shutdown"),
      },
    }
  }
}

impl Read for Resource {
  fn read(&mut self, _buf: &mut [u8]) -> std::io::Result<usize> {
    unimplemented!();
  }
}

impl AsyncRead for Resource {
  fn poll_read(&mut self, buf: &mut [u8]) -> Poll<usize, Error> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      None => panic!("bad rid"),
      Some(repr) => match repr {
        Repr::FsFile(ref mut f) => f.poll_read(buf),
        Repr::Stdin(ref mut f) => f.poll_read(buf),
        Repr::TcpStream(ref mut f) => f.poll_read(buf),
        Repr::HttpBody(ref mut f) => f.poll_read(buf),
        Repr::ChildStdout(ref mut f) => f.poll_read(buf),
        Repr::ChildStderr(ref mut f) => f.poll_read(buf),
        _ => panic!("Cannot read"),
      },
    }
  }
}

impl Write for Resource {
  fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
    unimplemented!()
  }

  fn flush(&mut self) -> std::io::Result<()> {
    unimplemented!()
  }
}

impl AsyncWrite for Resource {
  fn poll_write(&mut self, buf: &[u8]) -> Poll<usize, Error> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      None => panic!("bad rid"),
      Some(repr) => match repr {
        Repr::FsFile(ref mut f) => f.poll_write(buf),
        Repr::Stdout(ref mut f) => f.poll_write(buf),
        Repr::Stderr(ref mut f) => f.poll_write(buf),
        Repr::TcpStream(ref mut f) => f.poll_write(buf),
        Repr::ChildStdin(ref mut f) => f.poll_write(buf),
        _ => panic!("Cannot write"),
      },
    }
  }

  fn shutdown(&mut self) -> futures::Poll<(), std::io::Error> {
    unimplemented!()
  }
}

fn new_rid() -> ResourceId {
  let next_rid = NEXT_RID.fetch_add(1, Ordering::SeqCst);
  next_rid as ResourceId
}

pub fn add_fs_file(fs_file: tokio::fs::File) -> Resource {
  let rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();
  match tg.insert(rid, Repr::FsFile(fs_file)) {
    Some(_) => panic!("There is already a file with that rid"),
    None => Resource { rid },
  }
}

pub fn add_tcp_listener(listener: tokio::net::TcpListener) -> Resource {
  let rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();
  let r = tg.insert(rid, Repr::TcpListener(listener, None));
  assert!(r.is_none());
  Resource { rid }
}

pub fn add_tcp_stream(stream: tokio::net::TcpStream) -> Resource {
  let rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();
  let r = tg.insert(rid, Repr::TcpStream(stream));
  assert!(r.is_none());
  Resource { rid }
}

pub fn add_hyper_body(body: hyper::Body) -> Resource {
  let rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();
  let body = HttpBody::from(body);
  let r = tg.insert(rid, Repr::HttpBody(body));
  assert!(r.is_none());
  Resource { rid }
}

pub fn add_repl(repl: Repl) -> Resource {
  let rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();
  let r = tg.insert(rid, Repr::Repl(Arc::new(Mutex::new(repl))));
  assert!(r.is_none());
  Resource { rid }
}

pub fn add_worker(wc: WorkerChannels) -> Resource {
  let rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();
  let r = tg.insert(rid, Repr::Worker(wc));
  assert!(r.is_none());
  Resource { rid }
}

/// Post message to worker as a host or privilged overlord
pub fn post_message_to_worker(
  rid: ResourceId,
  buf: Buf,
) -> futures::sink::Send<mpsc::Sender<Buf>> {
  let mut table = RESOURCE_TABLE.lock().unwrap();
  let maybe_repr = table.get_mut(&rid);
  match maybe_repr {
    Some(Repr::Worker(ref mut wc)) => {
      // unwrap here is incorrect, but doing it anyway
      wc.0.clone().send(buf)
    }
    _ => panic!("bad resource"), // futures::future::err(bad_resource()).into(),
  }
}

pub struct WorkerReceiver {
  rid: ResourceId,
}

// Invert the dumbness that tokio_process causes by making Child itself a future.
impl Future for WorkerReceiver {
  type Item = Option<Buf>;
  type Error = DenoError;

  fn poll(&mut self) -> Poll<Option<Buf>, DenoError> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      Some(Repr::Worker(ref mut wc)) => wc
        .1
        .poll()
        .map_err(|err| errors::new(errors::ErrorKind::Other, err.to_string())),
      _ => Err(bad_resource()),
    }
  }
}

pub fn get_message_from_worker(rid: ResourceId) -> WorkerReceiver {
  WorkerReceiver { rid }
}

pub struct WorkerReceiverStream {
  rid: ResourceId,
}

// Invert the dumbness that tokio_process causes by making Child itself a future.
impl Stream for WorkerReceiverStream {
  type Item = Buf;
  type Error = DenoError;

  fn poll(&mut self) -> Poll<Option<Buf>, DenoError> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      Some(Repr::Worker(ref mut wc)) => wc
        .1
        .poll()
        .map_err(|err| errors::new(errors::ErrorKind::Other, err.to_string())),
      _ => Err(bad_resource()),
    }
  }
}

pub fn get_message_stream_from_worker(rid: ResourceId) -> WorkerReceiverStream {
  WorkerReceiverStream { rid }
}

#[cfg_attr(feature = "cargo-clippy", allow(stutter))]
pub struct ChildResources {
  pub child_rid: ResourceId,
  pub stdin_rid: Option<ResourceId>,
  pub stdout_rid: Option<ResourceId>,
  pub stderr_rid: Option<ResourceId>,
}

pub fn add_child(mut c: tokio_process::Child) -> ChildResources {
  let child_rid = new_rid();
  let mut tg = RESOURCE_TABLE.lock().unwrap();

  let mut resources = ChildResources {
    child_rid,
    stdin_rid: None,
    stdout_rid: None,
    stderr_rid: None,
  };

  if c.stdin().is_some() {
    let stdin = c.stdin().take().unwrap();
    let rid = new_rid();
    let r = tg.insert(rid, Repr::ChildStdin(stdin));
    assert!(r.is_none());
    resources.stdin_rid = Some(rid);
  }
  if c.stdout().is_some() {
    let stdout = c.stdout().take().unwrap();
    let rid = new_rid();
    let r = tg.insert(rid, Repr::ChildStdout(stdout));
    assert!(r.is_none());
    resources.stdout_rid = Some(rid);
  }
  if c.stderr().is_some() {
    let stderr = c.stderr().take().unwrap();
    let rid = new_rid();
    let r = tg.insert(rid, Repr::ChildStderr(stderr));
    assert!(r.is_none());
    resources.stderr_rid = Some(rid);
  }

  let r = tg.insert(child_rid, Repr::Child(Box::new(c)));
  assert!(r.is_none());

  resources
}

pub struct ChildStatus {
  rid: ResourceId,
}

// Invert the dumbness that tokio_process causes by making Child itself a future.
impl Future for ChildStatus {
  type Item = ExitStatus;
  type Error = DenoError;

  fn poll(&mut self) -> Poll<ExitStatus, DenoError> {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let maybe_repr = table.get_mut(&self.rid);
    match maybe_repr {
      Some(Repr::Child(ref mut child)) => child.poll().map_err(DenoError::from),
      _ => Err(bad_resource()),
    }
  }
}

pub fn child_status(rid: ResourceId) -> DenoResult<ChildStatus> {
  let mut table = RESOURCE_TABLE.lock().unwrap();
  let maybe_repr = table.get_mut(&rid);
  match maybe_repr {
    Some(Repr::Child(ref mut _child)) => Ok(ChildStatus { rid }),
    _ => Err(bad_resource()),
  }
}

pub fn get_repl(rid: ResourceId) -> DenoResult<Arc<Mutex<Repl>>> {
  let mut table = RESOURCE_TABLE.lock().unwrap();
  let maybe_repr = table.get_mut(&rid);
  match maybe_repr {
    Some(Repr::Repl(ref mut r)) => Ok(r.clone()),
    _ => Err(bad_resource()),
  }
}

pub fn lookup(rid: ResourceId) -> Option<Resource> {
  debug!("resource lookup {}", rid);
  let table = RESOURCE_TABLE.lock().unwrap();
  table.get(&rid).map(|_| Resource { rid })
}

// TODO(kevinkassimo): revamp this after the following lands:
// https://github.com/tokio-rs/tokio/pull/785
pub fn seek(
  resource: Resource,
  offset: i32,
  whence: u32,
) -> Box<dyn Future<Item = (), Error = DenoError> + Send> {
  let mut table = RESOURCE_TABLE.lock().unwrap();
  // We take ownership of File here.
  // It is put back below while still holding the lock.
  let maybe_repr = table.remove(&resource.rid);
  match maybe_repr {
    None => panic!("bad rid"),
    Some(Repr::FsFile(f)) => {
      // Trait Clone not implemented on tokio::fs::File,
      // so convert to std File first.
      let std_file = f.into_std();
      // Create a copy and immediately put back.
      // We don't want to block other resource ops.
      // try_clone() would yield a copy containing the same
      // underlying fd, so operations on the copy would also
      // affect the one in resource table, and we don't need
      // to write back.
      let maybe_std_file_copy = std_file.try_clone();
      // Insert the entry back with the same rid.
      table.insert(
        resource.rid,
        Repr::FsFile(tokio_fs::File::from_std(std_file)),
      );
      // Translate seek mode to Rust repr.
      let seek_from = match whence {
        0 => SeekFrom::Start(offset as u64),
        1 => SeekFrom::Current(i64::from(offset)),
        2 => SeekFrom::End(i64::from(offset)),
        _ => {
          return Box::new(futures::future::err(errors::new(
            errors::ErrorKind::InvalidSeekMode,
            format!("Invalid seek mode: {}", whence),
          )));
        }
      };
      if maybe_std_file_copy.is_err() {
        return Box::new(futures::future::err(DenoError::from(
          maybe_std_file_copy.unwrap_err(),
        )));
      }
      let mut std_file_copy = maybe_std_file_copy.unwrap();
      Box::new(futures::future::lazy(move || {
        let result = std_file_copy
          .seek(seek_from)
          .map(|_| {})
          .map_err(DenoError::from);
        futures::future::result(result)
      }))
    }
    _ => panic!("cannot seek"),
  }
}
