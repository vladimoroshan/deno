// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

// @ts-check
/// <reference path="../webidl/internal.d.ts" />
/// <reference path="../web/internal.d.ts" />
/// <reference path="../file/internal.d.ts" />
/// <reference path="../file/lib.deno_file.d.ts" />
/// <reference path="./internal.d.ts" />
/// <reference path="./11_streams_types.d.ts" />
/// <reference path="./lib.deno_fetch.d.ts" />
/// <reference lib="esnext" />
"use strict";

((window) => {
  const webidl = window.__bootstrap.webidl;
  const { HTTP_TOKEN_CODE_POINT_RE, byteUpperCase } = window.__bootstrap.infra;
  const { URL } = window.__bootstrap.url;
  const { guardFromHeaders } = window.__bootstrap.headers;
  const { mixinBody, extractBody } = window.__bootstrap.fetchBody;
  const { getLocationHref } = window.__bootstrap.location;
  const mimesniff = window.__bootstrap.mimesniff;
  const {
    headersFromHeaderList,
    headerListFromHeaders,
    fillHeaders,
    getDecodeSplitHeader,
  } = window.__bootstrap.headers;
  const { HttpClient } = window.__bootstrap.fetch;
  const abortSignal = window.__bootstrap.abortSignal;

  const _request = Symbol("request");
  const _headers = Symbol("headers");
  const _signal = Symbol("signal");
  const _mimeType = Symbol("mime type");
  const _body = Symbol("body");

  /**
   * @typedef InnerRequest
   * @property {string} method
   * @property {() => string} url
   * @property {() => string} currentUrl
   * @property {[string, string][]} headerList
   * @property {null | typeof __window.bootstrap.fetchBody.InnerBody} body
   * @property {"follow" | "error" | "manual"} redirectMode
   * @property {number} redirectCount
   * @property {string[]} urlList
   * @property {number | null} clientRid NOTE: non standard extension for `Deno.HttpClient`.
   */

  const defaultInnerRequest = {
    url() {
      return this.urlList[0];
    },
    currentUrl() {
      return this.urlList[this.urlList.length - 1];
    },
    redirectMode: "follow",
    redirectCount: 0,
    clientRid: null,
  };

  /**
   * @param {string} method
   * @param {string} url
   * @param {[string, string][]} headerList
   * @param {typeof __window.bootstrap.fetchBody.InnerBody} body
   * @returns
   */
  function newInnerRequest(method, url, headerList = [], body = null) {
    return {
      method: method,
      headerList,
      body,
      urlList: [url],
      ...defaultInnerRequest,
    };
  }

  /**
   * https://fetch.spec.whatwg.org/#concept-request-clone
   * @param {InnerRequest} request
   * @returns {InnerRequest}
   */
  function cloneInnerRequest(request) {
    const headerList = [...request.headerList.map((x) => [x[0], x[1]])];
    let body = null;
    if (request.body !== null) {
      body = request.body.clone();
    }

    return {
      method: request.method,
      url() {
        return this.urlList[0];
      },
      currentUrl() {
        return this.urlList[this.urlList.length - 1];
      },
      headerList,
      body,
      redirectMode: request.redirectMode,
      redirectCount: request.redirectCount,
      urlList: request.urlList,
      clientRid: request.clientRid,
    };
  }

  /**
   * @param {string} m
   * @returns {boolean}
   */
  function isKnownMethod(m) {
    return (
      m === "DELETE" ||
      m === "GET" ||
      m === "HEAD" ||
      m === "OPTIONS" ||
      m === "POST" ||
      m === "PUT"
    );
  }
  /**
   * @param {string} m
   * @returns {string}
   */
  function validateAndNormalizeMethod(m) {
    // Fast path for well-known methods
    if (isKnownMethod(m)) {
      return m;
    }

    // Regular path
    if (!HTTP_TOKEN_CODE_POINT_RE.test(m)) {
      throw new TypeError("Method is not valid.");
    }
    const upperCase = byteUpperCase(m);
    if (
      upperCase === "CONNECT" || upperCase === "TRACE" || upperCase === "TRACK"
    ) {
      throw new TypeError("Method is forbidden.");
    }
    return upperCase;
  }

  class Request {
    /** @type {InnerRequest} */
    [_request];
    /** @type {Headers} */
    [_headers];
    /** @type {AbortSignal} */
    [_signal];
    get [_mimeType]() {
      let charset = null;
      let essence = null;
      let mimeType = null;
      const values = getDecodeSplitHeader(
        headerListFromHeaders(this[_headers]),
        "Content-Type",
      );
      if (values === null) return null;
      for (const value of values) {
        const temporaryMimeType = mimesniff.parseMimeType(value);
        if (
          temporaryMimeType === null ||
          mimesniff.essence(temporaryMimeType) == "*/*"
        ) {
          continue;
        }
        mimeType = temporaryMimeType;
        if (mimesniff.essence(mimeType) !== essence) {
          charset = null;
          const newCharset = mimeType.parameters.get("charset");
          if (newCharset !== undefined) {
            charset = newCharset;
          }
          essence = mimesniff.essence(mimeType);
        } else {
          if (mimeType.parameters.has("charset") === null && charset !== null) {
            mimeType.parameters.set("charset", charset);
          }
        }
      }
      if (mimeType === null) return null;
      return mimeType;
    }
    get [_body]() {
      return this[_request].body;
    }

    /**
     * https://fetch.spec.whatwg.org/#dom-request
     * @param {RequestInfo} input
     * @param {RequestInit} init
     */
    constructor(input, init = {}) {
      const prefix = "Failed to construct 'Request'";
      webidl.requiredArguments(arguments.length, 1, { prefix });
      input = webidl.converters["RequestInfo"](input, {
        prefix,
        context: "Argument 1",
      });
      init = webidl.converters["RequestInit"](init, {
        prefix,
        context: "Argument 2",
      });

      this[webidl.brand] = webidl.brand;

      /** @type {InnerRequest} */
      let request;
      const baseURL = getLocationHref();

      // 4.
      let signal = null;

      // 5.
      if (typeof input === "string") {
        const parsedURL = new URL(input, baseURL);
        request = newInnerRequest("GET", parsedURL.href, [], null);
      } else { // 6.
        if (!(input instanceof Request)) throw new TypeError("Unreachable");
        request = input[_request];
        signal = input[_signal];
      }

      // 12.
      // TODO(lucacasonato): create a copy of `request`

      // 22.
      if (init.redirect !== undefined) {
        request.redirectMode = init.redirect;
      }

      // 25.
      if (init.method !== undefined) {
        let method = init.method;
        method = validateAndNormalizeMethod(method);
        request.method = method;
      }

      // 26.
      if (init.signal !== undefined) {
        signal = init.signal;
      }

      // NOTE: non standard extension. This handles Deno.HttpClient parameter
      if (init.client !== undefined) {
        if (init.client !== null && !(init.client instanceof HttpClient)) {
          throw webidl.makeException(
            TypeError,
            "`client` must be a Deno.HttpClient",
            { prefix, context: "Argument 2" },
          );
        }
        request.clientRid = init.client?.rid ?? null;
      }

      // 27.
      this[_request] = request;

      // 28.
      this[_signal] = abortSignal.newSignal();
      if (signal !== null) {
        abortSignal.follow(this[_signal], signal);
      }

      // 29.
      this[_headers] = headersFromHeaderList(request.headerList, "request");

      // 31.
      if (Object.keys(init).length > 0) {
        let headers = headerListFromHeaders(this[_headers]).slice(
          0,
          headerListFromHeaders(this[_headers]).length,
        );
        if (init.headers !== undefined) {
          headers = init.headers;
        }
        headerListFromHeaders(this[_headers]).splice(
          0,
          headerListFromHeaders(this[_headers]).length,
        );
        fillHeaders(this[_headers], headers);
      }

      // 32.
      let inputBody = null;
      if (input instanceof Request) {
        inputBody = input[_body];
      }

      // 33.
      if (
        (request.method === "GET" || request.method === "HEAD") &&
        ((init.body !== undefined && init.body !== null) ||
          inputBody !== null)
      ) {
        throw new TypeError("Request with GET/HEAD method cannot have body.");
      }

      // 34.
      let initBody = null;

      // 35.
      if (init.body !== undefined && init.body !== null) {
        const res = extractBody(init.body);
        initBody = res.body;
        if (res.contentType !== null && !this[_headers].has("content-type")) {
          this[_headers].append("Content-Type", res.contentType);
        }
      }

      // 36.
      const inputOrInitBody = initBody ?? inputBody;

      // 38.
      const finalBody = inputOrInitBody;

      // 39.
      // TODO(lucacasonato): implement this step. Is it needed?

      // 40.
      request.body = finalBody;

      // 41.
      // TODO(lucacasonato): Extranious? https://github.com/whatwg/fetch/issues/1249
    }

    get method() {
      webidl.assertBranded(this, Request);
      return this[_request].method;
    }

    get url() {
      webidl.assertBranded(this, Request);
      return this[_request].url();
    }

    get headers() {
      webidl.assertBranded(this, Request);
      return this[_headers];
    }

    get redirect() {
      webidl.assertBranded(this, Request);
      return this[_request].redirectMode;
    }

    get signal() {
      webidl.assertBranded(this, Request);
      return this[_signal];
    }

    clone() {
      webidl.assertBranded(this, Request);
      if (this[_body] && this[_body].unusable()) {
        throw new TypeError("Body is unusable.");
      }
      const newReq = cloneInnerRequest(this[_request]);
      const newSignal = abortSignal.newSignal();
      abortSignal.follow(newSignal, this[_signal]);
      return fromInnerRequest(
        newReq,
        newSignal,
        guardFromHeaders(this[_headers]),
      );
    }

    get [Symbol.toStringTag]() {
      return "Request";
    }

    [Symbol.for("Deno.customInspect")](inspect) {
      const inner = {
        bodyUsed: this.bodyUsed,
        headers: this.headers,
        method: this.method,
        redirect: this.redirect,
        url: this.url,
      };
      return `Request ${inspect(inner)}`;
    }
  }

  mixinBody(Request, _body, _mimeType);

  webidl.configurePrototype(Request);

  webidl.converters["Request"] = webidl.createInterfaceConverter(
    "Request",
    Request,
  );
  webidl.converters["RequestInfo"] = (V, opts) => {
    // Union for (Request or USVString)
    if (typeof V == "object") {
      if (V instanceof Request) {
        return webidl.converters["Request"](V, opts);
      }
    }
    return webidl.converters["USVString"](V, opts);
  };
  webidl.converters["RequestRedirect"] = webidl.createEnumConverter(
    "RequestRedirect",
    [
      "follow",
      "error",
      "manual",
    ],
  );
  webidl.converters["RequestInit"] = webidl.createDictionaryConverter(
    "RequestInit",
    [
      { key: "method", converter: webidl.converters["ByteString"] },
      { key: "headers", converter: webidl.converters["HeadersInit"] },
      {
        key: "body",
        converter: webidl.createNullableConverter(
          webidl.converters["BodyInit"],
        ),
      },
      { key: "redirect", converter: webidl.converters["RequestRedirect"] },
      {
        key: "signal",
        converter: webidl.createNullableConverter(
          webidl.converters["AbortSignal"],
        ),
      },
      { key: "client", converter: webidl.converters.any },
    ],
  );

  /**
   * @param {Request} request
   * @returns {InnerRequest}
   */
  function toInnerRequest(request) {
    return request[_request];
  }

  /**
   * @param {InnerRequest} inner
   * @param {"request" | "immutable" | "request-no-cors" | "response" | "none"} guard
   * @returns {Request}
   */
  function fromInnerRequest(inner, signal, guard) {
    const request = webidl.createBranded(Request);
    request[_request] = inner;
    request[_signal] = signal;
    request[_headers] = headersFromHeaderList(inner.headerList, guard);
    return request;
  }

  window.__bootstrap.fetch ??= {};
  window.__bootstrap.fetch.Request = Request;
  window.__bootstrap.fetch.toInnerRequest = toInnerRequest;
  window.__bootstrap.fetch.fromInnerRequest = fromInnerRequest;
  window.__bootstrap.fetch.newInnerRequest = newInnerRequest;
})(globalThis);
