// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

((window) => {
  const core = window.Deno.core;
  const webidl = window.__bootstrap.webidl;

  const _persistent = Symbol("[[persistent]]");

  class Storage {
    [_persistent];

    constructor() {
      webidl.illegalConstructor();
    }

    get length() {
      webidl.assertBranded(this, Storage);
      return core.opSync("op_webstorage_length", this[_persistent]);
    }

    key(index) {
      webidl.assertBranded(this, Storage);
      const prefix = "Failed to execute 'key' on 'Storage'";
      webidl.requiredArguments(arguments.length, 1, { prefix });
      index = webidl.converters["unsigned long"](index, {
        prefix,
        context: "Argument 1",
      });

      return core.opSync("op_webstorage_key", index, this[_persistent]);
    }

    setItem(key, value) {
      webidl.assertBranded(this, Storage);
      const prefix = "Failed to execute 'setItem' on 'Storage'";
      webidl.requiredArguments(arguments.length, 2, { prefix });
      key = webidl.converters.DOMString(key, {
        prefix,
        context: "Argument 1",
      });
      value = webidl.converters.DOMString(value, {
        prefix,
        context: "Argument 2",
      });

      core.opSync("op_webstorage_set", {
        keyName: key,
        keyValue: value,
      }, this[_persistent]);
    }

    getItem(key) {
      webidl.assertBranded(this, Storage);
      const prefix = "Failed to execute 'getItem' on 'Storage'";
      webidl.requiredArguments(arguments.length, 1, { prefix });
      key = webidl.converters.DOMString(key, {
        prefix,
        context: "Argument 1",
      });

      return core.opSync("op_webstorage_get", key, this[_persistent]);
    }

    removeItem(key) {
      webidl.assertBranded(this, Storage);
      const prefix = "Failed to execute 'removeItem' on 'Storage'";
      webidl.requiredArguments(arguments.length, 1, { prefix });
      key = webidl.converters.DOMString(key, {
        prefix,
        context: "Argument 1",
      });

      core.opSync("op_webstorage_remove", key, this[_persistent]);
    }

    clear() {
      webidl.assertBranded(this, Storage);
      core.opSync("op_webstorage_clear", this[_persistent]);
    }
  }

  function createStorage(persistent) {
    if (persistent) window.location;

    const storage = webidl.createBranded(Storage);
    storage[_persistent] = persistent;

    const proxy = new Proxy(storage, {
      deleteProperty(target, key) {
        if (typeof key == "symbol") {
          delete target[key];
        } else {
          target.removeItem(key);
        }
        return true;
      },
      defineProperty(target, key, descriptor) {
        if (typeof key == "symbol") {
          Object.defineProperty(target, key, descriptor);
        } else {
          target.setItem(key, descriptor.value);
        }
        return true;
      },
      get(target, key) {
        if (typeof key == "symbol") return target[key];
        if (key in target) {
          return Reflect.get(...arguments);
        } else {
          return target.getItem(key) ?? undefined;
        }
      },
      set(target, key, value) {
        if (typeof key == "symbol") {
          Object.defineProperty(target, key, {
            value,
            configurable: true,
          });
        } else {
          target.setItem(key, value);
        }
        return true;
      },
      has(target, p) {
        return (typeof target.getItem(p)) === "string";
      },
      ownKeys() {
        return core.opSync("op_webstorage_iterate_keys", persistent);
      },
      getOwnPropertyDescriptor(target, key) {
        if (arguments.length === 1) {
          return undefined;
        }
        if (key in target) {
          return undefined;
        }
        const value = target.getItem(key);
        if (value === null) {
          return undefined;
        }
        return {
          value,
          enumerable: true,
          configurable: true,
          writable: true,
        };
      },
    });

    proxy[Symbol.for("Deno.customInspect")] = function (inspect) {
      return `${this.constructor.name} ${
        inspect({
          length: this.length,
          ...Object.fromEntries(Object.entries(proxy)),
        })
      }`;
    };

    return proxy;
  }

  let localStorage;
  let sessionStorage;

  window.__bootstrap.webStorage = {
    localStorage() {
      if (!localStorage) {
        localStorage = createStorage(true);
      }
      return localStorage;
    },
    sessionStorage() {
      if (!sessionStorage) {
        sessionStorage = createStorage(false);
      }
      return sessionStorage;
    },
    Storage,
  };
})(this);
