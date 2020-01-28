// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { test } from "../testing/mod.ts";
import { assertEquals, assertStrContains } from "../testing/asserts.ts";
import * as path from "../path/mod.ts";
import { exists, existsSync } from "./exists.ts";

const testdataDir = path.resolve("fs", "testdata");

test(async function existsFile(): Promise<void> {
  assertEquals(
    await exists(path.join(testdataDir, "not_exist_file.ts")),
    false
  );
  assertEquals(await existsSync(path.join(testdataDir, "0.ts")), true);
});

test(function existsFileSync(): void {
  assertEquals(existsSync(path.join(testdataDir, "not_exist_file.ts")), false);
  assertEquals(existsSync(path.join(testdataDir, "0.ts")), true);
});

test(async function existsDirectory(): Promise<void> {
  assertEquals(
    await exists(path.join(testdataDir, "not_exist_directory")),
    false
  );
  assertEquals(existsSync(testdataDir), true);
});

test(function existsDirectorySync(): void {
  assertEquals(
    existsSync(path.join(testdataDir, "not_exist_directory")),
    false
  );
  assertEquals(existsSync(testdataDir), true);
});

test(function existsLinkSync(): void {
  // TODO(axetroy): generate link file use Deno api instead of set a link file
  // in repository
  assertEquals(existsSync(path.join(testdataDir, "0-link.ts")), true);
});

test(async function existsLink(): Promise<void> {
  // TODO(axetroy): generate link file use Deno api instead of set a link file
  // in repository
  assertEquals(await exists(path.join(testdataDir, "0-link.ts")), true);
});

test(async function existsPermission(): Promise<void> {
  interface Scenes {
    read: boolean; // --allow-read
    async: boolean;
    output: string;
    file: string; // target file to run
  }

  const scenes: Scenes[] = [
    // 1
    {
      read: false,
      async: true,
      output: "run again with the --allow-read flag",
      file: "0.ts"
    },
    {
      read: false,
      async: false,
      output: "run again with the --allow-read flag",
      file: "0.ts"
    },
    // 2
    {
      read: true,
      async: true,
      output: "exist",
      file: "0.ts"
    },
    {
      read: true,
      async: false,
      output: "exist",
      file: "0.ts"
    },
    // 3
    {
      read: false,
      async: true,
      output: "run again with the --allow-read flag",
      file: "no_exist_file_for_test.ts"
    },
    {
      read: false,
      async: false,
      output: "run again with the --allow-read flag",
      file: "no_exist_file_for_test.ts"
    },
    // 4
    {
      read: true,
      async: true,
      output: "not exist",
      file: "no_exist_file_for_test.ts"
    },
    {
      read: true,
      async: false,
      output: "not exist",
      file: "no_exist_file_for_test.ts"
    }
  ];

  for (const s of scenes) {
    console.log(
      `test ${s.async ? "exists" : "existsSync"}("testdata/${s.file}") ${
        s.read ? "with" : "without"
      } --allow-read`
    );

    const args = [Deno.execPath(), "run"];

    if (s.read) {
      args.push("--allow-read");
    }

    args.push(path.join(testdataDir, s.async ? "exists.ts" : "exists_sync.ts"));
    args.push(s.file);

    const { stdout } = Deno.run({
      stdout: "piped",
      cwd: testdataDir,
      args: args
    });

    const output = await Deno.readAll(stdout);

    assertStrContains(new TextDecoder().decode(output), s.output);
  }

  // done
});
