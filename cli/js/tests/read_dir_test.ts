// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { unitTest, assert, assertEquals } from "./test_util.ts";

function assertSameContent(files: Deno.DirEntry[]): void {
  let counter = 0;

  for (const entry of files) {
    if (entry.name === "subdir") {
      assert(entry.isDirectory);
      counter++;
    }
  }

  assertEquals(counter, 1);
}

unitTest({ perms: { read: true } }, function readdirSyncSuccess(): void {
  const files = [...Deno.readdirSync("cli/tests/")];
  assertSameContent(files);
});

unitTest({ perms: { read: false } }, function readdirSyncPerm(): void {
  let caughtError = false;
  try {
    Deno.readdirSync("tests/");
  } catch (e) {
    caughtError = true;
    assert(e instanceof Deno.errors.PermissionDenied);
  }
  assert(caughtError);
});

unitTest({ perms: { read: true } }, function readdirSyncNotDir(): void {
  let caughtError = false;
  let src;

  try {
    src = Deno.readdirSync("cli/tests/fixture.json");
  } catch (err) {
    caughtError = true;
    assert(err instanceof Error);
  }
  assert(caughtError);
  assertEquals(src, undefined);
});

unitTest({ perms: { read: true } }, function readdirSyncNotFound(): void {
  let caughtError = false;
  let src;

  try {
    src = Deno.readdirSync("bad_dir_name");
  } catch (err) {
    caughtError = true;
    assert(err instanceof Deno.errors.NotFound);
  }
  assert(caughtError);
  assertEquals(src, undefined);
});

unitTest({ perms: { read: true } }, async function readdirSuccess(): Promise<
  void
> {
  const files = [];
  for await (const dirEntry of Deno.readdir("cli/tests/")) {
    files.push(dirEntry);
  }
  assertSameContent(files);
});

unitTest({ perms: { read: false } }, async function readdirPerm(): Promise<
  void
> {
  let caughtError = false;
  try {
    await Deno.readdir("tests/")[Symbol.asyncIterator]().next();
  } catch (e) {
    caughtError = true;
    assert(e instanceof Deno.errors.PermissionDenied);
  }
  assert(caughtError);
});
