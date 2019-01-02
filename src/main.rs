// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
extern crate dirs;
extern crate flatbuffers;
extern crate getopts;
extern crate http;
extern crate hyper;
extern crate hyper_rustls;
extern crate libc;
extern crate rand;
extern crate remove_dir_all;
extern crate ring;
extern crate rustyline;
extern crate serde_json;
extern crate source_map_mappings;
extern crate tempfile;
extern crate tokio;
extern crate tokio_executor;
extern crate tokio_fs;
extern crate tokio_io;
extern crate tokio_process;
extern crate tokio_threadpool;
extern crate url;

#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate futures;

pub mod deno_dir;
pub mod errors;
pub mod flags;
mod fs;
mod http_body;
mod http_util;
pub mod isolate;
pub mod js_errors;
pub mod libdeno;
pub mod msg;
pub mod msg_util;
pub mod ops;
pub mod permissions;
mod repl;
pub mod resources;
pub mod snapshot;
mod tokio_util;
mod tokio_write;
pub mod version;

#[cfg(unix)]
mod eager_unix;

use std::env;
use std::sync::Arc;

static LOGGER: Logger = Logger;

struct Logger;

impl log::Log for Logger {
  fn enabled(&self, metadata: &log::Metadata) -> bool {
    metadata.level() <= log::max_level()
  }

  fn log(&self, record: &log::Record) {
    if self.enabled(record.metadata()) {
      println!("{} RS - {}", record.level(), record.args());
    }
  }
  fn flush(&self) {}
}

fn print_err_and_exit(err: js_errors::JSError) {
  eprintln!("{}", err.to_string());
  std::process::exit(1);
}

fn main() {
  log::set_logger(&LOGGER).unwrap();
  let args = env::args().collect();
  let (flags, rest_argv, usage_string) =
    flags::set_flags(args).unwrap_or_else(|err| {
      eprintln!("{}", err);
      std::process::exit(1)
    });

  if flags.help {
    println!("{}", &usage_string);
    std::process::exit(0);
  }

  log::set_max_level(if flags.log_debug {
    log::LevelFilter::Debug
  } else {
    log::LevelFilter::Warn
  });

  let state = Arc::new(isolate::IsolateState::new(flags, rest_argv));
  let snapshot = snapshot::deno_snapshot();
  let isolate = isolate::Isolate::new(snapshot, state, ops::dispatch);
  tokio_util::init(|| {
    isolate
      .execute("denoMain();")
      .unwrap_or_else(print_err_and_exit);
    isolate.event_loop().unwrap_or_else(print_err_and_exit);
  });
}
