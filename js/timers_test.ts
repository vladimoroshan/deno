// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { test, assertEquals } from "./test_util.ts";

function deferred(): {
  promise: Promise<{}>;
  resolve: (value?: {} | PromiseLike<{}>) => void;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  reject: (reason?: any) => void;
} {
  let resolve;
  let reject;
  const promise = new Promise(
    (res, rej): void => {
      resolve = res;
      reject = rej;
    }
  );
  return {
    promise,
    resolve,
    reject
  };
}

async function waitForMs(ms): Promise<number> {
  return new Promise((resolve): number => setTimeout(resolve, ms));
}

test(async function timeoutSuccess(): Promise<void> {
  const { promise, resolve } = deferred();
  let count = 0;
  setTimeout((): void => {
    count++;
    resolve();
  }, 500);
  await promise;
  // count should increment
  assertEquals(count, 1);
});

test(async function timeoutArgs(): Promise<void> {
  const { promise, resolve } = deferred();
  const arg = 1;
  setTimeout(
    (a, b, c): void => {
      assertEquals(a, arg);
      assertEquals(b, arg.toString());
      assertEquals(c, [arg]);
      resolve();
    },
    10,
    arg,
    arg.toString(),
    [arg]
  );
  await promise;
});

test(async function timeoutCancelSuccess(): Promise<void> {
  let count = 0;
  const id = setTimeout((): void => {
    count++;
  }, 1);
  // Cancelled, count should not increment
  clearTimeout(id);
  await waitForMs(600);
  assertEquals(count, 0);
});

test(async function timeoutCancelMultiple(): Promise<void> {
  function uncalled(): never {
    throw new Error("This function should not be called.");
  }

  // Set timers and cancel them in the same order.
  const t1 = setTimeout(uncalled, 10);
  const t2 = setTimeout(uncalled, 10);
  const t3 = setTimeout(uncalled, 10);
  clearTimeout(t1);
  clearTimeout(t2);
  clearTimeout(t3);

  // Set timers and cancel them in reverse order.
  const t4 = setTimeout(uncalled, 20);
  const t5 = setTimeout(uncalled, 20);
  const t6 = setTimeout(uncalled, 20);
  clearTimeout(t6);
  clearTimeout(t5);
  clearTimeout(t4);

  // Sleep until we're certain that the cancelled timers aren't gonna fire.
  await waitForMs(50);
});

test(async function timeoutCancelInvalidSilentFail(): Promise<void> {
  // Expect no panic
  const { promise, resolve } = deferred();
  let count = 0;
  const id = setTimeout((): void => {
    count++;
    // Should have no effect
    clearTimeout(id);
    resolve();
  }, 500);
  await promise;
  assertEquals(count, 1);

  // Should silently fail (no panic)
  clearTimeout(2147483647);
});

test(async function intervalSuccess(): Promise<void> {
  const { promise, resolve } = deferred();
  let count = 0;
  const id = setInterval((): void => {
    count++;
    clearInterval(id);
    resolve();
  }, 100);
  await promise;
  // Clear interval
  clearInterval(id);
  // count should increment twice
  assertEquals(count, 1);
});

test(async function intervalCancelSuccess(): Promise<void> {
  let count = 0;
  const id = setInterval((): void => {
    count++;
  }, 1);
  clearInterval(id);
  await waitForMs(500);
  assertEquals(count, 0);
});

test(async function intervalOrdering(): Promise<void> {
  const timers = [];
  let timeouts = 0;
  function onTimeout(): void {
    ++timeouts;
    for (let i = 1; i < timers.length; i++) {
      clearTimeout(timers[i]);
    }
  }
  for (let i = 0; i < 10; i++) {
    timers[i] = setTimeout(onTimeout, 1);
  }
  await waitForMs(500);
  assertEquals(timeouts, 1);
});

test(async function intervalCancelInvalidSilentFail(): Promise<void> {
  // Should silently fail (no panic)
  clearInterval(2147483647);
});

test(async function fireCallbackImmediatelyWhenDelayOverMaxValue(): Promise<
  void
> {
  let count = 0;
  setTimeout((): void => {
    count++;
  }, 2 ** 31);
  await waitForMs(1);
  assertEquals(count, 1);
});
