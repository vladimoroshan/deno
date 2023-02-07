// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
const core = globalThis.Deno.core;
const ops = core.ops;
const primordials = globalThis.__bootstrap.primordials;
import { BlobPrototype } from "internal:deno_web/09_file.js";
import { TcpConn } from "internal:deno_net/01_net.js";
import { toInnerResponse } from "internal:deno_fetch/23_response.js";
import { _flash, fromFlashRequest } from "internal:deno_fetch/23_request.js";
import { Event } from "internal:deno_web/02_event.js";
import {
  _state,
  getReadableStreamResourceBacking,
  ReadableStream,
  readableStreamClose,
  ReadableStreamPrototype,
} from "internal:deno_web/06_streams.js";
import {
  _eventLoop,
  _idleTimeoutDuration,
  _idleTimeoutTimeout,
  _protocol,
  _readyState,
  _rid,
  _serverHandleIdleTimeout,
  WebSocket,
} from "internal:deno_websocket/01_websocket.js";
import { _ws } from "internal:deno_http/01_http.js";
const {
  ObjectPrototypeIsPrototypeOf,
  PromisePrototype,
  PromisePrototypeCatch,
  PromisePrototypeThen,
  SafePromiseAll,
  TypedArrayPrototypeSubarray,
  TypeError,
  Uint8Array,
  Uint8ArrayPrototype,
} = primordials;

const statusCodes = {
  100: "Continue",
  101: "Switching Protocols",
  102: "Processing",
  200: "OK",
  201: "Created",
  202: "Accepted",
  203: "Non Authoritative Information",
  204: "No Content",
  205: "Reset Content",
  206: "Partial Content",
  207: "Multi-Status",
  208: "Already Reported",
  226: "IM Used",
  300: "Multiple Choices",
  301: "Moved Permanently",
  302: "Found",
  303: "See Other",
  304: "Not Modified",
  305: "Use Proxy",
  307: "Temporary Redirect",
  308: "Permanent Redirect",
  400: "Bad Request",
  401: "Unauthorized",
  402: "Payment Required",
  403: "Forbidden",
  404: "Not Found",
  405: "Method Not Allowed",
  406: "Not Acceptable",
  407: "Proxy Authentication Required",
  408: "Request Timeout",
  409: "Conflict",
  410: "Gone",
  411: "Length Required",
  412: "Precondition Failed",
  413: "Payload Too Large",
  414: "URI Too Long",
  415: "Unsupported Media Type",
  416: "Range Not Satisfiable",
  418: "I'm a teapot",
  421: "Misdirected Request",
  422: "Unprocessable Entity",
  423: "Locked",
  424: "Failed Dependency",
  426: "Upgrade Required",
  428: "Precondition Required",
  429: "Too Many Requests",
  431: "Request Header Fields Too Large",
  451: "Unavailable For Legal Reasons",
  500: "Internal Server Error",
  501: "Not Implemented",
  502: "Bad Gateway",
  503: "Service Unavailable",
  504: "Gateway Timeout",
  505: "HTTP Version Not Supported",
  506: "Variant Also Negotiates",
  507: "Insufficient Storage",
  508: "Loop Detected",
  510: "Not Extended",
  511: "Network Authentication Required",
};

const methods = {
  0: "GET",
  1: "HEAD",
  2: "CONNECT",
  3: "PUT",
  4: "DELETE",
  5: "OPTIONS",
  6: "TRACE",
  7: "POST",
  8: "PATCH",
};

let dateInterval;
let date;

