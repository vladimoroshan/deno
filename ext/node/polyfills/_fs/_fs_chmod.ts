// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
import type { CallbackWithError } from "internal:deno_node/polyfills/_fs/_fs_common.ts";
import { getValidatedPath } from "internal:deno_node/polyfills/internal/fs/utils.mjs";
import * as pathModule from "internal:deno_node/polyfills/path.ts";
import { parseFileMode } from "internal:deno_node/polyfills/internal/validators.mjs";
import { Buffer } from "internal:deno_node/polyfills/buffer.ts";
import { promisify } from "internal:deno_node/polyfills/internal/util.mjs";

export function chmod(
  path: string | Buffer | URL,
  mode: string | number,
  callback: CallbackWithError,
) {
  path = getValidatedPath(path).toString();

  try {
    mode = parseFileMode(mode, "mode");
  } catch (error) {
    // TODO(PolarETech): Errors should not be ignored when Deno.chmod is supported on Windows.
    // https://github.com/denoland/deno_std/issues/2916
    if (Deno.build.os === "windows") {
      mode = 0; // set dummy value to avoid type checking error at Deno.chmod
    } else {
      throw error;
    }
  }

  Deno.chmod(pathModule.toNamespacedPath(path), mode).catch((error) => {
    // Ignore NotSupportedError that occurs on windows
    // https://github.com/denoland/deno_std/issues/2995
    if (!(error instanceof Deno.errors.NotSupported)) {
      throw error;
    }
  }).then(
    () => callback(null),
    callback,
  );
}

export const chmodPromise = promisify(chmod) as (
  path: string | Buffer | URL,
  mode: string | number,
) => Promise<void>;

export function chmodSync(path: string | URL, mode: string | number) {
  path = getValidatedPath(path).toString();

  try {
    mode = parseFileMode(mode, "mode");
  } catch (error) {
    // TODO(PolarETech): Errors should not be ignored when Deno.chmodSync is supported on Windows.
    // https://github.com/denoland/deno_std/issues/2916
    if (Deno.build.os === "windows") {
      mode = 0; // set dummy value to avoid type checking error at Deno.chmodSync
    } else {
      throw error;
    }
  }

  try {
    Deno.chmodSync(pathModule.toNamespacedPath(path), mode);
  } catch (error) {
    // Ignore NotSupportedError that occurs on windows
    // https://github.com/denoland/deno_std/issues/2995
    if (!(error instanceof Deno.errors.NotSupported)) {
      throw error;
    }
  }
}
