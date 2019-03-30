// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import * as msg from "gen/cli/msg_generated";
import * as flatbuffers from "./flatbuffers";
import * as dispatch from "./dispatch";

function req(
  oldpath: string,
  newpath: string
): [flatbuffers.Builder, msg.Any, flatbuffers.Offset] {
  const builder = flatbuffers.createBuilder();
  const oldpath_ = builder.createString(oldpath);
  const newpath_ = builder.createString(newpath);
  msg.Rename.startRename(builder);
  msg.Rename.addOldpath(builder, oldpath_);
  msg.Rename.addNewpath(builder, newpath_);
  const inner = msg.Rename.endRename(builder);
  return [builder, msg.Any.Rename, inner];
}

/** Synchronously renames (moves) `oldpath` to `newpath`. If `newpath` already
 * exists and is not a directory, `renameSync()` replaces it. OS-specific
 * restrictions may apply when `oldpath` and `newpath` are in different
 * directories.
 *
 *       Deno.renameSync("old/path", "new/path");
 */
export function renameSync(oldpath: string, newpath: string): void {
  dispatch.sendSync(...req(oldpath, newpath));
}

/** Renames (moves) `oldpath` to `newpath`. If `newpath` already exists and is
 * not a directory, `rename()` replaces it. OS-specific restrictions may apply
 * when `oldpath` and `newpath` are in different directories.
 *
 *       await Deno.rename("old/path", "new/path");
 */
export async function rename(oldpath: string, newpath: string): Promise<void> {
  await dispatch.sendAsync(...req(oldpath, newpath));
}
