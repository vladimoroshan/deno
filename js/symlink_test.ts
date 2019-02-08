// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { test, testPerm, assert, assertEqual } from "./test_util.ts";
import * as deno from "deno";

testPerm({ read: true, write: true }, function symlinkSyncSuccess() {
  const testDir = deno.makeTempDirSync();
  const oldname = testDir + "/oldname";
  const newname = testDir + "/newname";
  deno.mkdirSync(oldname);
  let errOnWindows;
  // Just for now, until we implement symlink for Windows.
  try {
    deno.symlinkSync(oldname, newname);
  } catch (e) {
    errOnWindows = e;
  }
  if (errOnWindows) {
    assertEqual(errOnWindows.kind, deno.ErrorKind.Other);
    assertEqual(errOnWindows.message, "Not implemented");
  } else {
    const newNameInfoLStat = deno.lstatSync(newname);
    const newNameInfoStat = deno.statSync(newname);
    assert(newNameInfoLStat.isSymlink());
    assert(newNameInfoStat.isDirectory());
  }
});

test(function symlinkSyncPerm() {
  let err;
  try {
    deno.symlinkSync("oldbaddir", "newbaddir");
  } catch (e) {
    err = e;
  }
  assertEqual(err.kind, deno.ErrorKind.PermissionDenied);
  assertEqual(err.name, "PermissionDenied");
});

// Just for now, until we implement symlink for Windows.
testPerm({ write: true }, function symlinkSyncNotImplemented() {
  let err;
  try {
    deno.symlinkSync("oldname", "newname", "dir");
  } catch (e) {
    err = e;
  }
  assertEqual(err.message, "Not implemented");
});

testPerm({ read: true, write: true }, async function symlinkSuccess() {
  const testDir = deno.makeTempDirSync();
  const oldname = testDir + "/oldname";
  const newname = testDir + "/newname";
  deno.mkdirSync(oldname);
  let errOnWindows;
  // Just for now, until we implement symlink for Windows.
  try {
    await deno.symlink(oldname, newname);
  } catch (e) {
    errOnWindows = e;
  }
  if (errOnWindows) {
    assertEqual(errOnWindows.kind, deno.ErrorKind.Other);
    assertEqual(errOnWindows.message, "Not implemented");
  } else {
    const newNameInfoLStat = deno.lstatSync(newname);
    const newNameInfoStat = deno.statSync(newname);
    assert(newNameInfoLStat.isSymlink());
    assert(newNameInfoStat.isDirectory());
  }
});
