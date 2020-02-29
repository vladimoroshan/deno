// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import {
  test,
  testPerm,
  assert,
  assertEquals,
  createResolvable
} from "./test_util.ts";

test(async function workersBasic(): Promise<void> {
  const promise = createResolvable();
  const jsWorker = new Worker("../tests/subdir/test_worker.js", {
    type: "module",
    name: "jsWorker"
  });
  const tsWorker = new Worker("../tests/subdir/test_worker.ts", {
    type: "module",
    name: "tsWorker"
  });

  tsWorker.onmessage = (e): void => {
    assertEquals(e.data, "Hello World");
    promise.resolve();
  };

  jsWorker.onmessage = (e): void => {
    assertEquals(e.data, "Hello World");
    tsWorker.postMessage("Hello World");
  };

  jsWorker.onerror = (e: Event): void => {
    e.preventDefault();
    jsWorker.postMessage("Hello World");
  };

  jsWorker.postMessage("Hello World");
  await promise;
});

test(async function nestedWorker(): Promise<void> {
  const promise = createResolvable();

  const nestedWorker = new Worker("../tests/subdir/nested_worker.js", {
    type: "module",
    name: "nested"
  });

  nestedWorker.onmessage = (e): void => {
    assert(e.data.type !== "error");
    promise.resolve();
  };

  nestedWorker.postMessage("Hello World");
  await promise;
});

test(async function workerThrowsWhenExecuting(): Promise<void> {
  const promise = createResolvable();

  const throwingWorker = new Worker("../tests/subdir/throwing_worker.js", {
    type: "module"
  });

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  throwingWorker.onerror = (e: any): void => {
    e.preventDefault();
    assert(/Uncaught Error: Thrown error/.test(e.message));
    promise.resolve();
  };

  await promise;
});

testPerm({ net: true }, async function workerCanUseFetch(): Promise<void> {
  const promise = createResolvable();

  const fetchingWorker = new Worker("../tests/subdir/fetching_worker.js", {
    type: "module"
  });

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  fetchingWorker.onerror = (e: any): void => {
    e.preventDefault();
    promise.reject(e.message);
  };

  fetchingWorker.onmessage = (e): void => {
    assert(e.data === "Done!");
    promise.resolve();
  };

  await promise;
});
