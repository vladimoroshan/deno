// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import * as util from "./util.ts";
import { core } from "./core.ts";
import { TextDecoder } from "./text_encoding.ts";
import { ErrorKind, DenoError } from "./errors.ts";

const promiseTableMin = new Map<number, util.Resolvable<RecordMinimal>>();
// Note it's important that promiseId starts at 1 instead of 0, because sync
// messages are indicated with promiseId 0. If we ever add wrap around logic for
// overflows, this should be taken into account.
let _nextPromiseId = 1;

const decoder = new TextDecoder();

function nextPromiseId(): number {
  return _nextPromiseId++;
}

export interface RecordMinimal {
  promiseId: number;
  opId: number; // Maybe better called dispatchId
  arg: number;
  result: number;
  err?: {
    kind: ErrorKind;
    message: string;
  };
}

export function recordFromBufMinimal(
  opId: number,
  ui8: Uint8Array
): RecordMinimal {
  const header = ui8.slice(0, 12);
  const buf32 = new Int32Array(
    header.buffer,
    header.byteOffset,
    header.byteLength / 4
  );
  const promiseId = buf32[0];
  const arg = buf32[1];
  const result = buf32[2];
  let err;

  if (arg < 0) {
    const kind = result as ErrorKind;
    const message = decoder.decode(ui8.slice(12));
    err = { kind, message };
  } else if (ui8.length != 12) {
    err = { kind: ErrorKind.InvalidData, message: "Bad message" };
  }

  return {
    promiseId,
    opId,
    arg,
    result,
    err
  };
}

function unwrapResponse(res: RecordMinimal): number {
  if (res.err != null) {
    throw new DenoError(res.err!.kind, res.err!.message);
  }
  return res.result;
}

const scratch32 = new Int32Array(3);
const scratchBytes = new Uint8Array(
  scratch32.buffer,
  scratch32.byteOffset,
  scratch32.byteLength
);
util.assert(scratchBytes.byteLength === scratch32.length * 4);

export function asyncMsgFromRust(opId: number, ui8: Uint8Array): void {
  const record = recordFromBufMinimal(opId, ui8);
  const { promiseId } = record;
  const promise = promiseTableMin.get(promiseId);
  promiseTableMin.delete(promiseId);
  util.assert(promise);
  promise.resolve(record);
}

export async function sendAsyncMinimal(
  opId: number,
  arg: number,
  zeroCopy: Uint8Array
): Promise<number> {
  const promiseId = nextPromiseId(); // AKA cmdId
  scratch32[0] = promiseId;
  scratch32[1] = arg;
  scratch32[2] = 0; // result
  const promise = util.createResolvable<RecordMinimal>();
  const buf = core.dispatch(opId, scratchBytes, zeroCopy);
  if (buf) {
    const record = recordFromBufMinimal(opId, buf);
    // Sync result.
    promise.resolve(record);
  } else {
    // Async result.
    promiseTableMin.set(promiseId, promise);
  }

  const res = await promise;
  return unwrapResponse(res);
}

export function sendSyncMinimal(
  opId: number,
  arg: number,
  zeroCopy: Uint8Array
): number {
  scratch32[0] = 0; // promiseId 0 indicates sync
  scratch32[1] = arg;
  const res = core.dispatch(opId, scratchBytes, zeroCopy)!;
  const resRecord = recordFromBufMinimal(opId, res);
  return unwrapResponse(resRecord);
}
