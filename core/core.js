// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
"use strict";

((window) => {
  // Available on start due to bindings.
  const core = window.Deno.core;
  const { recv, send } = core;

  let opsCache = {};
  const errorMap = {};
  let nextPromiseId = 1;
  const promiseTable = new Map();

  function init() {
    recv(handleAsyncMsgFromRust);
  }

  function ops() {
    // op id 0 is a special value to retrieve the map of registered ops.
    const newOpsCache = Object.fromEntries(send(0));
    opsCache = Object.freeze(newOpsCache);
    return opsCache;
  }

  function handleAsyncMsgFromRust() {
    for (let i = 0; i < arguments.length; i += 2) {
      opAsyncHandler(arguments[i], arguments[i + 1]);
    }
  }

  function dispatch(opName, promiseId, control, zeroCopy) {
    return send(opsCache[opName], promiseId, control, zeroCopy);
  }

  function registerErrorClass(errorName, className, args) {
    if (typeof errorMap[errorName] !== "undefined") {
      throw new TypeError(`Error class for "${errorName}" already registered`);
    }
    errorMap[errorName] = [className, args ?? []];
  }

  function getErrorClassAndArgs(errorName) {
    return errorMap[errorName] ?? [undefined, []];
  }

  function processResponse(res) {
    if (!isErr(res)) {
      return res;
    }
    throw processErr(res);
  }

  // .$err_class_name is a special key that should only exist on errors
  function isErr(res) {
    return !!(res && res.$err_class_name);
  }

  function processErr(err) {
    const className = err.$err_class_name;
    const [ErrorClass, args] = getErrorClassAndArgs(className);
    if (!ErrorClass) {
      return new Error(
        `Unregistered error class: "${className}"\n  ${err.message}\n  Classes of errors returned from ops should be registered via Deno.core.registerErrorClass().`,
      );
    }
    return new ErrorClass(err.message, ...args);
  }

  function jsonOpAsync(opName, args = null, zeroCopy = null) {
    const promiseId = nextPromiseId++;
    const maybeError = dispatch(opName, promiseId, args, zeroCopy);
    // Handle sync error (e.g: error parsing args)
    if (maybeError) processResponse(maybeError);
    let resolve, reject;
    const promise = new Promise((resolve_, reject_) => {
      resolve = resolve_;
      reject = reject_;
    });
    promise.resolve = resolve;
    promise.reject = reject;
    promiseTable.set(promiseId, promise);
    return promise;
  }

  function jsonOpSync(opName, args = null, zeroCopy = null) {
    return processResponse(dispatch(opName, null, args, zeroCopy));
  }

  function opAsyncHandler(promiseId, res) {
    const promise = promiseTable.get(promiseId);
    promiseTable.delete(promiseId);
    if (!isErr(res)) {
      promise.resolve(res);
    } else {
      promise.reject(processErr(res));
    }
  }

  function binOpSync(opName, args = null, zeroCopy = null) {
    return jsonOpSync(opName, args, zeroCopy);
  }

  function binOpAsync(opName, args = null, zeroCopy = null) {
    return jsonOpAsync(opName, args, zeroCopy);
  }

  function resources() {
    return jsonOpSync("op_resources");
  }

  function close(rid) {
    jsonOpSync("op_close", { rid });
  }

  Object.assign(window.Deno.core, {
    binOpAsync,
    binOpSync,
    jsonOpAsync,
    jsonOpSync,
    dispatch: send,
    dispatchByName: dispatch,
    ops,
    close,
    resources,
    registerErrorClass,
    init,
  });
})(this);
