// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use deno_core::js_check;
use deno_core::JsRuntime;
use std::path::{Path, PathBuf};

pub fn init(isolate: &mut JsRuntime) {
  let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
  let files = vec![
    manifest_dir.join("00_dom_exception.js"),
    manifest_dir.join("01_event.js"),
    manifest_dir.join("02_abort_signal.js"),
    manifest_dir.join("08_text_encoding.js"),
  ];
  // TODO(nayeemrmn): https://github.com/rust-lang/cargo/issues/3946 to get the
  // workspace root.
  let display_root = manifest_dir.parent().unwrap().parent().unwrap();
  for file in files {
    println!("cargo:rerun-if-changed={}", file.display());
    let display_path = file.strip_prefix(display_root).unwrap();
    let display_path_str = display_path.display().to_string();
    js_check(isolate.execute(
      &("deno:".to_string() + &display_path_str.replace('\\', "/")),
      &std::fs::read_to_string(&file).unwrap(),
    ));
  }
}

pub fn get_declaration() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lib.deno_web.d.ts")
}

#[cfg(test)]
mod tests {
  use deno_core::js_check;
  use deno_core::JsRuntime;
  use deno_core::StartupData;
  use futures::future::lazy;
  use futures::future::FutureExt;
  use futures::task::Context;
  use futures::task::Poll;

  fn run_in_task<F>(f: F)
  where
    F: FnOnce(&mut Context) + Send + 'static,
  {
    futures::executor::block_on(lazy(move |cx| f(cx)));
  }

  fn setup() -> JsRuntime {
    let mut isolate = JsRuntime::new(StartupData::None, false);
    crate::init(&mut isolate);
    isolate
  }

  #[test]
  fn test_abort_controller() {
    run_in_task(|mut cx| {
      let mut isolate = setup();
      js_check(isolate.execute(
        "abort_controller_test.js",
        include_str!("abort_controller_test.js"),
      ));
      if let Poll::Ready(Err(_)) = isolate.poll_unpin(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn test_event() {
    run_in_task(|mut cx| {
      let mut isolate = setup();
      js_check(isolate.execute("event_test.js", include_str!("event_test.js")));
      if let Poll::Ready(Err(_)) = isolate.poll_unpin(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn test_event_error() {
    run_in_task(|mut cx| {
      let mut isolate = setup();
      let result = isolate.execute("foo.js", "new Event()");
      if let Err(error) = result {
        let error_string = error.to_string();
        // Test that the script specifier is a URL: `deno:<repo-relative path>`.
        assert!(error_string.starts_with("deno:op_crates/web/01_event.js"));
        assert!(error_string.contains("Uncaught TypeError"));
      } else {
        unreachable!();
      }
      if let Poll::Ready(Err(_)) = isolate.poll_unpin(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn test_event_target() {
    run_in_task(|mut cx| {
      let mut isolate = setup();
      js_check(
        isolate.execute(
          "event_target_test.js",
          include_str!("event_target_test.js"),
        ),
      );
      if let Poll::Ready(Err(_)) = isolate.poll_unpin(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn test_text_encoding() {
    run_in_task(|mut cx| {
      let mut isolate = setup();
      js_check(isolate.execute(
        "text_encoding_test.js",
        include_str!("text_encoding_test.js"),
      ));
      if let Poll::Ready(Err(_)) = isolate.poll_unpin(&mut cx) {
        unreachable!();
      }
    });
  }
}
