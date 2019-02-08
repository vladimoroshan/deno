// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { testPerm, assert, assertEqual } from "./test_util.ts";
import * as deno from "deno";

testPerm({ write: true, read: true }, function readlinkSyncSuccess() {
  const testDir = deno.makeTempDirSync();
  const target = testDir + "/target";
  const symlink = testDir + "/symln";
  deno.mkdirSync(target);
  // TODO Add test for Windows once symlink is implemented for Windows.
  // See https://github.com/denoland/deno/issues/815.
  if (deno.platform.os !== "win") {
    deno.symlinkSync(target, symlink);
    const targetPath = deno.readlinkSync(symlink);
    assertEqual(targetPath, target);
  }
});

testPerm({ read: false }, async function readlinkSyncPerm() {
  let caughtError = false;
  try {
    deno.readlinkSync("/symlink");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.PermissionDenied);
    assertEqual(e.name, "PermissionDenied");
  }
  assert(caughtError);
});

testPerm({ read: true }, function readlinkSyncNotFound() {
  let caughtError = false;
  let data;
  try {
    data = deno.readlinkSync("bad_filename");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.NotFound);
  }
  assert(caughtError);
  assertEqual(data, undefined);
});

testPerm({ write: true, read: true }, async function readlinkSuccess() {
  const testDir = deno.makeTempDirSync();
  const target = testDir + "/target";
  const symlink = testDir + "/symln";
  deno.mkdirSync(target);
  // TODO Add test for Windows once symlink is implemented for Windows.
  // See https://github.com/denoland/deno/issues/815.
  if (deno.platform.os !== "win") {
    deno.symlinkSync(target, symlink);
    const targetPath = await deno.readlink(symlink);
    assertEqual(targetPath, target);
  }
});

testPerm({ read: false }, async function readlinkPerm() {
  let caughtError = false;
  try {
    await deno.readlink("/symlink");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.PermissionDenied);
    assertEqual(e.name, "PermissionDenied");
  }
  assert(caughtError);
});
