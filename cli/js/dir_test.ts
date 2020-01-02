// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { test, testPerm, assert, assertEquals } from "./test_util.ts";

test(function dirCwdNotNull(): void {
  assert(Deno.cwd() != null);
});

testPerm({ write: true }, function dirCwdChdirSuccess(): void {
  const initialdir = Deno.cwd();
  const path = Deno.makeTempDirSync();
  Deno.chdir(path);
  const current = Deno.cwd();
  if (Deno.build.os === "mac") {
    assertEquals(current, "/private" + path);
  } else {
    assertEquals(current, path);
  }
  Deno.chdir(initialdir);
});

testPerm({ write: true }, function dirCwdError(): void {
  // excluding windows since it throws resource busy, while removeSync
  if (["linux", "mac"].includes(Deno.build.os)) {
    const initialdir = Deno.cwd();
    const path = Deno.makeTempDirSync();
    Deno.chdir(path);
    Deno.removeSync(path);
    try {
      Deno.cwd();
      throw Error("current directory removed, should throw error");
    } catch (err) {
      if (err instanceof Deno.DenoError) {
        console.log(err.name === "NotFound");
      } else {
        throw Error("raised different exception");
      }
    }
    Deno.chdir(initialdir);
  }
});

testPerm({ write: true }, function dirChdirError(): void {
  const path = Deno.makeTempDirSync() + "test";
  try {
    Deno.chdir(path);
    throw Error("directory not available, should throw error");
  } catch (err) {
    if (err instanceof Deno.DenoError) {
      console.log(err.name === "NotFound");
    } else {
      throw Error("raised different exception");
    }
  }
});
