import { assertStringIncludes, unitTest, unreachable } from "./test_util.ts";

unitTest(async function sendAsyncStackTrace() {
  const buf = new Uint8Array(10);
  const rid = 10;
  try {
    await Deno.read(rid, buf);
    unreachable();
  } catch (error) {
    const s = error.stack.toString();
    console.log(s);
    assertStringIncludes(s, "dispatch_bin_test.ts");
    assertStringIncludes(s, "read");
  }
});

declare global {
  // deno-lint-ignore no-namespace
  namespace Deno {
    // deno-lint-ignore no-explicit-any
    var core: any; // eslint-disable-line no-var
  }
}

unitTest(async function binOpsAsyncBadResource(): Promise<void> {
  try {
    const nonExistingRid = 9999;
    await Deno.core.binOpAsync(
      "op_read_async",
      nonExistingRid,
      new Uint8Array(0),
    );
  } catch (e) {
    if (!(e instanceof Deno.errors.BadResource)) {
      throw e;
    }
  }
});

unitTest(function binOpsSyncBadResource(): void {
  try {
    const nonExistingRid = 9999;
    Deno.core.binOpSync("op_read_sync", nonExistingRid, new Uint8Array(0));
  } catch (e) {
    if (!(e instanceof Deno.errors.BadResource)) {
      throw e;
    }
  }
});
