// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
import { CallbackWithError } from "internal:deno_node/polyfills/_fs/_fs_common.ts";

export function fdatasync(
  fd: number,
  callback: CallbackWithError,
) {
  Deno.fdatasync(fd).then(() => callback(null), callback);
}

export function fdatasyncSync(fd: number) {
  Deno.fdatasyncSync(fd);
}
