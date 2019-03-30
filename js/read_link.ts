// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import * as msg from "gen/cli/msg_generated";
import * as flatbuffers from "./flatbuffers";
import { assert } from "./util";
import * as dispatch from "./dispatch";

function req(name: string): [flatbuffers.Builder, msg.Any, flatbuffers.Offset] {
  const builder = flatbuffers.createBuilder();
  const name_ = builder.createString(name);
  msg.Readlink.startReadlink(builder);
  msg.Readlink.addName(builder, name_);
  const inner = msg.Readlink.endReadlink(builder);
  return [builder, msg.Any.Readlink, inner];
}

function res(baseRes: null | msg.Base): string {
  assert(baseRes !== null);
  assert(msg.Any.ReadlinkRes === baseRes!.innerType());
  const res = new msg.ReadlinkRes();
  assert(baseRes!.inner(res) !== null);
  const path = res.path();
  assert(path !== null);
  return path!;
}

/** Returns the destination of the named symbolic link synchronously.
 *
 *       const targetPath = Deno.readlinkSync("symlink/path");
 */
export function readlinkSync(name: string): string {
  return res(dispatch.sendSync(...req(name)));
}

/** Returns the destination of the named symbolic link.
 *
 *       const targetPath = await Deno.readlink("symlink/path");
 */
export async function readlink(name: string): Promise<string> {
  return res(await dispatch.sendAsync(...req(name)));
}
