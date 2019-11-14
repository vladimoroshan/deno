// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
use super::dispatch_json::{Deserialize, JsonOp, Value};
use crate::deno_error::bad_resource;
use crate::deno_error::DenoError;
use crate::deno_error::ErrorKind;
use crate::ops::json_op;
use crate::resolve_addr::resolve_addr;
use crate::resources;
use crate::resources::Resource;
use crate::state::ThreadSafeState;
use deno::*;
use futures::Async;
use futures::Future;
use futures::Poll;
use std;
use std::convert::From;
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio;
use tokio::net::TcpListener;
use tokio::net::TcpStream;
use tokio_rustls::{rustls::ClientConfig, TlsConnector};
use tokio_rustls::{
  rustls::{
    internal::pemfile::{certs, pkcs8_private_keys, rsa_private_keys},
    Certificate, NoClientAuth, PrivateKey, ServerConfig,
  },
  TlsAcceptor,
};
use webpki;
use webpki::DNSNameRef;
use webpki_roots;

pub fn init(i: &mut Isolate, s: &ThreadSafeState) {
  i.register_op("dial_tls", s.core_op(json_op(s.stateful_op(op_dial_tls))));
  i.register_op(
    "listen_tls",
    s.core_op(json_op(s.stateful_op(op_listen_tls))),
  );
  i.register_op(
    "accept_tls",
    s.core_op(json_op(s.stateful_op(op_accept_tls))),
  );
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DialTLSArgs {
  hostname: String,
  port: u16,
  cert_file: Option<String>,
}

pub fn op_dial_tls(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: DialTLSArgs = serde_json::from_value(args)?;
  let cert_file = args.cert_file;

  state.check_net(&args.hostname, args.port)?;
  if let Some(path) = cert_file.clone() {
    state.check_read(&path)?;
  }

  let mut domain = args.hostname.clone();
  if domain.is_empty() {
    domain.push_str("localhost");
  }

  let op = resolve_addr(&args.hostname, args.port).and_then(move |addr| {
    TcpStream::connect(&addr)
      .and_then(move |tcp_stream| {
        let local_addr = tcp_stream.local_addr()?;
        let remote_addr = tcp_stream.peer_addr()?;
        let mut config = ClientConfig::new();
        config
          .root_store
          .add_server_trust_anchors(&webpki_roots::TLS_SERVER_ROOTS);

        if let Some(path) = cert_file {
          let key_file = File::open(path)?;
          let reader = &mut BufReader::new(key_file);
          config.root_store.add_pem_file(reader).unwrap();
        }

        let tls_connector = TlsConnector::from(Arc::new(config));
        Ok((tls_connector, tcp_stream, local_addr, remote_addr))
      })
      .map_err(ErrBox::from)
      .and_then(
        move |(tls_connector, tcp_stream, local_addr, remote_addr)| {
          let dnsname = DNSNameRef::try_from_ascii_str(&domain)
            .expect("Invalid DNS lookup");
          tls_connector
            .connect(dnsname, tcp_stream)
            .map_err(ErrBox::from)
            .and_then(move |tls_stream| {
              let rid = resources::add_tls_stream(tls_stream);
              futures::future::ok(json!({
                "rid": rid,
                "localAddr": local_addr.to_string(),
                "remoteAddr": remote_addr.to_string(),
              }))
            })
        },
      )
  });

  Ok(JsonOp::Async(Box::new(op)))
}

fn load_certs(path: &str) -> Result<Vec<Certificate>, ErrBox> {
  let cert_file = File::open(path)?;
  let reader = &mut BufReader::new(cert_file);

  let certs = certs(reader).map_err(|_| {
    DenoError::new(ErrorKind::Other, "Unable to decode certificate".to_string())
  })?;

  if certs.is_empty() {
    let e = DenoError::new(
      ErrorKind::Other,
      "No certificates found in cert file".to_string(),
    );
    return Err(ErrBox::from(e));
  }

  Ok(certs)
}

fn key_decode_err() -> DenoError {
  DenoError::new(ErrorKind::Other, "Unable to decode key".to_string())
}

fn key_not_found_err() -> DenoError {
  DenoError::new(ErrorKind::Other, "No keys found in key file".to_string())
}

/// Starts with -----BEGIN RSA PRIVATE KEY-----
fn load_rsa_keys(path: &str) -> Result<Vec<PrivateKey>, ErrBox> {
  let key_file = File::open(path)?;
  let reader = &mut BufReader::new(key_file);
  let keys = rsa_private_keys(reader).map_err(|_| key_decode_err())?;
  Ok(keys)
}

/// Starts with -----BEGIN PRIVATE KEY-----
fn load_pkcs8_keys(path: &str) -> Result<Vec<PrivateKey>, ErrBox> {
  let key_file = File::open(path)?;
  let reader = &mut BufReader::new(key_file);
  let keys = pkcs8_private_keys(reader).map_err(|_| key_decode_err())?;
  Ok(keys)
}

fn load_keys(path: &str) -> Result<Vec<PrivateKey>, ErrBox> {
  let path = path.to_string();
  let mut keys = load_rsa_keys(&path)?;

  if keys.is_empty() {
    keys = load_pkcs8_keys(&path)?;
  }

  if keys.is_empty() {
    return Err(ErrBox::from(key_not_found_err()));
  }

  Ok(keys)
}

#[allow(dead_code)]
pub struct TlsListenerResource {
  listener: tokio::net::TcpListener,
  tls_acceptor: TlsAcceptor,
  task: Option<futures::task::Task>,
  local_addr: SocketAddr,
}

impl Resource for TlsListenerResource {}

impl Drop for TlsListenerResource {
  fn drop(&mut self) {
    self.notify_task();
  }
}

impl TlsListenerResource {
  /// Track the current task so future awaiting for connection
  /// can be notified when listener is closed.
  ///
  /// Throws an error if another task is already tracked.
  pub fn track_task(&mut self) -> Result<(), ErrBox> {
    // Currently, we only allow tracking a single accept task for a listener.
    // This might be changed in the future with multiple workers.
    // Caveat: TcpListener by itself also only tracks an accept task at a time.
    // See https://github.com/tokio-rs/tokio/issues/846#issuecomment-454208883
    if self.task.is_some() {
      let e = std::io::Error::new(
        std::io::ErrorKind::Other,
        "Another accept task is ongoing",
      );
      return Err(ErrBox::from(e));
    }

    self.task.replace(futures::task::current());
    Ok(())
  }

  /// Notifies a task when listener is closed so accept future can resolve.
  pub fn notify_task(&mut self) {
    if let Some(task) = self.task.take() {
      task.notify();
    }
  }

  /// Stop tracking a task.
  /// Happens when the task is done and thus no further tracking is needed.
  pub fn untrack_task(&mut self) {
    if self.task.is_some() {
      self.task.take();
    }
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ListenTlsArgs {
  transport: String,
  hostname: String,
  port: u16,
  cert_file: String,
  key_file: String,
}

fn op_listen_tls(
  state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: ListenTlsArgs = serde_json::from_value(args)?;
  assert_eq!(args.transport, "tcp");

  let cert_file = args.cert_file;
  let key_file = args.key_file;

  state.check_net(&args.hostname, args.port)?;
  state.check_read(&cert_file)?;
  state.check_read(&key_file)?;

  let mut config = ServerConfig::new(NoClientAuth::new());
  config
    .set_single_cert(load_certs(&cert_file)?, load_keys(&key_file)?.remove(0))
    .expect("invalid key or certificate");
  let tls_acceptor = TlsAcceptor::from(Arc::new(config));
  let addr = resolve_addr(&args.hostname, args.port).wait()?;
  let listener = TcpListener::bind(&addr)?;
  let local_addr = listener.local_addr()?;
  let local_addr_str = local_addr.to_string();
  let tls_listener_resource = TlsListenerResource {
    listener,
    tls_acceptor,
    task: None,
    local_addr,
  };
  let mut table = resources::lock_resource_table();
  let rid = table.add("tlsListener", Box::new(tls_listener_resource));

  Ok(JsonOp::Sync(json!({
    "rid": rid,
    "localAddr": local_addr_str
  })))
}

#[derive(Debug, PartialEq)]
enum AcceptTlsState {
  Eager,
  Pending,
  Done,
}

/// Simply accepts a TLS connection.
pub fn accept_tls(rid: ResourceId) -> AcceptTls {
  AcceptTls {
    state: AcceptTlsState::Eager,
    rid,
  }
}

/// A future representing state of accepting a TLS connection.
#[derive(Debug)]
pub struct AcceptTls {
  state: AcceptTlsState,
  rid: ResourceId,
}

impl Future for AcceptTls {
  type Item = (TcpStream, SocketAddr);
  type Error = ErrBox;

  fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
    if self.state == AcceptTlsState::Done {
      panic!("poll AcceptTls after it's done");
    }

    let mut table = resources::lock_resource_table();
    let listener_resource = table
      .get_mut::<TlsListenerResource>(self.rid)
      .ok_or_else(|| {
        let e = std::io::Error::new(
          std::io::ErrorKind::Other,
          "Listener has been closed",
        );
        ErrBox::from(e)
      })?;

    let listener = &mut listener_resource.listener;

    if self.state == AcceptTlsState::Eager {
      // Similar to try_ready!, but also track/untrack accept task
      // in TcpListener resource.
      // In this way, when the listener is closed, the task can be
      // notified to error out (instead of stuck forever).
      match listener.poll_accept().map_err(ErrBox::from) {
        Ok(Async::Ready((stream, addr))) => {
          self.state = AcceptTlsState::Done;
          return Ok((stream, addr).into());
        }
        Ok(Async::NotReady) => {
          self.state = AcceptTlsState::Pending;
          return Ok(Async::NotReady);
        }
        Err(e) => {
          self.state = AcceptTlsState::Done;
          return Err(e);
        }
      }
    }

    match listener.poll_accept().map_err(ErrBox::from) {
      Ok(Async::Ready((stream, addr))) => {
        listener_resource.untrack_task();
        self.state = AcceptTlsState::Done;
        Ok((stream, addr).into())
      }
      Ok(Async::NotReady) => {
        listener_resource.track_task()?;
        Ok(Async::NotReady)
      }
      Err(e) => {
        listener_resource.untrack_task();
        self.state = AcceptTlsState::Done;
        Err(e)
      }
    }
  }
}

#[derive(Deserialize)]
struct AcceptTlsArgs {
  rid: i32,
}

fn op_accept_tls(
  _state: &ThreadSafeState,
  args: Value,
  _zero_copy: Option<PinnedBuf>,
) -> Result<JsonOp, ErrBox> {
  let args: AcceptTlsArgs = serde_json::from_value(args)?;
  let rid = args.rid as u32;

  let op = accept_tls(rid)
    .and_then(move |(tcp_stream, _socket_addr)| {
      let local_addr = tcp_stream.local_addr()?;
      let remote_addr = tcp_stream.peer_addr()?;
      Ok((tcp_stream, local_addr, remote_addr))
    })
    .and_then(move |(tcp_stream, local_addr, remote_addr)| {
      let table = resources::lock_resource_table();
      let resource = table
        .get::<TlsListenerResource>(rid)
        .ok_or_else(bad_resource)
        .expect("Can't find tls listener");

      resource
        .tls_acceptor
        .accept(tcp_stream)
        .map_err(ErrBox::from)
        .and_then(move |tls_stream| {
          let rid = resources::add_server_tls_stream(tls_stream);
          Ok((rid, local_addr, remote_addr))
        })
    })
    .and_then(move |(rid, local_addr, remote_addr)| {
      futures::future::ok(json!({
        "rid": rid,
        "localAddr": local_addr.to_string(),
        "remoteAddr": remote_addr.to_string(),
      }))
    });

  Ok(JsonOp::Async(Box::new(op)))
}
