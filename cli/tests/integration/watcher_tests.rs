// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use flaky_test::flaky_test;
use std::fs::write;
use std::io::BufRead;
use tempfile::TempDir;
use test_util as util;

macro_rules! assert_contains {
  ($string:expr, $($test:expr),+) => {
    let string = $string; // This might be a function call or something
    if !($(string.contains($test))||+) {
      panic!("{:?} does not contain any of {:?}", string, [$($test),+]);
    }
  }
}

// Helper function to skip watcher output that contains "Restarting"
// phrase.
fn skip_restarting_line(
  mut stderr_lines: impl Iterator<Item = String>,
) -> String {
  loop {
    let msg = stderr_lines.next().unwrap();
    if !msg.contains("Restarting") {
      return msg;
    }
  }
}

fn wait_for(s: &str, lines: &mut impl Iterator<Item = String>) {
  loop {
    let msg = lines.next().unwrap();
    if msg.contains(s) {
      break;
    }
  }
}

fn check_alive_then_kill(mut child: std::process::Child) {
  assert!(child.try_wait().unwrap().is_none());
  child.kill().unwrap();
}

fn child_lines(
  child: &mut std::process::Child,
) -> (impl Iterator<Item = String>, impl Iterator<Item = String>) {
  let stdout_lines = std::io::BufReader::new(child.stdout.take().unwrap())
    .lines()
    .map(|r| r.unwrap());
  let stderr_lines = std::io::BufReader::new(child.stderr.take().unwrap())
    .lines()
    .map(|r| r.unwrap());
  (stdout_lines, stderr_lines)
}

#[test]
fn fmt_watch_test() {
  let t = TempDir::new().unwrap();
  let fixed = util::testdata_path().join("badly_formatted_fixed.js");
  let badly_formatted_original =
    util::testdata_path().join("badly_formatted.mjs");
  let badly_formatted = t.path().join("badly_formatted.js");
  std::fs::copy(&badly_formatted_original, &badly_formatted).unwrap();

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("fmt")
    .arg(&badly_formatted)
    .arg("--watch")
    .arg("--unstable")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (_stdout_lines, stderr_lines) = child_lines(&mut child);

  // TODO(lucacasonato): remove this timeout. It seems to be needed on Linux.
  std::thread::sleep(std::time::Duration::from_secs(1));

  assert!(skip_restarting_line(stderr_lines).contains("badly_formatted.js"));

  let expected = std::fs::read_to_string(fixed.clone()).unwrap();
  let actual = std::fs::read_to_string(badly_formatted.clone()).unwrap();
  assert_eq!(expected, actual);

  // Change content of the file again to be badly formatted
  std::fs::copy(&badly_formatted_original, &badly_formatted).unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));

  // Check if file has been automatically formatted by watcher
  let expected = std::fs::read_to_string(fixed).unwrap();
  let actual = std::fs::read_to_string(badly_formatted).unwrap();
  assert_eq!(expected, actual);
  check_alive_then_kill(child);
}

#[test]
fn bundle_js_watch() {
  use std::path::PathBuf;
  // Test strategy extends this of test bundle_js by adding watcher
  let t = TempDir::new().unwrap();
  let file_to_watch = t.path().join("file_to_watch.js");
  write(&file_to_watch, "console.log('Hello world');").unwrap();
  assert!(file_to_watch.is_file());
  let t = TempDir::new().unwrap();
  let bundle = t.path().join("mod6.bundle.js");
  let mut deno = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("bundle")
    .arg(&file_to_watch)
    .arg(&bundle)
    .arg("--watch")
    .arg("--unstable")
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();

  let (_stdout_lines, mut stderr_lines) = child_lines(&mut deno);

  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "file_to_watch.js");
  assert_contains!(stderr_lines.next().unwrap(), "mod6.bundle.js");
  let file = PathBuf::from(&bundle);
  assert!(file.is_file());
  wait_for("Bundle finished", &mut stderr_lines);

  write(&file_to_watch, "console.log('Hello world2');").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "File change detected!");
  assert_contains!(stderr_lines.next().unwrap(), "file_to_watch.js");
  assert_contains!(stderr_lines.next().unwrap(), "mod6.bundle.js");
  let file = PathBuf::from(&bundle);
  assert!(file.is_file());
  wait_for("Bundle finished", &mut stderr_lines);

  // Confirm that the watcher keeps on working even if the file is updated and has invalid syntax
  write(&file_to_watch, "syntax error ^^").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "File change detected!");
  assert_contains!(stderr_lines.next().unwrap(), "error: ");
  wait_for("Bundle failed", &mut stderr_lines);
  check_alive_then_kill(deno);
}

