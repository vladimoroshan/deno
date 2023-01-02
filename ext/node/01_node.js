// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

// deno-lint-ignore-file

"use strict";

((window) => {
  const {
    ArrayPrototypePush,
    ArrayPrototypeFilter,
    ObjectEntries,
    ObjectCreate,
    ObjectDefineProperty,
    Proxy,
    ReflectDefineProperty,
    ReflectGetOwnPropertyDescriptor,
    ReflectOwnKeys,
    Set,
    SetPrototypeHas,
  } = window.__bootstrap.primordials;

  function assert(cond) {
    if (!cond) {
      throw Error("assert");
    }
  }

  let initialized = false;
  const nodeGlobals = {};
  const nodeGlobalThis = new Proxy(globalThis, {
    get(_target, prop, _receiver) {
      if (prop in nodeGlobals) {
        return nodeGlobals[prop];
      } else {
        return globalThis[prop];
      }
    },
    set(_target, prop, value) {
      if (prop in nodeGlobals) {
        nodeGlobals[prop] = value;
      } else {
        globalThis[prop] = value;
      }
      return true;
    },
    deleteProperty(_target, prop) {
      let success = false;
      if (prop in nodeGlobals) {
        delete nodeGlobals[prop];
        success = true;
      }
      if (prop in globalThis) {
        delete globalThis[prop];
        success = true;
      }
      return success;
    },
    ownKeys(_target) {
      const globalThisKeys = ReflectOwnKeys(globalThis);
      const nodeGlobalsKeys = ReflectOwnKeys(nodeGlobals);
      const nodeGlobalsKeySet = new Set(nodeGlobalsKeys);
      return [
        ...ArrayPrototypeFilter(
          globalThisKeys,
          (k) => !SetPrototypeHas(nodeGlobalsKeySet, k),
        ),
        ...nodeGlobalsKeys,
      ];
    },
    defineProperty(_target, prop, desc) {
      if (prop in nodeGlobals) {
        return ReflectDefineProperty(nodeGlobals, prop, desc);
      } else {
        return ReflectDefineProperty(globalThis, prop, desc);
      }
    },
    getOwnPropertyDescriptor(_target, prop) {
      if (prop in nodeGlobals) {
        return ReflectGetOwnPropertyDescriptor(nodeGlobals, prop);
      } else {
        return ReflectGetOwnPropertyDescriptor(globalThis, prop);
      }
    },
    has(_target, prop) {
      return prop in nodeGlobals || prop in globalThis;
    },
  });

  const nativeModuleExports = ObjectCreate(null);
  const builtinModules = [];

  function initialize(nodeModules, nodeGlobalThisName) {
    assert(!initialized);
    initialized = true;
    for (const [name, exports] of ObjectEntries(nodeModules)) {
      nativeModuleExports[name] = exports;
      ArrayPrototypePush(builtinModules, name);
    }
    nodeGlobals.Buffer = nativeModuleExports["buffer"].Buffer;
    nodeGlobals.clearImmediate = nativeModuleExports["timers"].clearImmediate;
    nodeGlobals.clearInterval = nativeModuleExports["timers"].clearInterval;
    nodeGlobals.clearTimeout = nativeModuleExports["timers"].clearTimeout;
    nodeGlobals.console = nativeModuleExports["console"];
    nodeGlobals.global = nodeGlobalThis;
    nodeGlobals.process = nativeModuleExports["process"];
    nodeGlobals.setImmediate = nativeModuleExports["timers"].setImmediate;
    nodeGlobals.setInterval = nativeModuleExports["timers"].setInterval;
    nodeGlobals.setTimeout = nativeModuleExports["timers"].setTimeout;

    // add a hidden global for the esm code to use in order to reliably
    // get node's globalThis
    ObjectDefineProperty(globalThis, nodeGlobalThisName, {
      enumerable: false,
      writable: false,
      value: nodeGlobalThis,
    });
  }

  window.__bootstrap.internals = {
    ...window.__bootstrap.internals ?? {},
    node: {
      globalThis: nodeGlobalThis,
      initialize,
      nativeModuleExports,
      builtinModules,
    },
  };
})(globalThis);
