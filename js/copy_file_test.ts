// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { testPerm, assert, assertEqual } from "./test_util.ts";
import * as deno from "deno";

function readFileString(filename: string): string {
  const dataRead = deno.readFileSync(filename);
  const dec = new TextDecoder("utf-8");
  return dec.decode(dataRead);
}

function writeFileString(filename: string, s: string) {
  const enc = new TextEncoder();
  const data = enc.encode(s);
  deno.writeFileSync(filename, data, { perm: 0o666 });
}

function assertSameContent(filename1: string, filename2: string) {
  const data1 = deno.readFileSync(filename1);
  const data2 = deno.readFileSync(filename2);
  assertEqual(data1, data2);
}

testPerm({ read: true, write: true }, function copyFileSyncSuccess() {
  const tempDir = deno.makeTempDirSync();
  const fromFilename = tempDir + "/from.txt";
  const toFilename = tempDir + "/to.txt";
  writeFileString(fromFilename, "Hello world!");
  deno.copyFileSync(fromFilename, toFilename);
  // No change to original file
  assertEqual(readFileString(fromFilename), "Hello world!");
  // Original == Dest
  assertSameContent(fromFilename, toFilename);
});

testPerm({ write: true, read: true }, function copyFileSyncFailure() {
  const tempDir = deno.makeTempDirSync();
  const fromFilename = tempDir + "/from.txt";
  const toFilename = tempDir + "/to.txt";
  // We skip initial writing here, from.txt does not exist
  let err;
  try {
    deno.copyFileSync(fromFilename, toFilename);
  } catch (e) {
    err = e;
  }
  assert(!!err);
  assertEqual(err.kind, deno.ErrorKind.NotFound);
  assertEqual(err.name, "NotFound");
});

testPerm({ write: true, read: false }, function copyFileSyncPerm1() {
  let caughtError = false;
  try {
    deno.copyFileSync("/from.txt", "/to.txt");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.PermissionDenied);
    assertEqual(e.name, "PermissionDenied");
  }
  assert(caughtError);
});

testPerm({ write: false, read: true }, function copyFileSyncPerm2() {
  let caughtError = false;
  try {
    deno.copyFileSync("/from.txt", "/to.txt");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.PermissionDenied);
    assertEqual(e.name, "PermissionDenied");
  }
  assert(caughtError);
});

testPerm({ read: true, write: true }, function copyFileSyncOverwrite() {
  const tempDir = deno.makeTempDirSync();
  const fromFilename = tempDir + "/from.txt";
  const toFilename = tempDir + "/to.txt";
  writeFileString(fromFilename, "Hello world!");
  // Make Dest exist and have different content
  writeFileString(toFilename, "Goodbye!");
  deno.copyFileSync(fromFilename, toFilename);
  // No change to original file
  assertEqual(readFileString(fromFilename), "Hello world!");
  // Original == Dest
  assertSameContent(fromFilename, toFilename);
});

testPerm({ read: true, write: true }, async function copyFileSuccess() {
  const tempDir = deno.makeTempDirSync();
  const fromFilename = tempDir + "/from.txt";
  const toFilename = tempDir + "/to.txt";
  writeFileString(fromFilename, "Hello world!");
  await deno.copyFile(fromFilename, toFilename);
  // No change to original file
  assertEqual(readFileString(fromFilename), "Hello world!");
  // Original == Dest
  assertSameContent(fromFilename, toFilename);
});

testPerm({ read: true, write: true }, async function copyFileFailure() {
  const tempDir = deno.makeTempDirSync();
  const fromFilename = tempDir + "/from.txt";
  const toFilename = tempDir + "/to.txt";
  // We skip initial writing here, from.txt does not exist
  let err;
  try {
    await deno.copyFile(fromFilename, toFilename);
  } catch (e) {
    err = e;
  }
  assert(!!err);
  assertEqual(err.kind, deno.ErrorKind.NotFound);
  assertEqual(err.name, "NotFound");
});

testPerm({ read: true, write: true }, async function copyFileOverwrite() {
  const tempDir = deno.makeTempDirSync();
  const fromFilename = tempDir + "/from.txt";
  const toFilename = tempDir + "/to.txt";
  writeFileString(fromFilename, "Hello world!");
  // Make Dest exist and have different content
  writeFileString(toFilename, "Goodbye!");
  await deno.copyFile(fromFilename, toFilename);
  // No change to original file
  assertEqual(readFileString(fromFilename), "Hello world!");
  // Original == Dest
  assertSameContent(fromFilename, toFilename);
});

testPerm({ read: false, write: true }, async function copyFilePerm1() {
  let caughtError = false;
  try {
    await deno.copyFile("/from.txt", "/to.txt");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.PermissionDenied);
    assertEqual(e.name, "PermissionDenied");
  }
  assert(caughtError);
});

testPerm({ read: true, write: false }, async function copyFilePerm2() {
  let caughtError = false;
  try {
    await deno.copyFile("/from.txt", "/to.txt");
  } catch (e) {
    caughtError = true;
    assertEqual(e.kind, deno.ErrorKind.PermissionDenied);
    assertEqual(e.name, "PermissionDenied");
  }
  assert(caughtError);
});
