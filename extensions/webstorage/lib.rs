// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use deno_core::error::bad_resource_id;
use deno_core::error::AnyError;
use deno_core::include_js_files;
use deno_core::op_sync;
use deno_core::Extension;
use deno_core::OpState;
use deno_core::Resource;
use deno_core::ZeroCopyBuf;
use rusqlite::params;
use rusqlite::Connection;
use rusqlite::OptionalExtension;
use serde::Deserialize;
use std::borrow::Cow;
use std::fmt;
use std::path::PathBuf;

#[derive(Clone)]
struct LocationDataDir(PathBuf);

pub fn init(location_data_dir: Option<PathBuf>) -> Extension {
  Extension::builder()
    .js(include_js_files!(
      prefix "deno:extensions/webstorage",
      "01_webstorage.js",
    ))
    .ops(vec![
      ("op_webstorage_open", op_sync(op_webstorage_open)),
      ("op_webstorage_length", op_sync(op_webstorage_length)),
      ("op_webstorage_key", op_sync(op_webstorage_key)),
      ("op_webstorage_set", op_sync(op_webstorage_set)),
      ("op_webstorage_get", op_sync(op_webstorage_get)),
      ("op_webstorage_remove", op_sync(op_webstorage_remove)),
      ("op_webstorage_clear", op_sync(op_webstorage_clear)),
      (
        "op_webstorage_iterate_keys",
        op_sync(op_webstorage_iterate_keys),
      ),
    ])
    .state(move |state| {
      if let Some(location_data_dir) = location_data_dir.clone() {
        state.put(LocationDataDir(location_data_dir));
      }
      Ok(())
    })
    .build()
}

pub fn get_declaration() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib.deno_webstorage.d.ts")
}

struct WebStorageConnectionResource(Connection);

impl Resource for WebStorageConnectionResource {
  fn name(&self) -> Cow<str> {
    "webStorage".into()
  }
}

pub fn op_webstorage_open(
  state: &mut OpState,
  persistent: bool,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<u32, AnyError> {
  let connection = if persistent {
    let path = state.try_borrow::<LocationDataDir>().ok_or_else(|| {
      DomExceptionNotSupportedError::new(
        "LocalStorage is not supported in this context.",
      )
    })?;
    std::fs::create_dir_all(&path.0)?;
    Connection::open(path.0.join("local_storage"))?
  } else {
    Connection::open_in_memory()?
  };

  connection.execute(
    "CREATE TABLE IF NOT EXISTS data (key VARCHAR UNIQUE, value VARCHAR)",
    params![],
  )?;

  let rid = state
    .resource_table
    .add(WebStorageConnectionResource(connection));
  Ok(rid)
}

pub fn op_webstorage_length(
  state: &mut OpState,
  rid: u32,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<u32, AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(rid)
    .ok_or_else(bad_resource_id)?;

  let mut stmt = resource.0.prepare("SELECT COUNT(*) FROM data")?;

  let length: u32 = stmt.query_row(params![], |row| row.get(0))?;

  Ok(length)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyArgs {
  rid: u32,
  index: u32,
}

pub fn op_webstorage_key(
  state: &mut OpState,
  args: KeyArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<Option<String>, AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(args.rid)
    .ok_or_else(bad_resource_id)?;

  let mut stmt = resource
    .0
    .prepare("SELECT key FROM data LIMIT 1 OFFSET ?")?;

  let key: Option<String> = stmt
    .query_row(params![args.index], |row| row.get(0))
    .optional()?;

  Ok(key)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetArgs {
  rid: u32,
  key_name: String,
  key_value: String,
}

pub fn op_webstorage_set(
  state: &mut OpState,
  args: SetArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<(), AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(args.rid)
    .ok_or_else(bad_resource_id)?;

  let mut stmt = resource
    .0
    .prepare("SELECT SUM(pgsize) FROM dbstat WHERE name = 'data'")?;
  let size: u32 = stmt.query_row(params![], |row| row.get(0))?;

  if size >= 5000000 {
    return Err(
      DomExceptionQuotaExceededError::new("Exceeded maximum storage size")
        .into(),
    );
  }

  resource.0.execute(
    "INSERT OR REPLACE INTO data (key, value) VALUES (?, ?)",
    params![args.key_name, args.key_value],
  )?;

  Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GetArgs {
  rid: u32,
  key_name: String,
}

pub fn op_webstorage_get(
  state: &mut OpState,
  args: GetArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<Option<String>, AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(args.rid)
    .ok_or_else(bad_resource_id)?;

  let mut stmt = resource.0.prepare("SELECT value FROM data WHERE key = ?")?;

  let val = stmt
    .query_row(params![args.key_name], |row| row.get(0))
    .optional()?;

  Ok(val)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveArgs {
  rid: u32,
  key_name: String,
}

pub fn op_webstorage_remove(
  state: &mut OpState,
  args: RemoveArgs,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<(), AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(args.rid)
    .ok_or_else(bad_resource_id)?;

  resource
    .0
    .execute("DELETE FROM data WHERE key = ?", params![args.key_name])?;

  Ok(())
}

pub fn op_webstorage_clear(
  state: &mut OpState,
  rid: u32,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<(), AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(rid)
    .ok_or_else(bad_resource_id)?;

  resource.0.execute("DROP TABLE data", params![])?;
  resource.0.execute(
    "CREATE TABLE data (key VARCHAR UNIQUE, value VARCHAR)",
    params![],
  )?;

  Ok(())
}

pub fn op_webstorage_iterate_keys(
  state: &mut OpState,
  rid: u32,
  _zero_copy: Option<ZeroCopyBuf>,
) -> Result<Vec<String>, AnyError> {
  let resource = state
    .resource_table
    .get::<WebStorageConnectionResource>(rid)
    .ok_or_else(bad_resource_id)?;

  let mut stmt = resource.0.prepare("SELECT key FROM data")?;

  let keys = stmt
    .query_map(params![], |row| row.get::<_, String>(0))?
    .map(|r| r.unwrap())
    .collect();

  Ok(keys)
}

#[derive(Debug)]
pub struct DomExceptionQuotaExceededError {
  pub msg: String,
}

impl DomExceptionQuotaExceededError {
  pub fn new(msg: &str) -> Self {
    DomExceptionQuotaExceededError {
      msg: msg.to_string(),
    }
  }
}

impl fmt::Display for DomExceptionQuotaExceededError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.pad(&self.msg)
  }
}

impl std::error::Error for DomExceptionQuotaExceededError {}

pub fn get_quota_exceeded_error_class_name(
  e: &AnyError,
) -> Option<&'static str> {
  e.downcast_ref::<DomExceptionQuotaExceededError>()
    .map(|_| "DOMExceptionQuotaExceededError")
}

#[derive(Debug)]
pub struct DomExceptionNotSupportedError {
  pub msg: String,
}

impl DomExceptionNotSupportedError {
  pub fn new(msg: &str) -> Self {
    DomExceptionNotSupportedError {
      msg: msg.to_string(),
    }
  }
}

impl fmt::Display for DomExceptionNotSupportedError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    f.pad(&self.msg)
  }
}

impl std::error::Error for DomExceptionNotSupportedError {}

pub fn get_not_supported_error_class_name(
  e: &AnyError,
) -> Option<&'static str> {
  e.downcast_ref::<DomExceptionNotSupportedError>()
    .map(|_| "DOMExceptionNotSupportedError")
}
