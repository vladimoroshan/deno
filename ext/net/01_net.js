// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.
"use strict";

((window) => {
  const core = window.Deno.core;
  const { BadResourcePrototype, InterruptedPrototype } = core;
  const { ReadableStream, WritableStream } = window.__bootstrap.streams;
  const {
    ObjectPrototypeIsPrototypeOf,
    PromiseResolve,
    SymbolAsyncIterator,
    Error,
    Uint8Array,
    TypedArrayPrototypeSubarray,
  } = window.__bootstrap.primordials;

  async function read(
    rid,
    buffer,
  ) {
    if (buffer.length === 0) {
      return 0;
    }
    const nread = await core.read(rid, buffer);
    return nread === 0 ? null : nread;
  }

  async function write(rid, data) {
    return await core.write(rid, data);
  }

  function shutdown(rid) {
    return core.shutdown(rid);
  }

  function opAccept(rid, transport) {
    return core.opAsync("op_net_accept", { rid, transport });
  }

  function opListen(args) {
    return core.opSync("op_net_listen", args);
  }

  function opConnect(args) {
    return core.opAsync("op_net_connect", args);
  }

  function opReceive(rid, transport, zeroCopy) {
    return core.opAsync(
      "op_dgram_recv",
      { rid, transport },
      zeroCopy,
    );
  }

  function opSend(args, zeroCopy) {
    return core.opAsync("op_dgram_send", args, zeroCopy);
  }

  function resolveDns(query, recordType, options) {
    return core.opAsync("op_dns_resolve", { query, recordType, options });
  }

  const DEFAULT_CHUNK_SIZE = 16_640;

  function tryClose(rid) {
    try {
      core.close(rid);
    } catch {
      // Ignore errors
    }
  }

  function readableStreamForRid(rid) {
    return new ReadableStream({
      type: "bytes",
      async pull(controller) {
        const v = controller.byobRequest.view;
        try {
          const bytesRead = await read(rid, v);
          if (bytesRead === null) {
            tryClose(rid);
            controller.close();
            controller.byobRequest.respond(0);
          } else {
            controller.byobRequest.respond(bytesRead);
          }
        } catch (e) {
          controller.error(e);
          tryClose(rid);
        }
      },
      cancel() {
        tryClose(rid);
      },
      autoAllocateChunkSize: DEFAULT_CHUNK_SIZE,
    });
  }

  function writableStreamForRid(rid) {
    return new WritableStream({
      async write(chunk, controller) {
        try {
          let nwritten = 0;
          while (nwritten < chunk.length) {
            nwritten += await write(
              rid,
              TypedArrayPrototypeSubarray(chunk, nwritten),
            );
          }
        } catch (e) {
          controller.error(e);
          tryClose(rid);
        }
      },
      close() {
        tryClose(rid);
      },
      abort() {
        tryClose(rid);
      },
    });
  }

  class Conn {
    #rid = 0;
    #remoteAddr = null;
    #localAddr = null;

    #readable;
    #writable;

    constructor(rid, remoteAddr, localAddr) {
      this.#rid = rid;
      this.#remoteAddr = remoteAddr;
      this.#localAddr = localAddr;
    }

    get rid() {
      return this.#rid;
    }

    get remoteAddr() {
      return this.#remoteAddr;
    }

    get localAddr() {
      return this.#localAddr;
    }

    write(p) {
      return write(this.rid, p);
    }

    read(p) {
      return read(this.rid, p);
    }

    close() {
      core.close(this.rid);
    }

    closeWrite() {
      return shutdown(this.rid);
    }

    get readable() {
      if (this.#readable === undefined) {
        this.#readable = readableStreamForRid(this.rid);
      }
      return this.#readable;
    }

    get writable() {
      if (this.#writable === undefined) {
        this.#writable = writableStreamForRid(this.rid);
      }
      return this.#writable;
    }
  }

  class TcpConn extends Conn {
    setNoDelay(nodelay = true) {
      return core.opSync("op_set_nodelay", this.rid, nodelay);
    }

    setKeepAlive(keepalive = true) {
      return core.opSync("op_set_keepalive", this.rid, keepalive);
    }
  }

  class UnixConn extends Conn {}

  class Listener {
    #rid = 0;
    #addr = null;

    constructor(rid, addr) {
      this.#rid = rid;
      this.#addr = addr;
    }

    get rid() {
      return this.#rid;
    }

    get addr() {
      return this.#addr;
    }

    async accept() {
      const res = await opAccept(this.rid, this.addr.transport);
      if (this.addr.transport == "tcp") {
        return new TcpConn(res.rid, res.remoteAddr, res.localAddr);
      } else if (this.addr.transport == "unix") {
        return new UnixConn(res.rid, res.remoteAddr, res.localAddr);
      } else {
        throw new Error("unreachable");
      }
    }

    async next() {
      let conn;
      try {
        conn = await this.accept();
      } catch (error) {
        if (
          ObjectPrototypeIsPrototypeOf(BadResourcePrototype, error) ||
          ObjectPrototypeIsPrototypeOf(InterruptedPrototype, error)
        ) {
          return { value: undefined, done: true };
        }
        throw error;
      }
      return { value: conn, done: false };
    }

    return(value) {
      this.close();
      return PromiseResolve({ value, done: true });
    }

    close() {
      core.close(this.rid);
    }

    [SymbolAsyncIterator]() {
      return this;
    }
  }

  class Datagram {
    #rid = 0;
    #addr = null;

    constructor(rid, addr, bufSize = 1024) {
      this.#rid = rid;
      this.#addr = addr;
      this.bufSize = bufSize;
    }

    get rid() {
      return this.#rid;
    }

    get addr() {
      return this.#addr;
    }

    async receive(p) {
      const buf = p || new Uint8Array(this.bufSize);
      const { size, remoteAddr } = await opReceive(
        this.rid,
        this.addr.transport,
        buf,
      );
      const sub = TypedArrayPrototypeSubarray(buf, 0, size);
      return [sub, remoteAddr];
    }

    send(p, addr) {
      const remote = { hostname: "127.0.0.1", ...addr };

      const args = { ...remote, rid: this.rid };
      return opSend(args, p);
    }

    close() {
      core.close(this.rid);
    }

    async *[SymbolAsyncIterator]() {
      while (true) {
        try {
          yield await this.receive();
        } catch (err) {
          if (
            ObjectPrototypeIsPrototypeOf(BadResourcePrototype, err) ||
            ObjectPrototypeIsPrototypeOf(InterruptedPrototype, err)
          ) {
            break;
          }
          throw err;
        }
      }
    }
  }

  function listen({ hostname, ...options }) {
    const res = opListen({
      transport: "tcp",
      hostname: typeof hostname === "undefined" ? "0.0.0.0" : hostname,
      ...options,
    });

    return new Listener(res.rid, res.localAddr);
  }

  async function connect(options) {
    if (options.transport === "unix") {
      const res = await opConnect(options);
      return new UnixConn(res.rid, res.remoteAddr, res.localAddr);
    }

    const res = await opConnect({
      transport: "tcp",
      hostname: "127.0.0.1",
      ...options,
    });
    return new TcpConn(res.rid, res.remoteAddr, res.localAddr);
  }

  window.__bootstrap.net = {
    connect,
    Conn,
    TcpConn,
    UnixConn,
    opConnect,
    listen,
    opListen,
    Listener,
    shutdown,
    Datagram,
    resolveDns,
  };
  window.__bootstrap.streamUtils = {
    readableStreamForRid,
    writableStreamForRid,
  };
})(this);
