// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
/*
SharedQueue Binary Layout
+-------------------------------+-------------------------------+
|                        NUM_RECORDS (32)                       |
+---------------------------------------------------------------+
|                        NUM_SHIFTED_OFF (32)                   |
+---------------------------------------------------------------+
|                        HEAD (32)                              |
+---------------------------------------------------------------+
|                        OFFSETS (32)                           |
+---------------------------------------------------------------+
|                        RECORD_ENDS (*MAX_RECORDS)           ...
+---------------------------------------------------------------+
|                        RECORDS (*MAX_RECORDS)               ...
+---------------------------------------------------------------+
 */

/* eslint-disable @typescript-eslint/no-use-before-define */

(window => {
  const GLOBAL_NAMESPACE = "Deno";
  const CORE_NAMESPACE = "core";
  const MAX_RECORDS = 100;
  const INDEX_NUM_RECORDS = 0;
  const INDEX_NUM_SHIFTED_OFF = 1;
  const INDEX_HEAD = 2;
  const INDEX_OFFSETS = 3;
  const INDEX_RECORDS = INDEX_OFFSETS + 2 * MAX_RECORDS;
  const HEAD_INIT = 4 * INDEX_RECORDS;

  // Available on start due to bindings.
  const Deno = window[GLOBAL_NAMESPACE];
  const core = Deno[CORE_NAMESPACE];
  // Warning: DO NOT use window.Deno after this point.
  // It is possible that the Deno namespace has been deleted.
  // Use the above local Deno and core variable instead.

  let sharedBytes;
  let shared32;
  let initialized = false;

  function maybeInit() {
    if (!initialized) {
      init();
      initialized = true;
    }
  }

  function init() {
    const shared = Deno.core.shared;
    assert(shared.byteLength > 0);
    assert(sharedBytes == null);
    assert(shared32 == null);
    sharedBytes = new Uint8Array(shared);
    shared32 = new Int32Array(shared);
    // Callers should not call Deno.core.recv, use setAsyncHandler.
    Deno.core.recv(handleAsyncMsgFromRust);
  }

  function assert(cond) {
    if (!cond) {
      throw Error("assert");
    }
  }

  function reset() {
    maybeInit();
    shared32[INDEX_NUM_RECORDS] = 0;
    shared32[INDEX_NUM_SHIFTED_OFF] = 0;
    shared32[INDEX_HEAD] = HEAD_INIT;
  }

  function head() {
    maybeInit();
    return shared32[INDEX_HEAD];
  }

  function numRecords() {
    return shared32[INDEX_NUM_RECORDS];
  }

  function size() {
    return shared32[INDEX_NUM_RECORDS] - shared32[INDEX_NUM_SHIFTED_OFF];
  }

  // TODO(ry) rename to setMeta
  function setMeta(index, end, opId) {
    shared32[INDEX_OFFSETS + 2 * index] = end;
    shared32[INDEX_OFFSETS + 2 * index + 1] = opId;
  }

  function getMeta(index) {
    if (index < numRecords()) {
      const buf = shared32[INDEX_OFFSETS + 2 * index];
      const opId = shared32[INDEX_OFFSETS + 2 * index + 1];
      return [opId, buf];
    } else {
      return null;
    }
  }

  function getOffset(index) {
    if (index < numRecords()) {
      if (index == 0) {
        return HEAD_INIT;
      } else {
        return shared32[INDEX_OFFSETS + 2 * (index - 1)];
      }
    } else {
      return null;
    }
  }

  function push(opId, buf) {
    const off = head();
    const end = off + buf.byteLength;
    const index = numRecords();
    if (end > shared32.byteLength || index >= MAX_RECORDS) {
      // console.log("shared_queue.js push fail");
      return false;
    }
    setMeta(index, end, opId);
    assert(end - off == buf.byteLength);
    sharedBytes.set(buf, off);
    shared32[INDEX_NUM_RECORDS] += 1;
    shared32[INDEX_HEAD] = end;
    return true;
  }

  /// Returns null if empty.
  function shift() {
    const i = shared32[INDEX_NUM_SHIFTED_OFF];
    if (size() == 0) {
      assert(i == 0);
      return null;
    }

    const off = getOffset(i);
    const [opId, end] = getMeta(i);

    if (size() > 1) {
      shared32[INDEX_NUM_SHIFTED_OFF] += 1;
    } else {
      reset();
    }

    assert(off != null);
    assert(end != null);
    const buf = sharedBytes.subarray(off, end);
    return [opId, buf];
  }

  let asyncHandler;
  function setAsyncHandler(cb) {
    maybeInit();
    assert(asyncHandler == null);
    asyncHandler = cb;
  }

  function handleAsyncMsgFromRust(opId, buf) {
    if (buf) {
      // This is the overflow_response case of deno::Isolate::poll().
      asyncHandler(opId, buf);
    } else {
      while (true) {
        const opIdBuf = shift();
        if (opIdBuf == null) {
          break;
        }
        asyncHandler(...opIdBuf);
      }
    }
  }

  function dispatch(opId, control, zeroCopy = null) {
    maybeInit();
    return Deno.core.send(opId, control, zeroCopy);
  }

  const denoCore = {
    setAsyncHandler,
    dispatch,
    sharedQueue: {
      MAX_RECORDS,
      head,
      numRecords,
      size,
      push,
      reset,
      shift
    }
  };

  assert(window[GLOBAL_NAMESPACE] != null);
  assert(window[GLOBAL_NAMESPACE][CORE_NAMESPACE] != null);
  Object.assign(core, denoCore);
})(this);
