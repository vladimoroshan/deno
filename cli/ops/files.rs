// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{Deserialize, JsonOp, Value};
use super::io::StreamResource;
use crate::deno_error::bad_resource;
use crate::deno_error::DenoError;
use crate::deno_error::ErrorKind;
use crate::fs as deno_fs;
use crate::ops::json_op;
use crate::state::ThreadSafeState;
use deno_core::*;
use futures::future::FutureExt;
use std;
use std::convert::From;
use std::io::SeekFrom;
use std::path::Path;
use tokio;

pub fn init(i: &mut Isolate, s: &ThreadSafeState) {
  i.register_op("open", s.core_op(json_op(s.stateful_op(op_open))));
  i.register_op("close", s.core_op(json_op(s.stateful_op(op_close))));
  i.register_op("seek", s.core_op(json_op(s.stateful_op(op_seek))));
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenArgs {
  promise_id: Option<u64>,
  filename: String,
  options: Option<OpenOptions>,
  mode: Option<String>,
}

#[derive(Deserialize, Default, Debug)]
#[serde(rename_all = "camelCase")]
#[serde(default)]
struct OpenOptions {
  read: bool,
  write: bool,
  create: bool,
  truncate: bool,
  append: bool,
  create_new: bool,
}

fn op_open(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: OpenArgs = serde_json::from_value(args)?;
  let filename = deno_fs::resolve_from_cwd(Path::new(&args.filename))?;
  let state_ = state.clone();
  let mut open_options = tokio::fs::OpenOptions::new();

  if let Some(options) = args.options {
    if options.read {
      state.check_read(&filename)?;
    }

    if options.write || options.append {
      state.check_write(&filename)?;
    }

    open_options
      .read(options.read)
      .create(options.create)
      .write(options.write)
      .truncate(options.truncate)
      .append(options.append)
      .create_new(options.create_new);
  } else if let Some(mode) = args.mode {
    let mode = mode.as_ref();
    match mode {
      "r" => {
        state.check_read(&filename)?;
      }
      "w" | "a" | "x" => {
        state.check_write(&filename)?;
      }
      &_ => {
        state.check_read(&filename)?;
        state.check_write(&filename)?;
      }
    };

    match mode {
      "r" => {
        open_options.read(true);
      }
      "r+" => {
        open_options.read(true).write(true);
      }
      "w" => {
        open_options.create(true).write(true).truncate(true);
      }
      "w+" => {
        open_options
          .read(true)
          .create(true)
          .write(true)
          .truncate(true);
      }
      "a" => {
        open_options.create(true).append(true);
      }
      "a+" => {
        open_options.read(true).create(true).append(true);
      }
      "x" => {
        open_options.create_new(true).write(true);
      }
      "x+" => {
        open_options.create_new(true).read(true).write(true);
      }
      &_ => {
        return Err(ErrBox::from(DenoError::new(
          ErrorKind::Other,
          "Unknown open mode.".to_string(),
        )));
      }
    }
  } else {
    return Err(ErrBox::from(DenoError::new(
      ErrorKind::Other,
      "Open requires either mode or options.".to_string(),
    )));
  };

  let is_sync = args.promise_id.is_none();

  let fut = async move {
    let fs_file = open_options.open(filename).await?;
    let mut table = state_.lock_resource_table();
    let rid = table.add("fsFile", Box::new(StreamResource::FsFile(fs_file)));
    Ok(json!(rid))
  };

  if is_sync {
    let buf = futures::executor::block_on(fut)?;
    Ok(JsonOp::Sync(buf))
  } else {
    Ok(JsonOp::Async(fut.boxed()))
  }
}

#[derive(Deserialize)]
struct CloseArgs {
  rid: i32,
}

fn op_close(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: CloseArgs = serde_json::from_value(args)?;

  let mut table = state.lock_resource_table();
  table.close(args.rid as u32).ok_or_else(bad_resource)?;
  Ok(JsonOp::Sync(json!({})))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SeekArgs {
  promise_id: Option<u64>,
  rid: i32,
  offset: i32,
  whence: i32,
}

fn op_seek(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: SeekArgs = serde_json::from_value(args)?;
  let rid = args.rid as u32;
  let offset = args.offset;
  let whence = args.whence as u32;
  // Translate seek mode to Rust repr.
  let seek_from = match whence {
    0 => SeekFrom::Start(offset as u64),
    1 => SeekFrom::Current(i64::from(offset)),
    2 => SeekFrom::End(i64::from(offset)),
    _ => {
      return Err(ErrBox::from(DenoError::new(
        ErrorKind::InvalidSeekMode,
        format!("Invalid seek mode: {}", whence),
      )));
    }
  };

  let mut table = state.lock_resource_table();
  let resource = table
    .get_mut::<StreamResource>(rid)
    .ok_or_else(bad_resource)?;

  let tokio_file = match resource {
    StreamResource::FsFile(ref mut file) => file,
    _ => return Err(bad_resource()),
  };
  let mut file = futures::executor::block_on(tokio_file.try_clone())?;

  let fut = async move {
    file.seek(seek_from).await?;
    Ok(json!({}))
  };

  if args.promise_id.is_none() {
    let buf = futures::executor::block_on(fut)?;
    Ok(JsonOp::Sync(buf))
  } else {
    Ok(JsonOp::Async(fut.boxed()))
  }
}
