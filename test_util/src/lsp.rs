// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use super::new_deno_dir;

use lazy_static::lazy_static;
use regex::Regex;
use serde::de;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use serde_json::Value;
use std::io;
use std::io::Write;
use std::path::Path;
use std::process::Child;
use std::process::ChildStdin;
use std::process::ChildStdout;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;

lazy_static! {
  static ref CONTENT_TYPE_REG: Regex =
    Regex::new(r"(?i)^content-length:\s+(\d+)").unwrap();
}

#[derive(Debug, Deserialize, Serialize)]
pub struct LspResponseError {
  code: i32,
  message: String,
  data: Option<Value>,
}

#[derive(Debug)]
pub enum LspMessage {
  Notification(String, Option<Value>),
  Request(u64, String, Option<Value>),
  Response(u64, Option<Value>, Option<LspResponseError>),
}

impl<'a> From<&'a [u8]> for LspMessage {
  fn from(s: &'a [u8]) -> Self {
    let value: Value = serde_json::from_slice(s).unwrap();
    let obj = value.as_object().unwrap();
    if obj.contains_key("id") && obj.contains_key("method") {
      let id = obj.get("id").unwrap().as_u64().unwrap();
      let method = obj.get("method").unwrap().as_str().unwrap().to_string();
      Self::Request(id, method, obj.get("params").cloned())
    } else if obj.contains_key("id") {
      let id = obj.get("id").unwrap().as_u64().unwrap();
      let maybe_error: Option<LspResponseError> = obj
        .get("error")
        .map(|v| serde_json::from_value(v.clone()).unwrap());
      Self::Response(id, obj.get("result").cloned(), maybe_error)
    } else {
      assert!(obj.contains_key("method"));
      let method = obj.get("method").unwrap().as_str().unwrap().to_string();
      Self::Notification(method, obj.get("params").cloned())
    }
  }
}

fn read_message<R>(reader: &mut R) -> Result<Vec<u8>, anyhow::Error>
where
  R: io::Read + io::BufRead,
{
  let mut content_length = 0_usize;
  loop {
    let mut buf = String::new();
    reader.read_line(&mut buf)?;
    if let Some(captures) = CONTENT_TYPE_REG.captures(&buf) {
      let content_length_match = captures
        .get(1)
        .ok_or_else(|| anyhow::anyhow!("missing capture"))?;
      content_length = content_length_match.as_str().parse::<usize>()?;
    }
    if &buf == "\r\n" {
      break;
    }
  }

  let mut msg_buf = vec![0_u8; content_length];
  reader.read_exact(&mut msg_buf)?;
  Ok(msg_buf)
}

pub struct LspClient {
  reader: io::BufReader<ChildStdout>,
  child: Child,
  request_id: u64,
  start: Instant,
  writer: io::BufWriter<ChildStdin>,
}

impl Drop for LspClient {
  fn drop(&mut self) {
    match self.child.try_wait() {
      Ok(None) => {
        self.child.kill().unwrap();
        let _ = self.child.wait();
      }
      Ok(Some(status)) => panic!("deno lsp exited unexpectedly {}", status),
      Err(e) => panic!("pebble error: {}", e),
    }
  }
}

impl LspClient {
  pub fn new(deno_exe: &Path) -> Result<Self, anyhow::Error> {
    let deno_dir = new_deno_dir();
    let mut child = Command::new(deno_exe)
      .env("DENO_DIR", deno_dir.path())
      .arg("lsp")
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .spawn()?;

    let stdout = child.stdout.take().unwrap();
    let reader = io::BufReader::new(stdout);

    let stdin = child.stdin.take().unwrap();
    let writer = io::BufWriter::new(stdin);

    Ok(Self {
      child,
      reader,
      request_id: 1,
      start: Instant::now(),
      writer,
    })
  }

  pub fn duration(&self) -> Duration {
    self.start.elapsed()
  }

  fn read(&mut self) -> Result<LspMessage, anyhow::Error> {
    let msg_buf = read_message(&mut self.reader)?;
    let msg = LspMessage::from(msg_buf.as_slice());
    Ok(msg)
  }

  pub fn read_notification<R>(
    &mut self,
  ) -> Result<(String, Option<R>), anyhow::Error>
  where
    R: de::DeserializeOwned,
  {
    loop {
      if let LspMessage::Notification(method, maybe_params) = self.read()? {
        if let Some(p) = maybe_params {
          let params = serde_json::from_value(p)?;
          return Ok((method, Some(params)));
        } else {
          return Ok((method, None));
        }
      }
    }
  }

  pub fn read_request<R>(
    &mut self,
  ) -> Result<(u64, String, Option<R>), anyhow::Error>
  where
    R: de::DeserializeOwned,
  {
    loop {
      if let LspMessage::Request(id, method, maybe_params) = self.read()? {
        if let Some(p) = maybe_params {
          let params = serde_json::from_value(p)?;
          return Ok((id, method, Some(params)));
        } else {
          return Ok((id, method, None));
        }
      }
    }
  }

  fn write(&mut self, value: Value) -> Result<(), anyhow::Error> {
    let value_str = value.to_string();
    let msg = format!(
      "Content-Length: {}\r\n\r\n{}",
      value_str.as_bytes().len(),
      value_str
    );
    self.writer.write_all(msg.as_bytes())?;
    self.writer.flush()?;
    Ok(())
  }

  pub fn write_request<S, V, R>(
    &mut self,
    method: S,
    params: V,
  ) -> Result<(Option<R>, Option<LspResponseError>), anyhow::Error>
  where
    S: AsRef<str>,
    V: Serialize,
    R: de::DeserializeOwned,
  {
    let value = json!({
      "jsonrpc": "2.0",
      "id": self.request_id,
      "method": method.as_ref(),
      "params": params,
    });
    self.write(value)?;

    loop {
      if let LspMessage::Response(id, result, error) = self.read()? {
        assert_eq!(id, self.request_id);
        self.request_id += 1;
        if let Some(r) = result {
          let result = serde_json::from_value(r)?;
          return Ok((Some(result), error));
        } else {
          return Ok((None, error));
        }
      }
    }
  }

  pub fn write_response<V>(
    &mut self,
    id: u64,
    result: V,
  ) -> Result<(), anyhow::Error>
  where
    V: Serialize,
  {
    let value = json!({
      "jsonrpc": "2.0",
      "id": id,
      "result": result
    });
    self.write(value)
  }

  pub fn write_notification<S, V>(
    &mut self,
    method: S,
    params: V,
  ) -> Result<(), anyhow::Error>
  where
    S: AsRef<str>,
    V: Serialize,
  {
    let value = json!({
      "jsonrpc": "2.0",
      "method": method.as_ref(),
      "params": params,
    });
    self.write(value)?;
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_read_message() {
    let msg = b"content-length: 11\r\n\r\nhello world";
    let mut reader = std::io::Cursor::new(msg);
    assert_eq!(read_message(&mut reader).unwrap(), b"hello world");
  }
}
