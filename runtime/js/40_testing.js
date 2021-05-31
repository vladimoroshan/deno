// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
"use strict";

((window) => {
  const core = window.Deno.core;
  const { parsePermissions } = window.__bootstrap.worker;
  const { setExitHandler } = window.__bootstrap.os;
  const { Console, inspectArgs } = window.__bootstrap.console;
  const { metrics } = window.__bootstrap.metrics;
  const { assert } = window.__bootstrap.util;

  // Wrap test function in additional assertion that makes sure
  // the test case does not leak async "ops" - ie. number of async
  // completed ops after the test is the same as number of dispatched
  // ops. Note that "unref" ops are ignored since in nature that are
  // optional.
  function assertOps(fn) {
    return async function asyncOpSanitizer() {
      const pre = metrics();
      try {
        await fn();
      } finally {
        // Defer until next event loop turn - that way timeouts and intervals
        // cleared can actually be removed from resource table, otherwise
        // false positives may occur (https://github.com/denoland/deno/issues/4591)
        await new Promise((resolve) => setTimeout(resolve, 0));
      }

      const post = metrics();
      // We're checking diff because one might spawn HTTP server in the background
      // that will be a pending async op before test starts.
      const dispatchedDiff = post.opsDispatchedAsync - pre.opsDispatchedAsync;
      const completedDiff = post.opsCompletedAsync - pre.opsCompletedAsync;
      assert(
        dispatchedDiff === completedDiff,
        `Test case is leaking async ops.
Before:
  - dispatched: ${pre.opsDispatchedAsync}
  - completed: ${pre.opsCompletedAsync}
After:
  - dispatched: ${post.opsDispatchedAsync}
  - completed: ${post.opsCompletedAsync}

Make sure to await all promises returned from Deno APIs before
finishing test case.`,
      );
    };
  }

  // Wrap test function in additional assertion that makes sure
  // the test case does not "leak" resources - ie. resource table after
  // the test has exactly the same contents as before the test.
  function assertResources(
    fn,
  ) {
    return async function resourceSanitizer() {
      const pre = core.resources();
      await fn();
      const post = core.resources();

      const preStr = JSON.stringify(pre, null, 2);
      const postStr = JSON.stringify(post, null, 2);
      const msg = `Test case is leaking resources.
Before: ${preStr}
After: ${postStr}

Make sure to close all open resource handles returned from Deno APIs before
finishing test case.`;
      assert(preStr === postStr, msg);
    };
  }

  // Wrap test function in additional assertion that makes sure
  // that the test case does not accidentally exit prematurely.
  function assertExit(fn) {
    return async function exitSanitizer() {
      setExitHandler((exitCode) => {
        assert(
          false,
          `Test case attempted to exit with exit code: ${exitCode}`,
        );
      });

      try {
        await fn();
      } catch (err) {
        throw err;
      } finally {
        setExitHandler(null);
      }
    };
  }

  const tests = [];

  // Main test function provided by Deno, as you can see it merely
  // creates a new object with "name" and "fn" fields.
  function test(
    t,
    fn,
  ) {
    let testDef;
    const defaults = {
      ignore: false,
      only: false,
      sanitizeOps: true,
      sanitizeResources: true,
      sanitizeExit: true,
      permissions: null,
    };

    if (typeof t === "string") {
      if (!fn || typeof fn != "function") {
        throw new TypeError("Missing test function");
      }
      if (!t) {
        throw new TypeError("The test name can't be empty");
      }
      testDef = { fn: fn, name: t, ...defaults };
    } else {
      if (!t.fn) {
        throw new TypeError("Missing test function");
      }
      if (!t.name) {
        throw new TypeError("The test name can't be empty");
      }
      testDef = { ...defaults, ...t };
    }

    if (testDef.sanitizeOps) {
      testDef.fn = assertOps(testDef.fn);
    }

    if (testDef.sanitizeResources) {
      testDef.fn = assertResources(testDef.fn);
    }

    if (testDef.sanitizeExit) {
      testDef.fn = assertExit(testDef.fn);
    }

    tests.push(testDef);
  }

  function postTestMessage(kind, data) {
    return core.opSync("op_post_test_message", { message: { kind, data } });
  }

  function createTestFilter(filter) {
    return (def) => {
      if (filter) {
        if (filter.startsWith("/") && filter.endsWith("/")) {
          const regex = new RegExp(filter.slice(1, filter.length - 1));
          return regex.test(def.name);
        }

        return def.name.includes(filter);
      }

      return true;
    };
  }

  function pledgeTestPermissions(permissions) {
    return core.opSync(
      "op_pledge_test_permissions",
      parsePermissions(permissions),
    );
  }

  function restoreTestPermissions(token) {
    core.opSync("op_restore_test_permissions", token);
  }

  async function runTest({ name, ignore, fn, permissions }) {
    let token = null;
    const time = Date.now();

    try {
      postTestMessage("wait", {
        name,
      });

      if (permissions) {
        token = pledgeTestPermissions(permissions);
      }

      if (ignore) {
        const duration = Date.now() - time;
        postTestMessage("result", {
          name,
          duration,
          result: "ignored",
        });

        return;
      }

      await fn();

      const duration = Date.now() - time;
      postTestMessage("result", {
        name,
        duration,
        result: "ok",
      });
    } catch (error) {
      const duration = Date.now() - time;

      postTestMessage("result", {
        name,
        duration,
        result: {
          "failed": inspectArgs([error]),
        },
      });
    } finally {
      if (token) {
        restoreTestPermissions(token);
      }
    }
  }

  async function runTests({
    disableLog = false,
    filter = null,
  } = {}) {
    const originalConsole = globalThis.console;
    if (disableLog) {
      globalThis.console = new Console(() => {});
    }

    const only = tests.filter((test) => test.only);
    const pending = (only.length > 0 ? only : tests).filter(
      createTestFilter(filter),
    );
    postTestMessage("plan", {
      filtered: tests.length - pending.length,
      pending: pending.length,
      only: only.length > 0,
    });

    for (const test of pending) {
      await runTest(test);
    }

    if (disableLog) {
      globalThis.console = originalConsole;
    }
  }

  window.__bootstrap.internals = {
    ...window.__bootstrap.internals ?? {},
    runTests,
  };

  window.__bootstrap.testing = {
    test,
  };
})(this);
