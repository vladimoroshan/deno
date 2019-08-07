/// To run this benchmark:
///
/// > DENO_BUILD_MODE=release ./tools/build.py && \
///   ./target/release/deno_core_http_bench --multi-thread
extern crate deno;
extern crate futures;
extern crate libc;
extern crate tokio;

#[macro_use]
extern crate log;
#[macro_use]
extern crate lazy_static;

use deno::*;
use futures::future::lazy;
use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use tokio::prelude::*;

static LOGGER: Logger = Logger;
struct Logger;
impl log::Log for Logger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= log::max_level()
  }
  fn log(&self, record: &log::Record) {
    if self.enabled(record.metadata()) {
      println!("{} - {}", record.level(), record.args());
    }
  }
  fn flush(&self) {}
}

const OP_LISTEN: OpId = 1;
const OP_ACCEPT: OpId = 2;
const OP_READ: OpId = 3;
const OP_WRITE: OpId = 4;
const OP_CLOSE: OpId = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct Record {
  pub promise_id: i32,
  pub arg: i32,
  pub result: i32,
}

impl Into<Buf> for Record {
  fn into(self) -> Buf {
    let buf32 = vec![self.promise_id, self.arg, self.result].into_boxed_slice();
    let ptr = Box::into_raw(buf32) as *mut [u8; 3 * 4];
    unsafe { Box::from_raw(ptr) }
  }
}

impl From<&[u8]> for Record {
  fn from(s: &[u8]) -> Record {
    #[allow(clippy::cast_ptr_alignment)]
    let ptr = s.as_ptr() as *const i32;
    let ints = unsafe { std::slice::from_raw_parts(ptr, 3) };
    Record {
      promise_id: ints[0],
      arg: ints[1],
      result: ints[2],
    }
  }
}

impl From<Buf> for Record {
  fn from(buf: Buf) -> Record {
    assert_eq!(buf.len(), 3 * 4);
    #[allow(clippy::cast_ptr_alignment)]
    let ptr = Box::into_raw(buf) as *mut [i32; 3];
    let ints: Box<[i32]> = unsafe { Box::from_raw(ptr) };
    assert_eq!(ints.len(), 3);
    Record {
      promise_id: ints[0],
      arg: ints[1],
      result: ints[2],
    }
  }
}

#[test]
fn test_record_from() {
  let r = Record {
    promise_id: 1,
    arg: 3,
    result: 4,
  };
  let expected = r.clone();
  let buf: Buf = r.into();
  #[cfg(target_endian = "little")]
  assert_eq!(
    buf,
    vec![1u8, 0, 0, 0, 3, 0, 0, 0, 4, 0, 0, 0].into_boxed_slice()
  );
  let actual = Record::from(buf);
  assert_eq!(actual, expected);
  // TODO test From<&[u8]> for Record
}

pub type HttpBenchOp = dyn Future<Item = i32, Error = std::io::Error> + Send;

fn dispatch(
  op_id: OpId,
  control: &[u8],
  zero_copy_buf: Option<PinnedBuf>,
) -> CoreOp {
  let record = Record::from(control);
  let is_sync = record.promise_id == 0;
  let http_bench_op = match op_id {
    OP_LISTEN => {
      assert!(is_sync);
      op_listen()
    }
    OP_CLOSE => {
      assert!(is_sync);
      let rid = record.arg;
      op_close(rid)
    }
    OP_ACCEPT => {
      assert!(!is_sync);
      let listener_rid = record.arg;
      op_accept(listener_rid)
    }
    OP_READ => {
      assert!(!is_sync);
      let rid = record.arg;
      op_read(rid, zero_copy_buf)
    }
    OP_WRITE => {
      assert!(!is_sync);
      let rid = record.arg;
      op_write(rid, zero_copy_buf)
    }
    _ => panic!("bad op {}", op_id),
  };
  let mut record_a = record.clone();
  let mut record_b = record.clone();

  let fut = Box::new(
    http_bench_op
      .and_then(move |result| {
        record_a.result = result;
        Ok(record_a)
      })
      .or_else(|err| -> Result<Record, ()> {
        eprintln!("unexpected err {}", err);
        record_b.result = -1;
        Ok(record_b)
      })
      .then(|result| -> Result<Buf, ()> {
        let record = result.unwrap();
        Ok(record.into())
      }),
  );

  if is_sync {
    Op::Sync(fut.wait().unwrap())
  } else {
    Op::Async(fut)
  }
}