// Construct an HTTP response message.
// All HTTP/1.1 messages consist of a start-line followed by a sequence
// of octets.
//
//  HTTP-message = start-line
//    *( header-field CRLF )
//    CRLF
//    [ message-body ]
//
function http1Response(
  method,
  status,
  headerList,
  body,
  bodyLen,
  earlyEnd = false,
) {
  // HTTP uses a "<major>.<minor>" numbering scheme
  //   HTTP-version  = HTTP-name "/" DIGIT "." DIGIT
  //   HTTP-name     = %x48.54.54.50 ; "HTTP", case-sensitive
  //
  // status-line = HTTP-version SP status-code SP reason-phrase CRLF
  // Date header: https://datatracker.ietf.org/doc/html/rfc7231#section-7.1.1.2
  let str = `HTTP/1.1 ${status} ${statusCodes[status]}\r\nDate: ${date}\r\n`;
  for (let i = 0; i < headerList.length; ++i) {
    const { 0: name, 1: value } = headerList[i];
    // header-field   = field-name ":" OWS field-value OWS
    str += `${name}: ${value}\r\n`;
  }

  // https://datatracker.ietf.org/doc/html/rfc7231#section-6.3.6
  if (status === 205 || status === 304) {
    // MUST NOT generate a payload in a 205 response.
    // indicate a zero-length body for the response by
    // including a Content-Length header field with a value of 0.
    str += "Content-Length: 0\r\n\r\n";
    return str;
  }

  // MUST NOT send Content-Length or Transfer-Encoding if status code is 1xx or 204.
  if (status === 204 || status < 200) {
    str += "\r\n";
    return str;
  }

  if (earlyEnd === true) {
    return str;
  }

  // null body status is validated by inititalizeAResponse in ext/fetch
  if (body !== null && body !== undefined) {
    str += `Content-Length: ${bodyLen}\r\n\r\n`;
  } else {
    str += "Transfer-Encoding: chunked\r\n\r\n";
    return str;
  }

  // A HEAD request.
  if (method === 1) return str;

  if (typeof body === "string") {
    str += body ?? "";
  } else {
    const head = core.encode(str);
    const response = new Uint8Array(head.byteLength + body.byteLength);
    response.set(head, 0);
    response.set(body, head.byteLength);
    return response;
  }

  return str;
}

function prepareFastCalls() {
  return ops.op_flash_make_request();
}

function hostnameForDisplay(hostname) {
  // If the hostname is "0.0.0.0", we display "localhost" in console
  // because browsers in Windows don't resolve "0.0.0.0".
  // See the discussion in https://github.com/denoland/deno_std/issues/1165
  return hostname === "0.0.0.0" ? "localhost" : hostname;
}

function writeFixedResponse(
  server,
  requestId,
  response,
  responseLen,
  end,
  respondFast,
) {
  let nwritten = 0;
  // TypedArray
  if (typeof response !== "string") {
    nwritten = respondFast(requestId, response, end);
  } else {
    // string
    nwritten = ops.op_flash_respond(
      server,
      requestId,
      response,
      end,
    );
  }

  if (nwritten < responseLen) {
    core.opAsync(
      "op_flash_respond_async",
      server,
      requestId,
      response.slice(nwritten),
      end,
    );
  }
}

