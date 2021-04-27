# Deno runtime tests

Files in this directory are unit tests for Deno runtime.

Testing Deno runtime code requires checking API under different runtime
permissions. To accomplish this all tests exercised are created using
`unitTest()` function.

```ts
import { unitTest } from "./test_util.ts";

unitTest(function simpleTestFn(): void {
  // test code here
});

unitTest(
  {
    ignore: Deno.build.os === "windows",
    perms: { read: true, write: true },
  },
  function complexTestFn(): void {
    // test code here
  },
);
```

## Running tests

There are three ways to run `unit_test_runner.ts`:

```sh
# Run all tests.
target/debug/deno test --allow-all --unstable cli/tests/unit/

# Run a specific test module
target/debug/deno test --allow-all --unstable cli/tests/unit/files_test.ts
```

### Http server

`target/debug/test_server` is required to run when one's running unit tests.
During CI it's spawned automatically, but if you want to run tests manually make
sure that server is spawned otherwise there'll be cascade of test failures.
