// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import * as dispatch from "./dispatch";
import * as msg from "gen/cli/msg_generated";
import * as flatbuffers from "./flatbuffers";
import { assert, log } from "./util";
import { window } from "./window";

export async function postMessage(data: Uint8Array): Promise<void> {
  const builder = flatbuffers.createBuilder();
  msg.WorkerPostMessage.startWorkerPostMessage(builder);
  const inner = msg.WorkerPostMessage.endWorkerPostMessage(builder);
  const baseRes = await dispatch.sendAsync(
    builder,
    msg.Any.WorkerPostMessage,
    inner,
    data
  );
  assert(baseRes != null);
}

export async function getMessage(): Promise<null | Uint8Array> {
  log("getMessage");
  const builder = flatbuffers.createBuilder();
  msg.WorkerGetMessage.startWorkerGetMessage(builder);
  const inner = msg.WorkerGetMessage.endWorkerGetMessage(builder);
  const baseRes = await dispatch.sendAsync(
    builder,
    msg.Any.WorkerGetMessage,
    inner
  );
  assert(baseRes != null);
  assert(
    msg.Any.WorkerGetMessageRes === baseRes!.innerType(),
    `base.innerType() unexpectedly is ${baseRes!.innerType()}`
  );
  const res = new msg.WorkerGetMessageRes();
  assert(baseRes!.inner(res) != null);

  const dataArray = res.dataArray();
  if (dataArray == null) {
    return null;
  } else {
    return new Uint8Array(dataArray!);
  }
}

let isClosing = false;

export function workerClose(): void {
  isClosing = true;
}

export async function workerMain(): Promise<void> {
  log("workerMain");

  while (!isClosing) {
    const data = await getMessage();
    if (data == null) {
      log("workerMain got null message. quitting.");
      break;
    }
    if (window["onmessage"]) {
      const event = { data };
      window.onmessage(event);
    } else {
      break;
    }
  }
}
