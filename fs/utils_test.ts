// Copyright the Browserify authors. MIT License.

import { test } from "../testing/mod.ts";
import { assertEquals } from "../testing/asserts.ts";
import { isSubdir, getFileInfoType, PathType } from "./utils.ts";
import * as path from "./path/mod.ts";
import { ensureFileSync } from "./ensure_file.ts";
import { ensureDirSync } from "./ensure_dir.ts";

const testdataDir = path.resolve("fs", "testdata");

test(function _isSubdir() {
  const pairs = [
    ["", "", false, path.posix.sep],
    ["/first/second", "/first", false, path.posix.sep],
    ["/first", "/first", false, path.posix.sep],
    ["/first", "/first/second", true, path.posix.sep],
    ["first", "first/second", true, path.posix.sep],
    ["../first", "../first/second", true, path.posix.sep],
    ["c:\\first", "c:\\first", false, path.win32.sep],
    ["c:\\first", "c:\\first\\second", true, path.win32.sep]
  ];

  pairs.forEach(function(p) {
    const src = p[0] as string;
    const dest = p[1] as string;
    const expected = p[2] as boolean;
    const sep = p[3] as string;
    assertEquals(
      isSubdir(src, dest, sep),
      expected,
      `'${src}' should ${expected ? "" : "not"} be parent dir of '${dest}'`
    );
  });
});

test(function _getFileInfoType() {
  const pairs = [
    [path.join(testdataDir, "file_type_1"), PathType.file],
    [path.join(testdataDir, "file_type_dir_1"), PathType.dir]
  ];

  pairs.forEach(function(p) {
    const filePath = p[0] as string;
    const type = p[1] as PathType;
    switch (type) {
      case PathType.file:
        ensureFileSync(filePath);
        break;
      case PathType.dir:
        ensureDirSync(filePath);
        break;
      case PathType.symlink:
        // TODO(axetroy): test symlink
        break;
    }

    const stat = Deno.statSync(filePath);

    Deno.removeSync(filePath, { recursive: true });

    assertEquals(getFileInfoType(stat), type);
  });
});
