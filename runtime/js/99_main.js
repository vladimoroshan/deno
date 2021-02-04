// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
// Removes the `__proto__` for security reasons.  This intentionally makes
// Deno non compliant with ECMA-262 Annex B.2.2.1
//
"use strict";
delete Object.prototype.__proto__;

((window) => {
  const core = Deno.core;
  const util = window.__bootstrap.util;
  const eventTarget = window.__bootstrap.eventTarget;
  const globalInterfaces = window.__bootstrap.globalInterfaces;
  const location = window.__bootstrap.location;
  const dispatchMinimal = window.__bootstrap.dispatchMinimal;
  const build = window.__bootstrap.build;
  const version = window.__bootstrap.version;
  const errorStack = window.__bootstrap.errorStack;
  const os = window.__bootstrap.os;
  const timers = window.__bootstrap.timers;
  const Console = window.__bootstrap.console.Console;
  const worker = window.__bootstrap.worker;
  const signals = window.__bootstrap.signals;
  const { internalSymbol, internalObject } = window.__bootstrap.internals;
  const performance = window.__bootstrap.performance;
  const crypto = window.__bootstrap.crypto;
  const url = window.__bootstrap.url;
  const headers = window.__bootstrap.headers;
  const streams = window.__bootstrap.streams;
  const fileReader = window.__bootstrap.fileReader;
  const webSocket = window.__bootstrap.webSocket;
  const file = window.__bootstrap.file;
  const fetch = window.__bootstrap.fetch;
  const prompt = window.__bootstrap.prompt;
  const denoNs = window.__bootstrap.denoNs;
  const denoNsUnstable = window.__bootstrap.denoNsUnstable;
  const errors = window.__bootstrap.errors.errors;
  const { defineEventHandler } = window.__bootstrap.webUtil;

  let windowIsClosing = false;

  function windowClose() {
    if (!windowIsClosing) {
      windowIsClosing = true;
      // Push a macrotask to exit after a promise resolve.
      // This is not perfect, but should be fine for first pass.
      Promise.resolve().then(() =>
        timers.setTimeout.call(
          null,
          () => {
            // This should be fine, since only Window/MainWorker has .close()
            os.exit(0);
          },
          0,
        )
      );
    }
  }

  const encoder = new TextEncoder();

  function workerClose() {
    if (isClosing) {
      return;
    }

    isClosing = true;
    opCloseWorker();
  }

  // TODO(bartlomieju): remove these functions
  // Stuff for workers
  const onmessage = () => {};
  const onerror = () => {};

  function postMessage(data) {
    const dataJson = JSON.stringify(data);
    const dataIntArray = encoder.encode(dataJson);
    opPostMessage(dataIntArray);
  }

  let isClosing = false;
  async function workerMessageRecvCallback(data) {
    const msgEvent = new MessageEvent("message", {
      cancelable: false,
      data,
    });

    try {
      if (globalThis["onmessage"]) {
        const result = globalThis.onmessage(msgEvent);
        if (result && "then" in result) {
          await result;
        }
      }
      globalThis.dispatchEvent(msgEvent);
    } catch (e) {
      let handled = false;

      const errorEvent = new ErrorEvent("error", {
        cancelable: true,
        message: e.message,
        lineno: e.lineNumber ? e.lineNumber + 1 : undefined,
        colno: e.columnNumber ? e.columnNumber + 1 : undefined,
        filename: e.fileName,
        error: null,
      });

      if (globalThis["onerror"]) {
        const ret = globalThis.onerror(
          e.message,
          e.fileName,
          e.lineNumber,
          e.columnNumber,
          e,
        );
        handled = ret === true;
      }

      globalThis.dispatchEvent(errorEvent);
      if (errorEvent.defaultPrevented) {
        handled = true;
      }

      if (!handled) {
        throw e;
      }
    }
  }

  function opPostMessage(data) {
    core.jsonOpSync("op_worker_post_message", {}, data);
  }

  function opCloseWorker() {
    core.jsonOpSync("op_worker_close");
  }

  function opMainModule() {
    return core.jsonOpSync("op_main_module");
  }

  function runtimeStart(runtimeOptions, source) {
    const opsMap = core.ops();
    for (const [name, opId] of Object.entries(opsMap)) {
      if (name === "op_write" || name === "op_read") {
        core.setAsyncHandler(opId, dispatchMinimal.asyncMsgFromRust);
      }
    }

    core.setMacrotaskCallback(timers.handleTimerMacrotask);
    version.setVersions(
      runtimeOptions.denoVersion,
      runtimeOptions.v8Version,
      runtimeOptions.tsVersion,
    );
    build.setBuildInfo(runtimeOptions.target);
    util.setLogDebug(runtimeOptions.debugFlag, source);
    // TODO(bartlomieju): a very crude way to disable
    // source mapping of errors. This condition is true
    // only for compiled standalone binaries.
    let prepareStackTrace;
    if (runtimeOptions.applySourceMaps) {
      prepareStackTrace = core.createPrepareStackTrace(
        errorStack.opApplySourceMap,
      );
    } else {
      prepareStackTrace = core.createPrepareStackTrace();
    }
    Error.prepareStackTrace = prepareStackTrace;
  }

  function registerErrors() {
    core.registerErrorClass("NotFound", errors.NotFound);
    core.registerErrorClass("PermissionDenied", errors.PermissionDenied);
    core.registerErrorClass("ConnectionRefused", errors.ConnectionRefused);
    core.registerErrorClass("ConnectionReset", errors.ConnectionReset);
    core.registerErrorClass("ConnectionAborted", errors.ConnectionAborted);
    core.registerErrorClass("NotConnected", errors.NotConnected);
    core.registerErrorClass("AddrInUse", errors.AddrInUse);
    core.registerErrorClass("AddrNotAvailable", errors.AddrNotAvailable);
    core.registerErrorClass("BrokenPipe", errors.BrokenPipe);
    core.registerErrorClass("AlreadyExists", errors.AlreadyExists);
    core.registerErrorClass("InvalidData", errors.InvalidData);
    core.registerErrorClass("TimedOut", errors.TimedOut);
    core.registerErrorClass("Interrupted", errors.Interrupted);
    core.registerErrorClass("WriteZero", errors.WriteZero);
    core.registerErrorClass("UnexpectedEof", errors.UnexpectedEof);
    core.registerErrorClass("BadResource", errors.BadResource);
    core.registerErrorClass("Http", errors.Http);
    core.registerErrorClass("Busy", errors.Busy);
    core.registerErrorClass("NotSupported", errors.NotSupported);
    core.registerErrorClass("Error", Error);
    core.registerErrorClass("RangeError", RangeError);
    core.registerErrorClass("ReferenceError", ReferenceError);
    core.registerErrorClass("SyntaxError", SyntaxError);
    core.registerErrorClass("TypeError", TypeError);
    core.registerErrorClass("URIError", URIError);
  }

  // https://developer.mozilla.org/en-US/docs/Web/API/WindowOrWorkerGlobalScope
  const windowOrWorkerGlobalScope = {
    Blob: util.nonEnumerable(file.Blob),
    ByteLengthQueuingStrategy: util.nonEnumerable(
      streams.ByteLengthQueuingStrategy,
    ),
    CloseEvent: util.nonEnumerable(CloseEvent),
    CountQueuingStrategy: util.nonEnumerable(
      streams.CountQueuingStrategy,
    ),
    CustomEvent: util.nonEnumerable(CustomEvent),
    DOMException: util.nonEnumerable(DOMException),
    ErrorEvent: util.nonEnumerable(ErrorEvent),
    Event: util.nonEnumerable(Event),
    EventTarget: util.nonEnumerable(EventTarget),
    File: util.nonEnumerable(file.File),
    FileReader: util.nonEnumerable(fileReader.FileReader),
    FormData: util.nonEnumerable(fetch.FormData),
    Headers: util.nonEnumerable(headers.Headers),
    MessageEvent: util.nonEnumerable(MessageEvent),
    Performance: util.nonEnumerable(performance.Performance),
    PerformanceEntry: util.nonEnumerable(performance.PerformanceEntry),
    PerformanceMark: util.nonEnumerable(performance.PerformanceMark),
    PerformanceMeasure: util.nonEnumerable(performance.PerformanceMeasure),
    ProgressEvent: util.nonEnumerable(ProgressEvent),
    ReadableStream: util.nonEnumerable(streams.ReadableStream),
    ReadableStreamDefaultReader: util.nonEnumerable(
      streams.ReadableStreamDefaultReader,
    ),
    Request: util.nonEnumerable(fetch.Request),
    Response: util.nonEnumerable(fetch.Response),
    TextDecoder: util.nonEnumerable(TextDecoder),
    TextEncoder: util.nonEnumerable(TextEncoder),
    TransformStream: util.nonEnumerable(streams.TransformStream),
    URL: util.nonEnumerable(url.URL),
    URLSearchParams: util.nonEnumerable(url.URLSearchParams),
    WebSocket: util.nonEnumerable(webSocket.WebSocket),
    Worker: util.nonEnumerable(worker.Worker),
    WritableStream: util.nonEnumerable(streams.WritableStream),
    WritableStreamDefaultWriter: util.nonEnumerable(
      streams.WritableStreamDefaultWriter,
    ),
    atob: util.writable(atob),
    btoa: util.writable(btoa),
    clearInterval: util.writable(timers.clearInterval),
    clearTimeout: util.writable(timers.clearTimeout),
    console: util.writable(new Console(core.print)),
    crypto: util.readOnly(crypto),
    fetch: util.writable(fetch.fetch),
    performance: util.writable(performance.performance),
    setInterval: util.writable(timers.setInterval),
    setTimeout: util.writable(timers.setTimeout),
  };

  // The console seems to be the only one that should be writable and non-enumerable
  // thus we don't have a unique helper for it. If other properties follow the same
  // structure, it might be worth it to define a helper in `util`
  windowOrWorkerGlobalScope.console.enumerable = false;

  const mainRuntimeGlobalProperties = {
    Location: location.locationConstructorDescriptor,
    location: location.locationDescriptor,
    Window: globalInterfaces.windowConstructorDescriptor,
    window: util.readOnly(globalThis),
    self: util.readOnly(globalThis),
    // TODO(bartlomieju): from MDN docs (https://developer.mozilla.org/en-US/docs/Web/API/WorkerGlobalScope)
    // it seems those two properties should be available to workers as well
    onload: util.writable(null),
    onunload: util.writable(null),
    close: util.writable(windowClose),
    closed: util.getterOnly(() => windowIsClosing),
    alert: util.writable(prompt.alert),
    confirm: util.writable(prompt.confirm),
    prompt: util.writable(prompt.prompt),
  };

  const workerRuntimeGlobalProperties = {
    WorkerLocation: location.workerLocationConstructorDescriptor,
    location: location.workerLocationDescriptor,
    WorkerGlobalScope: globalInterfaces.workerGlobalScopeConstructorDescriptor,
    DedicatedWorkerGlobalScope:
      globalInterfaces.dedicatedWorkerGlobalScopeConstructorDescriptor,
    self: util.readOnly(globalThis),
    onmessage: util.writable(onmessage),
    onerror: util.writable(onerror),
    // TODO(bartlomieju): should be readonly?
    close: util.nonEnumerable(workerClose),
    postMessage: util.writable(postMessage),
    workerMessageRecvCallback: util.nonEnumerable(workerMessageRecvCallback),
  };

  let hasBootstrapped = false;

  function bootstrapMainRuntime(runtimeOptions) {
    if (hasBootstrapped) {
      throw new Error("Worker runtime already bootstrapped");
    }
    // Remove bootstrapping data from the global scope
    delete globalThis.__bootstrap;
    delete globalThis.bootstrap;
    util.log("bootstrapMainRuntime");
    hasBootstrapped = true;
    Object.defineProperties(globalThis, windowOrWorkerGlobalScope);
    Object.defineProperties(globalThis, mainRuntimeGlobalProperties);
    Object.setPrototypeOf(globalThis, Window.prototype);
    eventTarget.setEventTargetData(globalThis);

    defineEventHandler(window, "load", null);
    defineEventHandler(window, "unload", null);

    const isUnloadDispatched = Symbol.for("isUnloadDispatched");
    // Stores the flag for checking whether unload is dispatched or not.
    // This prevents the recursive dispatches of unload events.
    // See https://github.com/denoland/deno/issues/9201.
    window[isUnloadDispatched] = false;
    window.addEventListener("unload", () => {
      window[isUnloadDispatched] = true;
    });

    runtimeStart(runtimeOptions);
    const {
      args,
      location: locationHref,
      noColor,
      pid,
      ppid,
      unstableFlag,
    } = runtimeOptions;

    if (locationHref != null) {
      location.setLocationHref(locationHref);
      fetch.setBaseUrl(locationHref);
    }

    registerErrors();

    const finalDenoNs = {
      core,
      internal: internalSymbol,
      [internalSymbol]: internalObject,
      resources: core.resources,
      close: core.close,
      ...denoNs,
    };
    Object.defineProperties(finalDenoNs, {
      pid: util.readOnly(pid),
      ppid: util.readOnly(ppid),
      noColor: util.readOnly(noColor),
      args: util.readOnly(Object.freeze(args)),
      mainModule: util.getterOnly(opMainModule),
    });

    if (unstableFlag) {
      Object.assign(finalDenoNs, denoNsUnstable);
    }

    // Setup `Deno` global - we're actually overriding already
    // existing global `Deno` with `Deno` namespace from "./deno.ts".
    util.immutableDefine(globalThis, "Deno", finalDenoNs);
    Object.freeze(globalThis.Deno);
    Object.freeze(globalThis.Deno.core);
    Object.freeze(globalThis.Deno.core.sharedQueue);
    signals.setSignals();

    util.log("args", args);
  }

  function bootstrapWorkerRuntime(
    runtimeOptions,
    name,
    useDenoNamespace,
    internalName,
  ) {
    if (hasBootstrapped) {
      throw new Error("Worker runtime already bootstrapped");
    }
    // Remove bootstrapping data from the global scope
    delete globalThis.__bootstrap;
    delete globalThis.bootstrap;
    util.log("bootstrapWorkerRuntime");
    hasBootstrapped = true;
    Object.defineProperties(globalThis, windowOrWorkerGlobalScope);
    Object.defineProperties(globalThis, workerRuntimeGlobalProperties);
    Object.defineProperties(globalThis, { name: util.readOnly(name) });
    Object.setPrototypeOf(globalThis, DedicatedWorkerGlobalScope.prototype);
    eventTarget.setEventTargetData(globalThis);

    runtimeStart(
      runtimeOptions,
      internalName ?? name,
    );
    const { unstableFlag, pid, noColor, args, location: locationHref } =
      runtimeOptions;

    location.setLocationHref(locationHref);
    fetch.setBaseUrl(locationHref);
    registerErrors();

    const finalDenoNs = {
      core,
      internal: internalSymbol,
      [internalSymbol]: internalObject,
      resources: core.resources,
      close: core.close,
      ...denoNs,
    };
    if (useDenoNamespace) {
      if (unstableFlag) {
        Object.assign(finalDenoNs, denoNsUnstable);
      }
      Object.defineProperties(finalDenoNs, {
        pid: util.readOnly(pid),
        noColor: util.readOnly(noColor),
        args: util.readOnly(Object.freeze(args)),
      });
      // Setup `Deno` global - we're actually overriding already
      // existing global `Deno` with `Deno` namespace from "./deno.ts".
      util.immutableDefine(globalThis, "Deno", finalDenoNs);
      Object.freeze(globalThis.Deno);
      Object.freeze(globalThis.Deno.core);
      Object.freeze(globalThis.Deno.core.sharedQueue);
      signals.setSignals();
    } else {
      delete globalThis.Deno;
      util.assert(globalThis.Deno === undefined);
    }
  }

  Object.defineProperties(globalThis, {
    bootstrap: {
      value: {
        mainRuntime: bootstrapMainRuntime,
        workerRuntime: bootstrapWorkerRuntime,
      },
      configurable: true,
    },
  });
})(this);