/// Confirm that the watcher continues to work even if module resolution fails at the *first* attempt
#[test]
fn bundle_watch_not_exit() {
  let t = TempDir::new().unwrap();
  let file_to_watch = t.path().join("file_to_watch.js");
  write(&file_to_watch, "syntax error ^^").unwrap();
  let target_file = t.path().join("target.js");

  let mut deno = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("bundle")
    .arg(&file_to_watch)
    .arg(&target_file)
    .arg("--watch")
    .arg("--unstable")
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (_stdout_lines, mut stderr_lines) = child_lines(&mut deno);

  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "error:");
  assert_contains!(stderr_lines.next().unwrap(), "Bundle failed");
  // the target file hasn't been created yet
  assert!(!target_file.is_file());

  // Make sure the watcher actually restarts and works fine with the proper syntax
  write(&file_to_watch, "console.log(42);").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "File change detected!");
  assert_contains!(stderr_lines.next().unwrap(), "file_to_watch.js");
  assert_contains!(stderr_lines.next().unwrap(), "target.js");
  wait_for("Bundle finished", &mut stderr_lines);
  // bundled file is created
  assert!(target_file.is_file());
  check_alive_then_kill(deno);
}

#[flaky_test::flaky_test]
fn run_watch() {
  let t = TempDir::new().unwrap();
  let file_to_watch = t.path().join("file_to_watch.js");
  write(&file_to_watch, "console.log('Hello world');").unwrap();

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("run")
    .arg("--watch")
    .arg("--unstable")
    .arg(&file_to_watch)
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (mut stdout_lines, mut stderr_lines) = child_lines(&mut child);

  assert_contains!(stdout_lines.next().unwrap(), "Hello world");
  wait_for("Process finished", &mut stderr_lines);

  // TODO(lucacasonato): remove this timeout. It seems to be needed on Linux.
  std::thread::sleep(std::time::Duration::from_secs(1));

  // Change content of the file
  write(&file_to_watch, "console.log('Hello world2');").unwrap();
  // Events from the file watcher is "debounced", so we need to wait for the next execution to start
  std::thread::sleep(std::time::Duration::from_secs(1));

  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "Hello world2");
  wait_for("Process finished", &mut stderr_lines);

  // Add dependency
  let another_file = t.path().join("another_file.js");
  write(&another_file, "export const foo = 0;").unwrap();
  write(
    &file_to_watch,
    "import { foo } from './another_file.js'; console.log(foo);",
  )
  .unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), '0');
  wait_for("Process finished", &mut stderr_lines);

  // Confirm that restarting occurs when a new file is updated
  write(&another_file, "export const foo = 42;").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "42");
  wait_for("Process finished", &mut stderr_lines);

  // Confirm that the watcher keeps on working even if the file is updated and has invalid syntax
  write(&file_to_watch, "syntax error ^^").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stderr_lines.next().unwrap(), "error:");
  wait_for("Process failed", &mut stderr_lines);

  // Then restore the file
  write(
    &file_to_watch,
    "import { foo } from './another_file.js'; console.log(foo);",
  )
  .unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "42");
  wait_for("Process finished", &mut stderr_lines);

  // Update the content of the imported file with invalid syntax
  write(&another_file, "syntax error ^^").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stderr_lines.next().unwrap(), "error:");
  wait_for("Process failed", &mut stderr_lines);

  // Modify the imported file and make sure that restarting occurs
  write(&another_file, "export const foo = 'modified!';").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "modified!");
  wait_for("Process finished", &mut stderr_lines);
  check_alive_then_kill(child);
}

