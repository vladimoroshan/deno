// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
"use strict";

((window) => {
  const core = window.Deno.core;

  // provided by "deno_web"
  const { URL } = window.__bootstrap.url;

  const CONNECTING = 0;
  const OPEN = 1;
  const CLOSING = 2;
  const CLOSED = 3;

  function requiredArguments(
    name,
    length,
    required,
  ) {
    if (length < required) {
      const errMsg = `${name} requires at least ${required} argument${
        required === 1 ? "" : "s"
      }, but only ${length} present`;
      throw new TypeError(errMsg);
    }
  }

  /**
   * Tries to close the resource (and ignores BadResource errors).
   * @param {number} rid
   */
  function tryClose(rid) {
    try {
      core.close(rid);
    } catch (err) {
      // Ignore error if the socket has already been closed.
      if (!(err instanceof Deno.errors.BadResource)) throw err;
    }
  }

  const handlerSymbol = Symbol("eventHandlers");
  function makeWrappedHandler(handler) {
    function wrappedHandler(...args) {
      if (typeof wrappedHandler.handler !== "function") {
        return;
      }
      return wrappedHandler.handler.call(this, ...args);
    }
    wrappedHandler.handler = handler;
    return wrappedHandler;
  }
  // TODO(lucacasonato) reuse when we can reuse code between web crates
  function defineEventHandler(emitter, name) {
    // HTML specification section 8.1.5.1
    Object.defineProperty(emitter, `on${name}`, {
      get() {
        return this[handlerSymbol]?.get(name)?.handler;
      },
      set(value) {
        if (!this[handlerSymbol]) {
          this[handlerSymbol] = new Map();
        }
        let handlerWrapper = this[handlerSymbol]?.get(name);
        if (handlerWrapper) {
          handlerWrapper.handler = value;
        } else {
          handlerWrapper = makeWrappedHandler(value);
          this.addEventListener(name, handlerWrapper);
        }
        this[handlerSymbol].set(name, handlerWrapper);
      },
      configurable: true,
      enumerable: true,
    });
  }

  class WebSocket extends EventTarget {
    #readyState = CONNECTING;

    constructor(url, protocols = []) {
      super();
      requiredArguments("WebSocket", arguments.length, 1);

      const wsURL = new URL(url);

      if (wsURL.protocol !== "ws:" && wsURL.protocol !== "wss:") {
        throw new DOMException(
          "Only ws & wss schemes are allowed in a WebSocket URL.",
          "SyntaxError",
        );
      }

      if (wsURL.hash !== "" || wsURL.href.endsWith("#")) {
        throw new DOMException(
          "Fragments are not allowed in a WebSocket URL.",
          "SyntaxError",
        );
      }

      this.#url = wsURL.href;

      core.opSync("op_ws_check_permission", this.#url);

      if (protocols && typeof protocols === "string") {
        protocols = [protocols];
      }

      if (
        protocols.some((x) => protocols.indexOf(x) !== protocols.lastIndexOf(x))
      ) {
        throw new DOMException(
          "Can't supply multiple times the same protocol.",
          "SyntaxError",
        );
      }

      core.opAsync("op_ws_create", {
        url: wsURL.href,
        protocols: protocols.join(", "),
      }).then((create) => {
        if (create.success) {
          this.#rid = create.rid;
          this.#extensions = create.extensions;
          this.#protocol = create.protocol;

          if (this.#readyState === CLOSING) {
            core.opAsync("op_ws_close", {
              rid: this.#rid,
            }).then(() => {
              this.#readyState = CLOSED;

              const errEvent = new ErrorEvent("error");
              errEvent.target = this;
              this.dispatchEvent(errEvent);

              const event = new CloseEvent("close");
              event.target = this;
              this.dispatchEvent(event);
              tryClose(this.#rid);
            });
          } else {
            this.#readyState = OPEN;
            const event = new Event("open");
            event.target = this;
            this.dispatchEvent(event);

            this.#eventLoop();
          }
        } else {
          this.#readyState = CLOSED;

          const errEvent = new ErrorEvent("error");
          errEvent.target = this;
          this.dispatchEvent(errEvent);

          const closeEvent = new CloseEvent("close");
          closeEvent.target = this;
          this.dispatchEvent(closeEvent);
        }
      }).catch((err) => {
        this.#readyState = CLOSED;

        const errorEv = new ErrorEvent(
          "error",
          { error: err, message: err.toString() },
        );
        errorEv.target = this;
        this.dispatchEvent(errorEv);

        const closeEv = new CloseEvent("close");
        closeEv.target = this;
        this.dispatchEvent(closeEv);
      });
    }

    get CONNECTING() {
      return CONNECTING;
    }
    get OPEN() {
      return OPEN;
    }
    get CLOSING() {
      return CLOSING;
    }
    get CLOSED() {
      return CLOSED;
    }

    get readyState() {
      return this.#readyState;
    }

    #extensions = "";
    #protocol = "";
    #url = "";
    #rid;

    get extensions() {
      return this.#extensions;
    }
    get protocol() {
      return this.#protocol;
    }

    #binaryType = "blob";
    get binaryType() {
      return this.#binaryType;
    }
    set binaryType(value) {
      if (value === "blob" || value === "arraybuffer") {
        this.#binaryType = value;
      }
    }
    #bufferedAmount = 0;
    get bufferedAmount() {
      return this.#bufferedAmount;
    }

    get url() {
      return this.#url;
    }

    send(data) {
      requiredArguments("WebSocket.send", arguments.length, 1);

      if (this.#readyState != OPEN) {
        throw Error("readyState not OPEN");
      }

      const sendTypedArray = (ta) => {
        this.#bufferedAmount += ta.size;
        core.opAsync("op_ws_send", {
          rid: this.#rid,
          kind: "binary",
        }, ta).then(() => {
          this.#bufferedAmount -= ta.size;
        });
      };

      if (data instanceof Blob) {
        data.slice().arrayBuffer().then((ab) =>
          sendTypedArray(new DataView(ab))
        );
      } else if (
        data instanceof Int8Array || data instanceof Int16Array ||
        data instanceof Int32Array || data instanceof Uint8Array ||
        data instanceof Uint16Array || data instanceof Uint32Array ||
        data instanceof Uint8ClampedArray || data instanceof Float32Array ||
        data instanceof Float64Array || data instanceof DataView
      ) {
        sendTypedArray(data);
      } else if (data instanceof ArrayBuffer) {
        sendTypedArray(new DataView(data));
      } else {
        const string = String(data);
        const encoder = new TextEncoder();
        const d = encoder.encode(string);
        this.#bufferedAmount += d.size;
        core.opAsync("op_ws_send", {
          rid: this.#rid,
          kind: "text",
          text: string,
        }).then(() => {
          this.#bufferedAmount -= d.size;
        });
      }
    }

    close(code, reason) {
      if (code && !(code === 1000 || (3000 <= code && code < 5000))) {
        throw new DOMException(
          "The close code must be either 1000 or in the range of 3000 to 4999.",
          "NotSupportedError",
        );
      }

      const encoder = new TextEncoder();
      if (reason && encoder.encode(reason).byteLength > 123) {
        throw new DOMException(
          "The close reason may not be longer than 123 bytes.",
          "SyntaxError",
        );
      }

      if (this.#readyState === CONNECTING) {
        this.#readyState = CLOSING;
      } else if (this.#readyState === OPEN) {
        this.#readyState = CLOSING;

        core.opAsync("op_ws_close", {
          rid: this.#rid,
          code,
          reason,
        }).then(() => {
          this.#readyState = CLOSED;
          const event = new CloseEvent("close", {
            wasClean: true,
            code,
            reason,
          });
          event.target = this;
          this.dispatchEvent(event);
          tryClose(this.#rid);
        });
      }
    }

    async #eventLoop() {
      while (this.#readyState === OPEN) {
        const message = await core.opAsync(
          "op_ws_next_event",
          this.#rid,
        );

        if ("string" in message) {
          const event = new MessageEvent("message", {
            data: message.string,
            origin: this.#url,
          });
          event.target = this;
          this.dispatchEvent(event);
        } else if ("binary" in message) {
          let data;

          if (this.binaryType === "blob") {
            data = new Blob([new Uint8Array(message.binary)]);
          } else {
            data = new Uint8Array(message.binary).buffer;
          }

          const event = new MessageEvent("message", {
            data,
            origin: this.#url,
          });
          event.target = this;
          this.dispatchEvent(event);
        } else if ("ping" in message) {
          core.opAsync("op_ws_send", {
            rid: this.#rid,
            kind: "pong",
          });
        } else if ("close" in message) {
          this.#readyState = CLOSED;

          const event = new CloseEvent("close", {
            wasClean: true,
            code: message.close.code,
            reason: message.close.reason,
          });
          event.target = this;
          this.dispatchEvent(event);
          tryClose(this.#rid);
        } else if ("error" in message) {
          this.#readyState = CLOSED;

          const errorEv = new ErrorEvent("error");
          errorEv.target = this;
          this.dispatchEvent(errorEv);

          const closeEv = new CloseEvent("close");
          closeEv.target = this;
          this.dispatchEvent(closeEv);
          tryClose(this.#rid);
        }
      }
    }
  }

  Object.defineProperties(WebSocket, {
    CONNECTING: {
      value: 0,
    },
    OPEN: {
      value: 1,
    },
    CLOSING: {
      value: 2,
    },
    CLOSED: {
      value: 3,
    },
  });

  defineEventHandler(WebSocket.prototype, "message");
  defineEventHandler(WebSocket.prototype, "error");
  defineEventHandler(WebSocket.prototype, "close");
  defineEventHandler(WebSocket.prototype, "open");

  window.__bootstrap.webSocket = { WebSocket };
})(this);
