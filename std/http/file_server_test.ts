// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { test } from "../testing/mod.ts";
import { assert, assertEquals } from "../testing/asserts.ts";
import { BufReader } from "../io/bufio.ts";
import { TextProtoReader } from "../textproto/mod.ts";

let fileServer: Deno.Process;

async function startFileServer(): Promise<void> {
  fileServer = Deno.run({
    args: [
      Deno.execPath(),
      "run",
      "--allow-read",
      "--allow-net",
      "http/file_server.ts",
      ".",
      "--cors"
    ],
    stdout: "piped"
  });
  // Once fileServer is ready it will write to its stdout.
  const r = new TextProtoReader(new BufReader(fileServer.stdout!));
  const s = await r.readLine();
  assert(s !== Deno.EOF && s.includes("server listening"));
}

function killFileServer(): void {
  fileServer.close();
  fileServer.stdout!.close();
}

test(async function serveFile(): Promise<void> {
  await startFileServer();
  try {
    const res = await fetch("http://localhost:4500/README.md");
    assert(res.headers.has("access-control-allow-origin"));
    assert(res.headers.has("access-control-allow-headers"));
    assert(res.headers.has("content-type"));
    assert(res.headers.get("content-type").includes("charset=utf-8"));
    const downloadedFile = await res.text();
    const localFile = new TextDecoder().decode(
      await Deno.readFile("README.md")
    );
    assertEquals(downloadedFile, localFile);
  } finally {
    killFileServer();
  }
});

test(async function serveDirectory(): Promise<void> {
  await startFileServer();
  try {
    const res = await fetch("http://localhost:4500/");
    assert(res.headers.has("access-control-allow-origin"));
    assert(res.headers.has("access-control-allow-headers"));
    const page = await res.text();
    assert(page.includes("README.md"));

    // `Deno.FileInfo` is not completely compatible with Windows yet
    // TODO: `mode` should work correctly in the future.
    // Correct this test case accordingly.
    Deno.build.os !== "win" &&
      assert(/<td class="mode">(\s)*\([a-zA-Z-]{10}\)(\s)*<\/td>/.test(page));
    Deno.build.os === "win" &&
      assert(/<td class="mode">(\s)*\(unknown mode\)(\s)*<\/td>/.test(page));
    assert(page.includes(`<a href="/README.md">README.md</a>`));
  } finally {
    killFileServer();
  }
});

test(async function serveFallback(): Promise<void> {
  await startFileServer();
  try {
    const res = await fetch("http://localhost:4500/badfile.txt");
    assert(res.headers.has("access-control-allow-origin"));
    assert(res.headers.has("access-control-allow-headers"));
    assertEquals(res.status, 404);
  } finally {
    killFileServer();
  }
});

test(async function serveFallback(): Promise<void> {
  await startFileServer();
  try {
    const res = await fetch(
      "http://localhost:4500/http/testdata/test%20file.txt"
    );
    assert(res.headers.has("access-control-allow-origin"));
    assert(res.headers.has("access-control-allow-headers"));
    assertEquals(res.status, 200);
  } finally {
    killFileServer();
  }
});

test(async function servePermissionDenied(): Promise<void> {
  const deniedServer = Deno.run({
    args: [Deno.execPath(), "run", "--allow-net", "http/file_server.ts"],
    stdout: "piped",
    stderr: "piped"
  });
  const reader = new TextProtoReader(new BufReader(deniedServer.stdout!));
  const errReader = new TextProtoReader(new BufReader(deniedServer.stderr!));
  const s = await reader.readLine();
  assert(s !== Deno.EOF && s.includes("server listening"));

  try {
    await fetch("http://localhost:4500/");
    assertEquals(
      await errReader.readLine(),
      "run again with the --allow-read flag"
    );
  } finally {
    deniedServer.close();
    deniedServer.stdout!.close();
    deniedServer.stderr!.close();
  }
});

test(async function printHelp(): Promise<void> {
  const helpProcess = Deno.run({
    args: [Deno.execPath(), "run", "http/file_server.ts", "--help"],
    stdout: "piped"
  });
  const r = new TextProtoReader(new BufReader(helpProcess.stdout!));
  const s = await r.readLine();
  assert(s !== Deno.EOF && s.includes("Deno File Server"));
  helpProcess.close();
  helpProcess.stdout!.close();
});
