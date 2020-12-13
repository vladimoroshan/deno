// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use super::io::{StreamResource, StreamResourceHolder};
use crate::permissions::Permissions;
use crate::resolve_addr::resolve_addr;
use deno_core::error::bad_resource;
use deno_core::error::bad_resource_id;
use deno_core::error::custom_error;
use deno_core::error::AnyError;
use deno_core::futures;
use deno_core::futures::future::poll_fn;
use deno_core::serde_json;
use deno_core::serde_json::json;
use deno_core::serde_json::Value;
use deno_core::BufVec;
use deno_core::OpState;
use deno_core::ZeroCopyBuf;
use serde::Deserialize;
use std::cell::RefCell;
use std::convert::From;
use std::fs::File;
use std::io::BufReader;
use std::net::SocketAddr;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
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
use webpki::DNSNameRef;

pub fn init(rt: &mut deno_core::JsRuntime) {
  super::reg_json_async(rt, "op_start_tls", op_start_tls);
  super::reg_json_async(rt, "op_connect_tls", op_connect_tls);
  super::reg_json_sync(rt, "op_listen_tls", op_listen_tls);
  super::reg_json_async(rt, "op_accept_tls", op_accept_tls);
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConnectTLSArgs {
  transport: String,
  hostname: String,
  port: u16,
  cert_file: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StartTLSArgs {
  rid: u32,
  cert_file: Option<String>,
  hostname: String,
}

async fn op_start_tls(
  state: Rc<RefCell<OpState>>,
  args: Value,
  _zero_copy: BufVec,
) -> Result<Value, AnyError> {
  let args: StartTLSArgs = serde_json::from_value(args)?;
  let rid = args.rid as u32;
  let cert_file = args.cert_file.clone();

  let mut domain = args.hostname;
  if domain.is_empty() {
    domain.push_str("localhost");
  }
  {
    super::check_unstable2(&state, "Deno.startTls");
    let s = state.borrow();
    let permissions = s.borrow::<Permissions>();
    permissions.check_net(&domain, 0)?;
    if let Some(path) = cert_file.clone() {
      permissions.check_read(Path::new(&path))?;
    }
  }
  let mut resource_holder = {
    let mut state_ = state.borrow_mut();
    match state_.resource_table.remove::<StreamResourceHolder>(rid) {
      Some(resource) => *resource,
      None => return Err(bad_resource_id()),
    }
  };

  if let StreamResource::TcpStream(ref mut tcp_stream) =
    resource_holder.resource
  {
    let tcp_stream = tcp_stream.take().unwrap();
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
    let dnsname =
      DNSNameRef::try_from_ascii_str(&domain).expect("Invalid DNS lookup");
    let tls_stream = tls_connector.connect(dnsname, tcp_stream).await?;

    let rid = {
      let mut state_ = state.borrow_mut();
      state_.resource_table.add(
        "clientTlsStream",
        Box::new(StreamResourceHolder::new(StreamResource::ClientTlsStream(
          Box::new(tls_stream),
        ))),
      )
    };
    Ok(json!({
        "rid": rid,
        "localAddr": {
          "hostname": local_addr.ip().to_string(),
          "port": local_addr.port(),
          "transport": "tcp",
        },
        "remoteAddr": {
          "hostname": remote_addr.ip().to_string(),
          "port": remote_addr.port(),
          "transport": "tcp",
        }
    }))
  } else {
    Err(bad_resource_id())
  }
}

async fn op_connect_tls(
  state: Rc<RefCell<OpState>>,
  args: Value,
  _zero_copy: BufVec,
) -> Result<Value, AnyError> {
  let args: ConnectTLSArgs = serde_json::from_value(args)?;
  let cert_file = args.cert_file.clone();
  {
    let s = state.borrow();
    let permissions = s.borrow::<Permissions>();
    permissions.check_net(&args.hostname, args.port)?;
    if let Some(path) = cert_file.clone() {
      permissions.check_read(Path::new(&path))?;
    }
  }
  let mut domain = args.hostname.clone();
  if domain.is_empty() {
    domain.push_str("localhost");
  }

  let addr = resolve_addr(&args.hostname, args.port)?;
  let tcp_stream = TcpStream::connect(&addr).await?;
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
  let dnsname =
    DNSNameRef::try_from_ascii_str(&domain).expect("Invalid DNS lookup");
  let tls_stream = tls_connector.connect(dnsname, tcp_stream).await?;
  let rid = {
    let mut state_ = state.borrow_mut();
    state_.resource_table.add(
      "clientTlsStream",
      Box::new(StreamResourceHolder::new(StreamResource::ClientTlsStream(
        Box::new(tls_stream),
      ))),
    )
  };
  Ok(json!({
      "rid": rid,
      "localAddr": {
        "hostname": local_addr.ip().to_string(),
        "port": local_addr.port(),
        "transport": args.transport,
      },
      "remoteAddr": {
        "hostname": remote_addr.ip().to_string(),
        "port": remote_addr.port(),
        "transport": args.transport,
      }
  }))
}

fn load_certs(path: &str) -> Result<Vec<Certificate>, AnyError> {
  let cert_file = File::open(path)?;
  let reader = &mut BufReader::new(cert_file);

  let certs = certs(reader)
    .map_err(|_| custom_error("InvalidData", "Unable to decode certificate"))?;

  if certs.is_empty() {
    let e = custom_error("InvalidData", "No certificates found in cert file");
    return Err(e);
  }

  Ok(certs)
}

fn key_decode_err() -> AnyError {
  custom_error("InvalidData", "Unable to decode key")
}

fn key_not_found_err() -> AnyError {
  custom_error("InvalidData", "No keys found in key file")
}

/// Starts with -----BEGIN RSA PRIVATE KEY-----
fn load_rsa_keys(path: &str) -> Result<Vec<PrivateKey>, AnyError> {
  let key_file = File::open(path)?;
  let reader = &mut BufReader::new(key_file);
  let keys = rsa_private_keys(reader).map_err(|_| key_decode_err())?;
  Ok(keys)
}

/// Starts with -----BEGIN PRIVATE KEY-----
fn load_pkcs8_keys(path: &str) -> Result<Vec<PrivateKey>, AnyError> {
  let key_file = File::open(path)?;
  let reader = &mut BufReader::new(key_file);
  let keys = pkcs8_private_keys(reader).map_err(|_| key_decode_err())?;
  Ok(keys)
}

fn load_keys(path: &str) -> Result<Vec<PrivateKey>, AnyError> {
  let path = path.to_string();
  let mut keys = load_rsa_keys(&path)?;

  if keys.is_empty() {
    keys = load_pkcs8_keys(&path)?;
  }

  if keys.is_empty() {
    return Err(key_not_found_err());
  }

  Ok(keys)
}

#[allow(dead_code)]
pub struct TlsListenerResource {
  listener: TcpListener,
  tls_acceptor: TlsAcceptor,
  waker: Option<futures::task::AtomicWaker>,
  local_addr: SocketAddr,
}

impl Drop for TlsListenerResource {
  fn drop(&mut self) {
    self.wake_task();
  }
}

impl TlsListenerResource {
  /// Track the current task so future awaiting for connection
  /// can be notified when listener is closed.
  ///
  /// Throws an error if another task is already tracked.
  pub fn track_task(&mut self, cx: &Context) -> Result<(), AnyError> {
    // Currently, we only allow tracking a single accept task for a listener.
    // This might be changed in the future with multiple workers.
    // Caveat: TcpListener by itself also only tracks an accept task at a time.
    // See https://github.com/tokio-rs/tokio/issues/846#issuecomment-454208883
    if self.waker.is_some() {
      return Err(custom_error("Busy", "Another accept task is ongoing"));
    }

    let waker = futures::task::AtomicWaker::new();
    waker.register(cx.waker());
    self.waker.replace(waker);
    Ok(())
  }

  /// Notifies a task when listener is closed so accept future can resolve.
  pub fn wake_task(&mut self) {
    if let Some(waker) = self.waker.as_ref() {
      waker.wake();
    }
  }

  /// Stop tracking a task.
  /// Happens when the task is done and thus no further tracking is needed.
  pub fn untrack_task(&mut self) {
    self.waker.take();
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
  state: &mut OpState,
  args: Value,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let args: ListenTlsArgs = serde_json::from_value(args)?;
  assert_eq!(args.transport, "tcp");

  let cert_file = args.cert_file;
  let key_file = args.key_file;
  {
    let permissions = state.borrow::<Permissions>();
    permissions.check_net(&args.hostname, args.port)?;
    permissions.check_read(Path::new(&cert_file))?;
    permissions.check_read(Path::new(&key_file))?;
  }
  let mut config = ServerConfig::new(NoClientAuth::new());
  config
    .set_single_cert(load_certs(&cert_file)?, load_keys(&key_file)?.remove(0))
    .expect("invalid key or certificate");
  let tls_acceptor = TlsAcceptor::from(Arc::new(config));
  let addr = resolve_addr(&args.hostname, args.port)?;
  let std_listener = std::net::TcpListener::bind(&addr)?;
  let listener = TcpListener::from_std(std_listener)?;
  let local_addr = listener.local_addr()?;
  let tls_listener_resource = TlsListenerResource {
    listener,
    tls_acceptor,
    waker: None,
    local_addr,
  };

  let rid = state
    .resource_table
    .add("tlsListener", Box::new(tls_listener_resource));

  Ok(json!({
    "rid": rid,
    "localAddr": {
      "hostname": local_addr.ip().to_string(),
      "port": local_addr.port(),
      "transport": args.transport,
    },
  }))
}

#[derive(Deserialize)]
struct AcceptTlsArgs {
  rid: i32,
}

async fn op_accept_tls(
  state: Rc<RefCell<OpState>>,
  args: Value,
  _zero_copy: BufVec,
) -> Result<Value, AnyError> {
  let args: AcceptTlsArgs = serde_json::from_value(args)?;
  let rid = args.rid as u32;
  let accept_fut = poll_fn(|cx| {
    let mut state = state.borrow_mut();
    let listener_resource = state
      .resource_table
      .get_mut::<TlsListenerResource>(rid)
      .ok_or_else(|| bad_resource("Listener has been closed"))?;
    let listener = &mut listener_resource.listener;
    match listener.poll_accept(cx).map_err(AnyError::from) {
      Poll::Ready(Ok((stream, addr))) => {
        listener_resource.untrack_task();
        Poll::Ready(Ok((stream, addr)))
      }
      Poll::Pending => {
        listener_resource.track_task(cx)?;
        Poll::Pending
      }
      Poll::Ready(Err(e)) => {
        listener_resource.untrack_task();
        Poll::Ready(Err(e))
      }
    }
  });
  let (tcp_stream, _socket_addr) = accept_fut.await?;
  let local_addr = tcp_stream.local_addr()?;
  let remote_addr = tcp_stream.peer_addr()?;
  let tls_acceptor = {
    let state_ = state.borrow();
    let resource = state_
      .resource_table
      .get::<TlsListenerResource>(rid)
      .ok_or_else(bad_resource_id)
      .expect("Can't find tls listener");
    resource.tls_acceptor.clone()
  };
  let tls_stream = tls_acceptor.accept(tcp_stream).await?;
  let rid = {
    let mut state_ = state.borrow_mut();
    state_.resource_table.add(
      "serverTlsStream",
      Box::new(StreamResourceHolder::new(StreamResource::ServerTlsStream(
        Box::new(tls_stream),
      ))),
    )
  };
  Ok(json!({
    "rid": rid,
    "localAddr": {
      "transport": "tcp",
      "hostname": local_addr.ip().to_string(),
      "port": local_addr.port()
    },
    "remoteAddr": {
      "transport": "tcp",
      "hostname": remote_addr.ip().to_string(),
      "port": remote_addr.port()
    }
  }))
}