fn main() {
  let main_future = lazy(move || {
    // TODO currently isolate.execute() must be run inside tokio, hence the
    // lazy(). It would be nice to not have that contraint. Probably requires
    // using v8::MicrotasksPolicy::kExplicit

    let js_source = include_str!("http_bench.js");

    let startup_data = StartupData::Script(Script {
      source: js_source,
      filename: "http_bench.js",
    });

    let mut isolate = deno::Isolate::new(startup_data, false);
    isolate.set_dispatch(dispatch);

    isolate.then(|r| {
      js_check(r);
      Ok(())
    })
  });

  let args: Vec<String> = env::args().collect();
  // NOTE: `--help` arg will display V8 help and exit
  let args = deno::v8_set_flags(args);

  log::set_logger(&LOGGER).unwrap();
  log::set_max_level(if args.iter().any(|a| a == "-D") {
    log::LevelFilter::Debug
  } else {
    log::LevelFilter::Warn
  });

  if args.iter().any(|a| a == "--multi-thread") {
    println!("multi-thread");
    tokio::run(main_future);
  } else {
    println!("single-thread");
    tokio::runtime::current_thread::run(main_future);
  }
}

enum Repr {
  TcpListener(tokio::net::TcpListener),
  TcpStream(tokio::net::TcpStream),
}

type ResourceTable = HashMap<i32, Repr>;
lazy_static! {
  static ref RESOURCE_TABLE: Mutex<ResourceTable> = Mutex::new(HashMap::new());
  static ref NEXT_RID: AtomicUsize = AtomicUsize::new(3);
}

fn new_rid() -> i32 {
  let rid = NEXT_RID.fetch_add(1, Ordering::SeqCst);
  rid as i32
}

fn op_accept(listener_rid: i32) -> Box<HttpBenchOp> {
  debug!("accept {}", listener_rid);
  Box::new(
    futures::future::poll_fn(move || {
      let mut table = RESOURCE_TABLE.lock().unwrap();
      let maybe_repr = table.get_mut(&listener_rid);
      match maybe_repr {
        Some(Repr::TcpListener(ref mut listener)) => listener.poll_accept(),
        _ => panic!("bad rid {}", listener_rid),
      }
    })
    .and_then(move |(stream, addr)| {
      debug!("accept success {}", addr);
      let rid = new_rid();

      let mut guard = RESOURCE_TABLE.lock().unwrap();
      guard.insert(rid, Repr::TcpStream(stream));

      Ok(rid as i32)
    }),
  )
}

fn op_listen() -> Box<HttpBenchOp> {
  debug!("listen");

  Box::new(lazy(move || {
    let addr = "127.0.0.1:4544".parse::<SocketAddr>().unwrap();
    let listener = tokio::net::TcpListener::bind(&addr).unwrap();
    let rid = new_rid();

    let mut guard = RESOURCE_TABLE.lock().unwrap();
    guard.insert(rid, Repr::TcpListener(listener));
    futures::future::ok(rid)
  }))
}

fn op_close(rid: i32) -> Box<HttpBenchOp> {
  debug!("close");
  Box::new(lazy(move || {
    let mut table = RESOURCE_TABLE.lock().unwrap();
    let r = table.remove(&rid);
    let result = if r.is_some() { 0 } else { -1 };
    futures::future::ok(result)
  }))
}

fn op_read(rid: i32, zero_copy_buf: Option<PinnedBuf>) -> Box<HttpBenchOp> {
  debug!("read rid={}", rid);
  let mut zero_copy_buf = zero_copy_buf.unwrap();
  Box::new(
    futures::future::poll_fn(move || {
      let mut table = RESOURCE_TABLE.lock().unwrap();
      let maybe_repr = table.get_mut(&rid);
      match maybe_repr {
        Some(Repr::TcpStream(ref mut stream)) => {
          stream.poll_read(&mut zero_copy_buf)
        }
        _ => panic!("bad rid"),
      }
    })
    .and_then(move |nread| {
      debug!("read success {}", nread);
      Ok(nread as i32)
    }),
  )
}

fn op_write(rid: i32, zero_copy_buf: Option<PinnedBuf>) -> Box<HttpBenchOp> {
  debug!("write rid={}", rid);
  let zero_copy_buf = zero_copy_buf.unwrap();
  Box::new(
    futures::future::poll_fn(move || {
      let mut table = RESOURCE_TABLE.lock().unwrap();
      let maybe_repr = table.get_mut(&rid);
      match maybe_repr {
        Some(Repr::TcpStream(ref mut stream)) => {
          stream.poll_write(&zero_copy_buf)
        }
        _ => panic!("bad rid"),
      }
    })
    .and_then(move |nwritten| {
      debug!("write success {}", nwritten);
      Ok(nwritten as i32)
    }),
  )
}

fn js_check(r: Result<(), ErrBox>) {
  if let Err(e) = r {
    panic!(e.to_string());
  }
}