#[test]
fn run_watch_load_unload_events() {
  let t = TempDir::new().unwrap();
  let file_to_watch = t.path().join("file_to_watch.js");
  write(
    &file_to_watch,
    r#"
      setInterval(() => {}, 0);
      window.addEventListener("load", () => {
        console.log("load");
      });

      window.addEventListener("unload", () => {
        console.log("unload");
      });
    "#,
  )
  .unwrap();

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("run")
    .arg("--watch")
    .arg("--unstable")
    .arg(&file_to_watch)
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (mut stdout_lines, mut stderr_lines) = child_lines(&mut child);

  // Wait for the first load event to fire
  assert_contains!(stdout_lines.next().unwrap(), "load");

  // Change content of the file, this time without an interval to keep it alive.
  write(
    &file_to_watch,
    r#"
      window.addEventListener("load", () => {
        console.log("load");
      });

      window.addEventListener("unload", () => {
        console.log("unload");
      });
    "#,
  )
  .unwrap();

  // Events from the file watcher is "debounced", so we need to wait for the next execution to start
  std::thread::sleep(std::time::Duration::from_secs(1));

  // Wait for the restart
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");

  // Confirm that the unload event was dispatched from the first run
  assert_contains!(stdout_lines.next().unwrap(), "unload");

  // Followed by the load event of the second run
  assert_contains!(stdout_lines.next().unwrap(), "load");

  // Which is then unloaded as there is nothing keeping it alive.
  assert_contains!(stdout_lines.next().unwrap(), "unload");
  check_alive_then_kill(child);
}

/// Confirm that the watcher continues to work even if module resolution fails at the *first* attempt
#[test]
fn run_watch_not_exit() {
  let t = TempDir::new().unwrap();
  let file_to_watch = t.path().join("file_to_watch.js");
  write(&file_to_watch, "syntax error ^^").unwrap();

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("run")
    .arg("--watch")
    .arg("--unstable")
    .arg(&file_to_watch)
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (mut stdout_lines, mut stderr_lines) = child_lines(&mut child);

  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "error:");
  assert_contains!(stderr_lines.next().unwrap(), "Process failed");

  // Make sure the watcher actually restarts and works fine with the proper syntax
  write(&file_to_watch, "console.log(42);").unwrap();
  std::thread::sleep(std::time::Duration::from_secs(1));
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "42");
  wait_for("Process finished", &mut stderr_lines);
  check_alive_then_kill(child);
}

#[test]
fn run_watch_with_import_map_and_relative_paths() {
  fn create_relative_tmp_file(
    directory: &TempDir,
    filename: &'static str,
    filecontent: &'static str,
  ) -> std::path::PathBuf {
    let absolute_path = directory.path().join(filename);
    write(&absolute_path, filecontent).unwrap();
    let relative_path = absolute_path
      .strip_prefix(util::testdata_path())
      .unwrap()
      .to_owned();
    assert!(relative_path.is_relative());
    relative_path
  }
  let temp_directory = TempDir::new_in(util::testdata_path()).unwrap();
  let file_to_watch = create_relative_tmp_file(
    &temp_directory,
    "file_to_watch.js",
    "console.log('Hello world');",
  );
  let import_map_path = create_relative_tmp_file(
    &temp_directory,
    "import_map.json",
    "{\"imports\": {}}",
  );

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("run")
    .arg("--unstable")
    .arg("--watch")
    .arg("--import-map")
    .arg(&import_map_path)
    .arg(&file_to_watch)
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (mut stdout_lines, mut stderr_lines) = child_lines(&mut child);

  assert_contains!(stderr_lines.next().unwrap(), "Process finished");
  assert_contains!(stdout_lines.next().unwrap(), "Hello world");

  check_alive_then_kill(child);
}