// TODO(@littledivy): Woah woah, cut down the number of arguments.
async function handleResponse(
  req,
  resp,
  body,
  hasBody,
  method,
  serverId,
  i,
  respondFast,
  respondChunked,
  tryRespondChunked,
) {
  // there might've been an HTTP upgrade.
  if (resp === undefined) {
    return;
  }
  const innerResp = toInnerResponse(resp);
  // If response body length is known, it will be sent synchronously in a
  // single op, in other case a "response body" resource will be created and
  // we'll be streaming it.
  /** @type {ReadableStream<Uint8Array> | Uint8Array | null} */
  let respBody = null;
  let isStreamingResponseBody = false;
  if (innerResp.body !== null) {
    if (typeof innerResp.body.streamOrStatic?.body === "string") {
      if (innerResp.body.streamOrStatic.consumed === true) {
        throw new TypeError("Body is unusable.");
      }
      innerResp.body.streamOrStatic.consumed = true;
      respBody = innerResp.body.streamOrStatic.body;
      isStreamingResponseBody = false;
    } else if (
      ObjectPrototypeIsPrototypeOf(
        ReadableStreamPrototype,
        innerResp.body.streamOrStatic,
      )
    ) {
      if (innerResp.body.unusable()) {
        throw new TypeError("Body is unusable.");
      }
      if (
        innerResp.body.length === null ||
        ObjectPrototypeIsPrototypeOf(
          BlobPrototype,
          innerResp.body.source,
        )
      ) {
        respBody = innerResp.body.stream;
      } else {
        const reader = innerResp.body.stream.getReader();
        const r1 = await reader.read();
        if (r1.done) {
          respBody = new Uint8Array(0);
        } else {
          respBody = r1.value;
          const r2 = await reader.read();
          if (!r2.done) throw new TypeError("Unreachable");
        }
      }
      isStreamingResponseBody = !(
        typeof respBody === "string" ||
        ObjectPrototypeIsPrototypeOf(Uint8ArrayPrototype, respBody)
      );
    } else {
      if (innerResp.body.streamOrStatic.consumed === true) {
        throw new TypeError("Body is unusable.");
      }
      innerResp.body.streamOrStatic.consumed = true;
      respBody = innerResp.body.streamOrStatic.body;
    }
  } else {
    respBody = new Uint8Array(0);
  }

  const ws = resp[_ws];
  if (isStreamingResponseBody === false) {
    const length = respBody.byteLength || core.byteLength(respBody);
    const responseStr = http1Response(
      method,
      innerResp.status ?? 200,
      innerResp.headerList,
      respBody,
      length,
    );
    writeFixedResponse(
      serverId,
      i,
      responseStr,
      length,
      !ws, // Don't close socket if there is a deferred websocket upgrade.
      respondFast,
    );
  }

  return (async () => {
    if (!ws) {
      if (hasBody && body[_state] !== "closed") {
        // TODO(@littledivy): Optimize by draining in a single op.
        try {
          await req.arrayBuffer();
        } catch { /* pass */ }
      }
    }

    if (isStreamingResponseBody === true) {
      const resourceBacking = getReadableStreamResourceBacking(respBody);
      if (resourceBacking) {
        if (respBody.locked) {
          throw new TypeError("ReadableStream is locked.");
        }
        const reader = respBody.getReader(); // Aquire JS lock.
        try {
          PromisePrototypeThen(
            core.opAsync(
              "op_flash_write_resource",
              http1Response(
                method,
                innerResp.status ?? 200,
                innerResp.headerList,
                0, // Content-Length will be set by the op.
                null,
                true,
              ),
              serverId,
              i,
              resourceBacking.rid,
              resourceBacking.autoClose,
            ),
            () => {
              // Release JS lock.
              readableStreamClose(respBody);
            },
          );
        } catch (error) {
          await reader.cancel(error);
          throw error;
        }
      } else {
        const reader = respBody.getReader();

        // Best case: sends headers + first chunk in a single go.
        const { value, done } = await reader.read();
        writeFixedResponse(
          serverId,
          i,
          http1Response(
            method,
            innerResp.status ?? 200,
            innerResp.headerList,
            respBody.byteLength,
            null,
          ),
          respBody.byteLength,
          false,
          respondFast,
        );

        await tryRespondChunked(
          i,
          value,
          done,
        );

        if (!done) {
          while (true) {
            const chunk = await reader.read();
            await respondChunked(
              i,
              chunk.value,
              chunk.done,
            );
            if (chunk.done) break;
          }
        }
      }
    }

    if (ws) {
      const wsRid = await core.opAsync(
        "op_flash_upgrade_websocket",
        serverId,
        i,
      );
      ws[_rid] = wsRid;
      ws[_protocol] = resp.headers.get("sec-websocket-protocol");

      ws[_readyState] = WebSocket.OPEN;
      const event = new Event("open");
      ws.dispatchEvent(event);

      ws[_eventLoop]();
      if (ws[_idleTimeoutDuration]) {
        ws.addEventListener(
          "close",
          () => clearTimeout(ws[_idleTimeoutTimeout]),
        );
      }
      ws[_serverHandleIdleTimeout]();
    }
  })();
}

