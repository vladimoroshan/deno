// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
import {
  assertEquals,
  assertRejects,
  assertThrows,
  pathToAbsoluteFileUrl,
  unitTest,
} from "./test_util.ts";

unitTest(
  { permissions: { read: true, write: true } },
  async function futimeSyncSuccess() {
    const testDir = await Deno.makeTempDir();
    const filename = testDir + "/file.txt";
    const file = await Deno.open(filename, {
      create: true,
      write: true,
    });

    const atime = 1000;
    const mtime = 50000;
    await Deno.futime(file.rid, atime, mtime);
    await Deno.fdatasync(file.rid);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, new Date(atime * 1000));
    assertEquals(fileInfo.mtime, new Date(mtime * 1000));
    file.close();
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function futimeSyncSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    const file = Deno.openSync(filename, {
      create: true,
      write: true,
    });

    const atime = 1000;
    const mtime = 50000;
    Deno.futimeSync(file.rid, atime, mtime);
    Deno.fdatasyncSync(file.rid);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, new Date(atime * 1000));
    assertEquals(fileInfo.mtime, new Date(mtime * 1000));
    file.close();
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncFileSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    Deno.writeFileSync(filename, new TextEncoder().encode("hello"), {
      mode: 0o666,
    });

    const atime = 1000;
    const mtime = 50000;
    Deno.utimeSync(filename, atime, mtime);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, new Date(atime * 1000));
    assertEquals(fileInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncUrlSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    Deno.writeFileSync(filename, new TextEncoder().encode("hello"), {
      mode: 0o666,
    });

    const atime = 1000;
    const mtime = 50000;
    Deno.utimeSync(pathToAbsoluteFileUrl(filename), atime, mtime);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, new Date(atime * 1000));
    assertEquals(fileInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncDirectorySuccess() {
    const testDir = Deno.makeTempDirSync();

    const atime = 1000;
    const mtime = 50000;
    Deno.utimeSync(testDir, atime, mtime);

    const dirInfo = Deno.statSync(testDir);
    assertEquals(dirInfo.atime, new Date(atime * 1000));
    assertEquals(dirInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncDateSuccess() {
    const testDir = Deno.makeTempDirSync();

    const atime = new Date(1000_000);
    const mtime = new Date(50000_000);
    Deno.utimeSync(testDir, atime, mtime);

    const dirInfo = Deno.statSync(testDir);
    assertEquals(dirInfo.atime, atime);
    assertEquals(dirInfo.mtime, mtime);
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncFileDateSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    Deno.writeFileSync(filename, new TextEncoder().encode("hello"), {
      mode: 0o666,
    });
    const atime = new Date();
    const mtime = new Date();
    Deno.utimeSync(filename, atime, mtime);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, atime);
    assertEquals(fileInfo.mtime, mtime);
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncLargeNumberSuccess() {
    const testDir = Deno.makeTempDirSync();

    // There are Rust side caps (might be fs relate),
    // so JUST make them slightly larger than UINT32_MAX.
    const atime = 0x100000001;
    const mtime = 0x100000002;
    Deno.utimeSync(testDir, atime, mtime);

    const dirInfo = Deno.statSync(testDir);
    assertEquals(dirInfo.atime, new Date(atime * 1000));
    assertEquals(dirInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  function utimeSyncNotFound() {
    const atime = 1000;
    const mtime = 50000;

    assertThrows(() => {
      Deno.utimeSync("/baddir", atime, mtime);
    }, Deno.errors.NotFound);
  },
);

unitTest(
  { permissions: { read: true, write: false } },
  function utimeSyncPerm() {
    const atime = 1000;
    const mtime = 50000;

    assertThrows(() => {
      Deno.utimeSync("/some_dir", atime, mtime);
    }, Deno.errors.PermissionDenied);
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  async function utimeFileSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    Deno.writeFileSync(filename, new TextEncoder().encode("hello"), {
      mode: 0o666,
    });

    const atime = 1000;
    const mtime = 50000;
    await Deno.utime(filename, atime, mtime);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, new Date(atime * 1000));
    assertEquals(fileInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  async function utimeUrlSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    Deno.writeFileSync(filename, new TextEncoder().encode("hello"), {
      mode: 0o666,
    });

    const atime = 1000;
    const mtime = 50000;
    await Deno.utime(pathToAbsoluteFileUrl(filename), atime, mtime);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, new Date(atime * 1000));
    assertEquals(fileInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  async function utimeDirectorySuccess() {
    const testDir = Deno.makeTempDirSync();

    const atime = 1000;
    const mtime = 50000;
    await Deno.utime(testDir, atime, mtime);

    const dirInfo = Deno.statSync(testDir);
    assertEquals(dirInfo.atime, new Date(atime * 1000));
    assertEquals(dirInfo.mtime, new Date(mtime * 1000));
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  async function utimeDateSuccess() {
    const testDir = Deno.makeTempDirSync();

    const atime = new Date(100_000);
    const mtime = new Date(5000_000);
    await Deno.utime(testDir, atime, mtime);

    const dirInfo = Deno.statSync(testDir);
    assertEquals(dirInfo.atime, atime);
    assertEquals(dirInfo.mtime, mtime);
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  async function utimeFileDateSuccess() {
    const testDir = Deno.makeTempDirSync();
    const filename = testDir + "/file.txt";
    Deno.writeFileSync(filename, new TextEncoder().encode("hello"), {
      mode: 0o666,
    });

    const atime = new Date();
    const mtime = new Date();
    await Deno.utime(filename, atime, mtime);

    const fileInfo = Deno.statSync(filename);
    assertEquals(fileInfo.atime, atime);
    assertEquals(fileInfo.mtime, mtime);
  },
);

unitTest(
  { permissions: { read: true, write: true } },
  async function utimeNotFound() {
    const atime = 1000;
    const mtime = 50000;

    await assertRejects(async () => {
      await Deno.utime("/baddir", atime, mtime);
    }, Deno.errors.NotFound);
  },
);

unitTest(
  { permissions: { read: true, write: false } },
  async function utimeSyncPerm() {
    const atime = 1000;
    const mtime = 50000;

    await assertRejects(async () => {
      await Deno.utime("/some_dir", atime, mtime);
    }, Deno.errors.PermissionDenied);
  },
);
