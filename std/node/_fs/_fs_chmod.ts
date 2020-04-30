// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

import { CallbackWithError } from "./_fs_common.ts";

const allowedModes = /^[0-7]{3}/;

/**
 * TODO: Also accept 'path' parameter as a Node polyfill Buffer or URL type once these
 * are implemented. See https://github.com/denoland/deno/issues/3403
 */
export function chmod(
  path: string,
  mode: string | number,
  callback: CallbackWithError
): void {
  Deno.chmod(path, getResolvedMode(mode))
    .then(() => callback())
    .catch(callback);
}

/**
 * TODO: Also accept 'path' parameter as a Node polyfill Buffer or URL type once these
 * are implemented. See https://github.com/denoland/deno/issues/3403
 */
export function chmodSync(path: string, mode: string | number): void {
  Deno.chmodSync(path, getResolvedMode(mode));
}

function getResolvedMode(mode: string | number): number {
  if (typeof mode === "number") {
    return mode;
  }

  if (typeof mode === "string" && !allowedModes.test(mode)) {
    throw new Error("Unrecognized mode: " + mode);
  }

  return parseInt(mode, 8);
}