function createServe(opFn) {
  return async function serve(arg1, arg2) {
    let options = undefined;
    let handler = undefined;
    if (typeof arg1 === "function") {
      handler = arg1;
      options = arg2;
    } else if (typeof arg2 === "function") {
      handler = arg2;
      options = arg1;
    } else {
      options = arg1;
    }
    if (handler === undefined) {
      if (options === undefined) {
        throw new TypeError(
          "No handler was provided, so an options bag is mandatory.",
        );
      }
      handler = options.handler;
    }
    if (typeof handler !== "function") {
      throw new TypeError("A handler function must be provided.");
    }
    if (options === undefined) {
      options = {};
    }

    const signal = options.signal;

    const onError = options.onError ?? function (error) {
      console.error(error);
      return new Response("Internal Server Error", { status: 500 });
    };

    const onListen = options.onListen ?? function ({ port }) {
      console.log(
        `Listening on http://${
          hostnameForDisplay(listenOpts.hostname)
        }:${port}/`,
      );
    };

    const listenOpts = {
      hostname: options.hostname ?? "127.0.0.1",
      port: options.port ?? 9000,
      reuseport: options.reusePort ?? false,
    };
    if (options.cert || options.key) {
      if (!options.cert || !options.key) {
        throw new TypeError(
          "Both cert and key must be provided to enable HTTPS.",
        );
      }
      listenOpts.cert = options.cert;
      listenOpts.key = options.key;
    }

    const serverId = opFn(listenOpts);
    const serverPromise = core.opAsync("op_flash_drive_server", serverId);

    PromisePrototypeCatch(
      PromisePrototypeThen(
        core.opAsync("op_flash_wait_for_listening", serverId),
        (port) => {
          onListen({ hostname: listenOpts.hostname, port });
        },
      ),
      () => {},
    );
    const finishedPromise = PromisePrototypeCatch(serverPromise, () => {});

    const server = {
      id: serverId,
      transport: listenOpts.cert && listenOpts.key ? "https" : "http",
      hostname: listenOpts.hostname,
      port: listenOpts.port,
      closed: false,
      finished: finishedPromise,
      async close() {
        if (server.closed) {
          return;
        }
        server.closed = true;
        await core.opAsync("op_flash_close_server", serverId);
        await server.finished;
      },
      async serve() {
        let offset = 0;
        while (true) {
          if (server.closed) {
            break;
          }

          let tokens = nextRequestSync();
          if (tokens === 0) {
            tokens = await core.opAsync("op_flash_next_async", serverId);
            if (server.closed) {
              break;
            }
          }

          for (let i = offset; i < offset + tokens; i++) {
            let body = null;
            // There might be a body, but we don't expose it for GET/HEAD requests.
            // It will be closed automatically once the request has been handled and
            // the response has been sent.
            const method = getMethodSync(i);
            let hasBody = method > 2; // Not GET/HEAD/CONNECT
            if (hasBody) {
              body = createRequestBodyStream(serverId, i);
              if (body === null) {
                hasBody = false;
              }
            }

            const req = fromFlashRequest(
              serverId,
              /* streamRid */
              i,
              body,
              /* methodCb */
              () => methods[method],
              /* urlCb */
              () => {
                const path = ops.op_flash_path(serverId, i);
                return `${server.transport}://${server.hostname}:${server.port}${path}`;
              },
              /* headersCb */
              () => ops.op_flash_headers(serverId, i),
            );

            let resp;
            try {
              resp = handler(req);
              if (ObjectPrototypeIsPrototypeOf(PromisePrototype, resp)) {
                PromisePrototypeCatch(
                  PromisePrototypeThen(
                    resp,
                    (resp) =>
                      handleResponse(
                        req,
                        resp,
                        body,
                        hasBody,
                        method,
                        serverId,
                        i,
                        respondFast,
                        respondChunked,
                        tryRespondChunked,
                      ),
                  ),
                  onError,
                );
              } else if (typeof resp?.then === "function") {
                resp.then((resp) =>
                  handleResponse(
                    req,
                    resp,
                    body,
                    hasBody,
                    method,
                    serverId,
                    i,
                    respondFast,
                    respondChunked,
                    tryRespondChunked,
                  )
                ).catch(onError);
              } else {
                handleResponse(
                  req,
                  resp,
                  body,
                  hasBody,
                  method,
                  serverId,
                  i,
                  respondFast,
                  respondChunked,
                  tryRespondChunked,
                ).catch(onError);
              }
            } catch (e) {
              resp = await onError(e);
            }
          }

          offset += tokens;
        }
        await server.finished;
      },
    };

    signal?.addEventListener("abort", () => {
      clearInterval(dateInterval);
      PromisePrototypeThen(server.close(), () => {}, () => {});
    }, {
      once: true,
    });

    function tryRespondChunked(token, chunk, shutdown) {
      const nwritten = ops.op_try_flash_respond_chunked(
        serverId,
        token,
        chunk ?? new Uint8Array(),
        shutdown,
      );
      if (nwritten > 0) {
        return core.opAsync(
          "op_flash_respond_chunked",
          serverId,
          token,
          chunk,
          shutdown,
          nwritten,
        );
      }
    }

    function respondChunked(token, chunk, shutdown) {
      return core.opAsync(
        "op_flash_respond_chunked",
        serverId,
        token,
        chunk,
        shutdown,
      );
    }

    const fastOp = prepareFastCalls();
    let nextRequestSync = () => fastOp.nextRequest();
    let getMethodSync = (token) => fastOp.getMethod(token);
    let respondFast = (token, response, shutdown) =>
      fastOp.respond(token, response, shutdown);
    if (serverId > 0) {
      nextRequestSync = () => ops.op_flash_next_server(serverId);
      getMethodSync = (token) => ops.op_flash_method(serverId, token);
      respondFast = (token, response, shutdown) =>
        ops.op_flash_respond(serverId, token, response, null, shutdown);
    }

    if (!dateInterval) {
      date = new Date().toUTCString();
      dateInterval = setInterval(() => {
        date = new Date().toUTCString();
      }, 1000);
    }

    await SafePromiseAll([
      PromisePrototypeCatch(server.serve(), console.error),
      serverPromise,
    ]);
  };
}

