// Copyright the Browserify authors. MIT License.
// Ported mostly from https://github.com/browserify/path-browserify/

import * as _win32 from "./win32.ts";
import * as _posix from "./posix.ts";

import { isWindows } from "./constants.ts";

const path = isWindows ? _win32 : _posix;

export const win32 = _win32;
export const posix = _posix;
export const {
  resolve,
  normalize,
  isAbsolute,
  join,
  relative,
  toNamespacedPath,
  dirname,
  basename,
  extname,
  format,
  parse,
  sep,
  delimiter,
} = path;

export { common } from "./common.ts";
export { EOL, SEP, SEP_PATTERN, isWindows } from "./constants.ts";
export * from "./interface.ts";
export * from "./glob.ts";
export * from "./globrex.ts";