#[flaky_test]
fn test_watch() {
  let t = TempDir::new().unwrap();

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("test")
    .arg("--watch")
    .arg("--unstable")
    .arg("--no-check")
    .arg(&t.path())
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (mut stdout_lines, mut stderr_lines) = child_lines(&mut child);

  assert_eq!(stdout_lines.next().unwrap(), "");
  assert_contains!(
    stdout_lines.next().unwrap(),
    "0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
  );
  wait_for("Test finished", &mut stderr_lines);

  let foo_file = t.path().join("foo.js");
  let bar_file = t.path().join("bar.js");
  let foo_test = t.path().join("foo_test.js");
  let bar_test = t.path().join("bar_test.js");
  write(&foo_file, "export default function foo() { 1 + 1 }").unwrap();
  write(&bar_file, "export default function bar() { 2 + 2 }").unwrap();
  write(
    &foo_test,
    "import foo from './foo.js'; Deno.test('foo', foo);",
  )
  .unwrap();
  write(
    &bar_test,
    "import bar from './bar.js'; Deno.test('bar', bar);",
  )
  .unwrap();

  assert_eq!(stdout_lines.next().unwrap(), "");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "foo", "bar");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "foo", "bar");
  stdout_lines.next();
  stdout_lines.next();
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Change content of the file
  write(
    &foo_test,
    "import foo from './foo.js'; Deno.test('foobar', foo);",
  )
  .unwrap();

  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "foobar");
  stdout_lines.next();
  stdout_lines.next();
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Add test
  let another_test = t.path().join("new_test.js");
  write(&another_test, "Deno.test('another one', () => 3 + 3)").unwrap();
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "another one");
  stdout_lines.next();
  stdout_lines.next();
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Confirm that restarting occurs when a new file is updated
  write(&another_test, "Deno.test('another one', () => 3 + 3); Deno.test('another another one', () => 4 + 4)")
    .unwrap();
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "running 2 tests");
  assert_contains!(stdout_lines.next().unwrap(), "another one");
  assert_contains!(stdout_lines.next().unwrap(), "another another one");
  stdout_lines.next();
  stdout_lines.next();
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Confirm that the watcher keeps on working even if the file is updated and has invalid syntax
  write(&another_test, "syntax error ^^").unwrap();
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stderr_lines.next().unwrap(), "error:");
  assert_contains!(stderr_lines.next().unwrap(), "Test failed");

  // Then restore the file
  write(&another_test, "Deno.test('another one', () => 3 + 3)").unwrap();
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "another one");
  stdout_lines.next();
  stdout_lines.next();
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Confirm that the watcher keeps on working even if the file is updated and the test fails
  // This also confirms that it restarts when dependencies change
  write(
    &foo_file,
    "export default function foo() { throw new Error('Whoops!'); }",
  )
  .unwrap();
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "FAILED");
  wait_for("test result", &mut stdout_lines);
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Then restore the file
  write(&foo_file, "export default function foo() { 1 + 1 }").unwrap();
  assert_contains!(stderr_lines.next().unwrap(), "Restarting");
  assert_contains!(stdout_lines.next().unwrap(), "running 1 test");
  assert_contains!(stdout_lines.next().unwrap(), "foo");
  stdout_lines.next();
  stdout_lines.next();
  stdout_lines.next();
  wait_for("Test finished", &mut stderr_lines);

  // Test that circular dependencies work fine
  write(
    &foo_file,
    "import './bar.js'; export default function foo() { 1 + 1 }",
  )
  .unwrap();
  write(
    &bar_file,
    "import './foo.js'; export default function bar() { 2 + 2 }",
  )
  .unwrap();
  check_alive_then_kill(child);
}

#[flaky_test]
fn test_watch_doc() {
  let t = TempDir::new().unwrap();

  let mut child = util::deno_cmd()
    .current_dir(util::testdata_path())
    .arg("test")
    .arg("--watch")
    .arg("--doc")
    .arg("--unstable")
    .arg(&t.path())
    .env("NO_COLOR", "1")
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let (mut stdout_lines, mut stderr_lines) = child_lines(&mut child);

  assert_eq!(stdout_lines.next().unwrap(), "");
  assert_contains!(
    stdout_lines.next().unwrap(),
    "0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out"
  );
  wait_for("Test finished", &mut stderr_lines);

  let foo_file = t.path().join("foo.ts");
  write(
    &foo_file,
    r#"
    export default function foo() {}
  "#,
  )
  .unwrap();

  write(
    &foo_file,
    r#"
    /**
     * ```ts
     * import foo from "./foo.ts";
     * ```
     */
    export default function foo() {}
  "#,
  )
  .unwrap();

  // We only need to scan for a Check file://.../foo.ts$3-6 line that
  // corresponds to the documentation block being type-checked.
  assert_contains!(skip_restarting_line(stderr_lines), "foo.ts$3-6");
  check_alive_then_kill(child);
}