function createRequestBodyStream(serverId, token) {
  // The first packet is left over bytes after parsing the request
  const firstRead = ops.op_flash_first_packet(
    serverId,
    token,
  );
  if (!firstRead) return null;
  let firstEnqueued = firstRead.byteLength == 0;

  return new ReadableStream({
    type: "bytes",
    async pull(controller) {
      try {
        if (firstEnqueued === false) {
          controller.enqueue(firstRead);
          firstEnqueued = true;
          return;
        }
        // This is the largest possible size for a single packet on a TLS
        // stream.
        const chunk = new Uint8Array(16 * 1024 + 256);
        const read = await core.opAsync(
          "op_flash_read_body",
          serverId,
          token,
          chunk,
        );
        if (read > 0) {
          // We read some data. Enqueue it onto the stream.
          controller.enqueue(TypedArrayPrototypeSubarray(chunk, 0, read));
        } else {
          // We have reached the end of the body, so we close the stream.
          controller.close();
        }
      } catch (err) {
        // There was an error while reading a chunk of the body, so we
        // error.
        controller.error(err);
        controller.close();
      }
    },
  });
}

function upgradeHttpRaw(req) {
  if (!req[_flash]) {
    throw new TypeError(
      "Non-flash requests can not be upgraded with `upgradeHttpRaw`. Use `upgradeHttp` instead.",
    );
  }

  // NOTE(bartlomieju):
  // Access these fields so they are cached on `req` object, otherwise
  // they wouldn't be available after the connection gets upgraded.
  req.url;
  req.method;
  req.headers;

  const { serverId, streamRid } = req[_flash];
  const connRid = ops.op_flash_upgrade_http(streamRid, serverId);
  // TODO(@littledivy): return already read first packet too.
  return [new TcpConn(connRid), new Uint8Array()];
}

export { createServe, upgradeHttpRaw };
