// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

itest!(_036_import_map_fetch {
  args:
    "cache --quiet --reload --import-map=import_maps/import_map.json import_maps/test.ts",
    output: "cache/036_import_map_fetch.out",
  });

itest!(_037_fetch_multiple {
  args: "cache --reload --check=all run/fetch/test.ts run/fetch/other.ts",
  http_server: true,
  output: "cache/037_fetch_multiple.out",
});

itest!(_095_cache_with_bare_import {
  args: "cache cache/095_cache_with_bare_import.ts",
  output: "cache/095_cache_with_bare_import.ts.out",
  exit_code: 1,
});

itest!(cache_extensionless {
  args: "cache --reload --check=all http://localhost:4545/subdir/no_js_ext",
  output: "cache/cache_extensionless.out",
  http_server: true,
});

itest!(cache_random_extension {
  args:
    "cache --reload --check=all http://localhost:4545/subdir/no_js_ext@1.0.0",
  output: "cache/cache_random_extension.out",
  http_server: true,
});

itest!(performance_stats {
  args: "cache --reload --check=all --log-level debug run/002_hello.ts",
  output: "cache/performance_stats.out",
});

itest!(redirect_cache {
  http_server: true,
  args:
    "cache --reload --check=all http://localhost:4548/subdir/redirects/a.ts",
  output: "cache/redirect_cache.out",
});

itest!(ignore_require {
  args: "cache --reload --no-check cache/ignore_require.js",
  output_str: Some(""),
  exit_code: 0,
});

// This test only runs on linux, because it hardcodes the XDG_CACHE_HOME env var
// which is only used on linux.
#[cfg(target_os = "linux")]
#[test]
fn relative_home_dir() {
  use test_util as util;
  use test_util::TempDir;

  let deno_dir = TempDir::new_in(&util::testdata_path());
  let path = deno_dir.path().strip_prefix(util::testdata_path()).unwrap();

  let mut deno_cmd = util::deno_cmd();
  let output = deno_cmd
    .current_dir(util::testdata_path())
    .env("XDG_CACHE_HOME", path)
    .env_remove("HOME")
    .env_remove("DENO_DIR")
    .arg("cache")
    .arg("--reload")
    .arg("--no-check")
    .arg("run/002_hello.ts")
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap()
    .wait_with_output()
    .unwrap();
  assert!(output.status.success());
  assert_eq!(output.stdout, b"");
}

itest!(check_local_by_default {
  args: "cache --quiet cache/check_local_by_default.ts",
  output: "cache/check_local_by_default.out",
  http_server: true,
});

itest!(check_local_by_default2 {
  args: "cache --quiet cache/check_local_by_default2.ts",
  output: "cache/check_local_by_default2.out",
  http_server: true,
});

itest!(json_import {
  // should not error
  args: "cache --quiet cache/json_import/main.ts",
});
