// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

// Interfaces 100% copied from Go.
// Documentation liberally lifted from them too.
// Thank you! We love Go! <3
"use strict";

((window) => {
  const core = window.Deno.core;
  const DEFAULT_BUFFER_SIZE = 32 * 1024;
  // Seek whence values.
  // https://golang.org/pkg/io/#pkg-constants
  const SeekMode = {
    0: "Start",
    1: "Current",
    2: "End",

    Start: 0,
    Current: 1,
    End: 2,
  };

  async function copy(
    src,
    dst,
    options,
  ) {
    let n = 0;
    const bufSize = options?.bufSize ?? DEFAULT_BUFFER_SIZE;
    const b = new Uint8Array(bufSize);
    let gotEOF = false;
    while (gotEOF === false) {
      const result = await src.read(b);
      if (result === null) {
        gotEOF = true;
      } else {
        let nwritten = 0;
        while (nwritten < result) {
          nwritten += await dst.write(b.subarray(nwritten, result));
        }
        n += nwritten;
      }
    }
    return n;
  }

  async function* iter(
    r,
    options,
  ) {
    const bufSize = options?.bufSize ?? DEFAULT_BUFFER_SIZE;
    const b = new Uint8Array(bufSize);
    while (true) {
      const result = await r.read(b);
      if (result === null) {
        break;
      }

      yield b.subarray(0, result);
    }
  }

  function* iterSync(
    r,
    options,
  ) {
    const bufSize = options?.bufSize ?? DEFAULT_BUFFER_SIZE;
    const b = new Uint8Array(bufSize);
    while (true) {
      const result = r.readSync(b);
      if (result === null) {
        break;
      }

      yield b.subarray(0, result);
    }
  }

  function readSync(rid, buffer) {
    if (buffer.length === 0) {
      return 0;
    }

    const nread = core.opSync("op_read_sync", rid, buffer);

    return nread === 0 ? null : nread;
  }

  async function read(
    rid,
    buffer,
  ) {
    if (buffer.length === 0) {
      return 0;
    }

    const nread = await core.opAsync("op_read_async", rid, buffer);

    return nread === 0 ? null : nread;
  }

  function writeSync(rid, data) {
    return core.opSync("op_write_sync", rid, data);
  }

  async function write(rid, data) {
    return await core.opAsync("op_write_async", rid, data);
  }

  const READ_PER_ITER = 32 * 1024;

  async function readAll(r) {
    const buffers = [];

    while (true) {
      const buf = new Uint8Array(READ_PER_ITER);
      const read = await r.read(buf);
      if (typeof read == "number") {
        buffers.push(new Uint8Array(buf.buffer, 0, read));
      } else {
        break;
      }
    }

    let totalLen = 0;
    for (const buf of buffers) {
      totalLen += buf.byteLength;
    }

    const contents = new Uint8Array(totalLen);

    let n = 0;
    for (const buf of buffers) {
      contents.set(buf, n);
      n += buf.byteLength;
    }

    return contents;
  }

  function readAllSync(r) {
    const buffers = [];

    while (true) {
      const buf = new Uint8Array(READ_PER_ITER);
      const read = r.readSync(buf);
      if (typeof read == "number") {
        buffers.push(new Uint8Array(buf.buffer, 0, read));
      } else {
        break;
      }
    }

    let totalLen = 0;
    for (const buf of buffers) {
      totalLen += buf.byteLength;
    }

    const contents = new Uint8Array(totalLen);

    let n = 0;
    for (const buf of buffers) {
      contents.set(buf, n);
      n += buf.byteLength;
    }

    return contents;
  }

  window.__bootstrap.io = {
    iterSync,
    iter,
    copy,
    SeekMode,
    read,
    readSync,
    write,
    writeSync,
    readAll,
    readAllSync,
  };
})(this);
