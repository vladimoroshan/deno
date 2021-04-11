// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use deno_core::plugin_api::Interface;
use deno_core::plugin_api::Op;
use deno_core::plugin_api::OpResponse;
use deno_core::plugin_api::ZeroCopyBuf;
use futures::future::FutureExt;

#[no_mangle]
pub fn deno_plugin_init(interface: &mut dyn Interface) {
  interface.register_op("testSync", op_test_sync);
  interface.register_op("testAsync", op_test_async);
}

fn op_test_sync(
  _interface: &mut dyn Interface,
  zero_copy: Option<ZeroCopyBuf>,
) -> Op {
  if zero_copy.is_some() {
    println!("Hello from plugin.");
  }
  if let Some(buf) = zero_copy {
    let buf_str = std::str::from_utf8(&buf[..]).unwrap();
    println!("zero_copy: {}", buf_str);
  }
  let result = b"test";
  let result_box: Box<[u8]> = Box::new(*result);
  Op::Sync(OpResponse::Buffer(result_box))
}

fn op_test_async(
  _interface: &mut dyn Interface,
  zero_copy: Option<ZeroCopyBuf>,
) -> Op {
  if zero_copy.is_some() {
    println!("Hello from plugin.");
  }
  let fut = async move {
    if let Some(buf) = zero_copy {
      let buf_str = std::str::from_utf8(&buf[..]).unwrap();
      println!("zero_copy: {}", buf_str);
    }
    let (tx, rx) = futures::channel::oneshot::channel::<Result<(), ()>>();
    std::thread::spawn(move || {
      std::thread::sleep(std::time::Duration::from_secs(1));
      tx.send(Ok(())).unwrap();
    });
    assert!(rx.await.is_ok());
    let result = b"test";
    let result_box: Box<[u8]> = Box::new(*result);
    (0, OpResponse::Buffer(result_box))
  };

  Op::Async(fut.boxed())
}
