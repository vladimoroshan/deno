#!/usr/bin/env -S deno run --unstable --allow-read --allow-write --allow-run
// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

import { join, ROOT_PATH } from "./util.js";

const COMMIT = "659f6977051345e4e06ab4832c6f7d268f25a1ad";
const REPO = "gfx-rs/wgpu";
const V_WGPU = "0.15";
const TARGET_DIR = join(ROOT_PATH, "ext", "webgpu");

async function bash(subcmd, opts = {}) {
  const { success, code } = await new Deno.Command("bash", {
    ...opts,
    args: ["-c", subcmd],
    stdout: "inherit",
    sdterr: "inherit",
  }).output();

  // Exit process on failure
  if (!success) {
    Deno.exit(code);
  }
}

async function clearTargetDir() {
  await bash(`rm -r ${TARGET_DIR}/*`);
}

async function checkoutUpstream() {
  // Path of deno_webgpu inside the TAR
  const tarPrefix = `${REPO.replace("/", "-")}-${
    COMMIT.slice(0, 7)
  }/deno_webgpu/`;
  const cmd =
    `curl -L https://api.github.com/repos/${REPO}/tarball/${COMMIT} | tar -C '${TARGET_DIR}' -xzvf - --strip=2 '${tarPrefix}'`;
  // console.log(cmd);
  await bash(cmd);
}

async function denoWebgpuVersion() {
  const coreCargo = join(ROOT_PATH, "Cargo.toml");
  const contents = await Deno.readTextFile(coreCargo);
  return contents.match(
    /^deno_webgpu = { version = "(\d+\.\d+\.\d+)", path = ".\/ext\/webgpu" }$/m,
  )[1];
}

async function patchFile(path, patcher) {
  const data = await Deno.readTextFile(path);
  const patched = patcher(data);
  await Deno.writeTextFile(path, patched);
}

async function patchCargo() {
  const vDenoWebgpu = await denoWebgpuVersion();
  await patchFile(
    join(TARGET_DIR, "Cargo.toml"),
    (data) =>
      data
        .replace(/^version = .*/m, `version = "${vDenoWebgpu}"`)
        .replace(
          /^repository.workspace = true/m,
          `repository = "https://github.com/gfx-rs/wgpu"`,
        )
        .replace(
          /^serde = { workspace = true, features = ["derive"] }/m,
          `serde.workspace = true`,
        )
        .replace(
          /^tokio = { workspace = true, features = ["full"] }/m,
          `tokio.workspace = true`,
        ),
  );

  await patchFile(
    join(ROOT_PATH, "Cargo.toml"),
    (data) =>
      data
        .replace(/^wgpu-core = .*/m, `wgpu-core = "${V_WGPU}"`)
        .replace(/^wgpu-types = .*/m, `wgpu-types = "${V_WGPU}"`),
  );
}

async function patchSrcLib() {
  await patchFile(
    join(TARGET_DIR, "src", "lib.rs"),
    (data) =>
      data.replace(
        `prefix "internal:deno_webgpu",`,
        `prefix "internal:ext/webgpu",`,
      ),
  );
}

async function main() {
  await clearTargetDir();
  await checkoutUpstream();
  await patchCargo();
  await patchSrcLib();
  await bash(join(ROOT_PATH, "tools", "format.js"));
}

await main();
