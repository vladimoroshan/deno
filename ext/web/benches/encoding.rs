// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use deno_bench_util::bench_js_sync;
use deno_bench_util::bench_or_profile;
use deno_bench_util::bencher::benchmark_group;
use deno_bench_util::bencher::Bencher;
use deno_core::Extension;
use deno_web::BlobStore;

struct Permissions;

impl deno_web::TimersPermission for Permissions {
  fn allow_hrtime(&mut self) -> bool {
    false
  }
  fn check_unstable(
    &self,
    _state: &deno_core::OpState,
    _api_name: &'static str,
  ) {
    unreachable!()
  }
}

fn setup() -> Vec<Extension> {
  vec![
    deno_webidl::init(),
    deno_url::init(),
    deno_console::init(),
    deno_web::init::<Permissions>(BlobStore::default(), None),
    Extension::builder("bench_setup")
      .esm(vec![(
        "internal:setup",
        r#"
        import { TextDecoder } from "internal:ext/web/08_text_encoding.js";
        globalThis.TextDecoder = TextDecoder;
        globalThis.hello12k = Deno.core.encode("hello world\n".repeat(1e3));
        "#,
      )])
      .state(|state| {
        state.put(Permissions {});
        Ok(())
      })
      .build(),
  ]
}

fn bench_encode_12kb(b: &mut Bencher) {
  bench_js_sync(b, r#"new TextDecoder().decode(hello12k);"#, setup);
}

benchmark_group!(benches, bench_encode_12kb);
bench_or_profile!(benches);
