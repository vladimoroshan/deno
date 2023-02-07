// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use deno_bench_util::bench_js_sync;
use deno_bench_util::bench_or_profile;
use deno_bench_util::bencher::benchmark_group;
use deno_bench_util::bencher::Bencher;

use deno_core::Extension;
use deno_core::ExtensionFileSource;

fn setup() -> Vec<Extension> {
  vec![
    deno_webidl::init(),
    deno_url::init(),
    Extension::builder("bench_setup")
      .esm(vec![ExtensionFileSource {
        specifier: "internal:setup".to_string(),
        code: r#"import { URL } from "internal:deno_url/00_url.js";
        globalThis.URL = URL;
        "#,
      }])
      .build(),
  ]
}

fn bench_url_parse(b: &mut Bencher) {
  bench_js_sync(b, r#"new URL(`http://www.google.com/`);"#, setup);
}

benchmark_group!(benches, bench_url_parse,);
bench_or_profile!(benches);
