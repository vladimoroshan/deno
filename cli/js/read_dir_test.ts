// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { unitTest, assert, assertEquals } from "./test_util.ts";

type FileInfo = Deno.FileInfo;

function assertSameContent(files: FileInfo[]): void {
  let counter = 0;

  for (const file of files) {
    if (file.name === "subdir") {
      assert(file.isDirectory());
      counter++;
    }

    if (file.name === "002_hello.ts") {
      assertEquals(file.mode!, Deno.statSync(`cli/tests/${file.name}`).mode!);
      counter++;
    }
  }

  assertEquals(counter, 2);
}

unitTest({ perms: { read: true } }, function readDirSyncSuccess(): void {
  const files = Deno.readDirSync("cli/tests/");
  assertSameContent(files);
});

unitTest({ perms: { read: false } }, function readDirSyncPerm(): void {
  let caughtError = false;
  try {
    Deno.readDirSync("tests/");
  } catch (e) {
    caughtError = true;
    assert(e instanceof Deno.errors.PermissionDenied);
  }
  assert(caughtError);
});

unitTest({ perms: { read: true } }, function readDirSyncNotDir(): void {
  let caughtError = false;
  let src;

  try {
    src = Deno.readDirSync("cli/tests/fixture.json");
  } catch (err) {
    caughtError = true;
    assert(err instanceof Error);
  }
  assert(caughtError);
  assertEquals(src, undefined);
});

unitTest({ perms: { read: true } }, function readDirSyncNotFound(): void {
  let caughtError = false;
  let src;

  try {
    src = Deno.readDirSync("bad_dir_name");
  } catch (err) {
    caughtError = true;
    assert(err instanceof Deno.errors.NotFound);
  }
  assert(caughtError);
  assertEquals(src, undefined);
});

unitTest({ perms: { read: true } }, async function readDirSuccess(): Promise<
  void
> {
  const files = await Deno.readDir("cli/tests/");
  assertSameContent(files);
});

unitTest({ perms: { read: false } }, async function readDirPerm(): Promise<
  void
> {
  let caughtError = false;
  try {
    await Deno.readDir("tests/");
  } catch (e) {
    caughtError = true;
    assert(e instanceof Deno.errors.PermissionDenied);
  }
  assert(caughtError);
});
