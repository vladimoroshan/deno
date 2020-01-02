// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { test, assert, assertEquals, assertNotEquals } from "./test_util.ts";

function deferred(): {
  promise: Promise<{}>;
  resolve: (value?: {} | PromiseLike<{}>) => void;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  reject: (reason?: any) => void;
} {
  let resolve;
  let reject;
  const promise = new Promise((res, rej): void => {
    resolve = res;
    reject = rej;
  });
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

test(async function timeoutCallbackThis(): Promise<void> {
  const { promise, resolve } = deferred();
  const obj = {
    foo(): void {
      assertEquals(this, window);
      resolve();
    }
  };
  setTimeout(obj.foo, 1);
  await promise;
});

test(async function timeoutBindThis(): Promise<void> {
  function noop(): void {}

  const thisCheckPassed = [null, undefined, window, globalThis];

  const thisCheckFailed = [
    0,
    "",
    true,
    false,
    {},
    [],
    "foo",
    (): void => {},
    Object.prototype
  ];

  thisCheckPassed.forEach(
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (thisArg: any): void => {
      let hasThrown = 0;
      try {
        setTimeout.call(thisArg, noop, 1);
        hasThrown = 1;
      } catch (err) {
        if (err instanceof TypeError) {
          hasThrown = 2;
        } else {
          hasThrown = 3;
        }
      }
      assertEquals(hasThrown, 1);
    }
  );

  thisCheckFailed.forEach(
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    (thisArg: any): void => {
      let hasThrown = 0;
      try {
        setTimeout.call(thisArg, noop, 1);
        hasThrown = 1;
      } catch (err) {
        if (err instanceof TypeError) {
          hasThrown = 2;
        } else {
          hasThrown = 3;
        }
      }
      assertEquals(hasThrown, 2);
    }
  );
});

test(async function clearTimeoutShouldConvertToNumber(): Promise<void> {
  let called = false;
  const obj = {
    valueOf(): number {
      called = true;
      return 1;
    }
  };
  clearTimeout((obj as unknown) as number);
  assert(called);
});

test(function setTimeoutShouldThrowWithBigint(): void {
  let hasThrown = 0;
  try {
    setTimeout((): void => {}, (1n as unknown) as number);
    hasThrown = 1;
  } catch (err) {
    if (err instanceof TypeError) {
      hasThrown = 2;
    } else {
      hasThrown = 3;
    }
  }
  assertEquals(hasThrown, 2);
});

test(function clearTimeoutShouldThrowWithBigint(): void {
  let hasThrown = 0;
  try {
    clearTimeout((1n as unknown) as number);
    hasThrown = 1;
  } catch (err) {
    if (err instanceof TypeError) {
      hasThrown = 2;
    } else {
      hasThrown = 3;
    }
  }
  assertEquals(hasThrown, 2);
});

test(function testFunctionName(): void {
  assertEquals(clearTimeout.name, "clearTimeout");
  assertEquals(clearInterval.name, "clearInterval");
});

test(function testFunctionParamsLength(): void {
  assertEquals(setTimeout.length, 1);
  assertEquals(setInterval.length, 1);
  assertEquals(clearTimeout.length, 0);
  assertEquals(clearInterval.length, 0);
});

test(function clearTimeoutAndClearIntervalNotBeEquals(): void {
  assertNotEquals(clearTimeout, clearInterval);
});

test(async function timerMaxCpuBug(): Promise<void> {
  // There was a bug where clearing a timeout would cause Deno to use 100% CPU.
  clearTimeout(setTimeout(() => {}, 1000));
  // We can check this by counting how many ops have triggered in the interim.
  // Certainly less than 10 ops should have been dispatched in next 100 ms.
  const { opsDispatched } = Deno.metrics();
  await waitForMs(100);
  const opsDispatched_ = Deno.metrics().opsDispatched;
  console.log("opsDispatched", opsDispatched, "opsDispatched_", opsDispatched_);
  assert(opsDispatched_ - opsDispatched < 10);
});

test(async function timerBasicMicrotaskOrdering(): Promise<void> {
  let s = "";
  let count = 0;
  const { promise, resolve } = deferred();
  setTimeout(() => {
    Promise.resolve().then(() => {
      count++;
      s += "de";
      if (count === 2) {
        resolve();
      }
    });
  });
  setTimeout(() => {
    count++;
    s += "no";
    if (count === 2) {
      resolve();
    }
  });
  await promise;
  assertEquals(s, "deno");
});

test(async function timerNestedMicrotaskOrdering(): Promise<void> {
  let s = "";
  const { promise, resolve } = deferred();
  s += "0";
  setTimeout(() => {
    s += "4";
    setTimeout(() => (s += "8"));
    Promise.resolve().then(() => {
      setTimeout(() => {
        s += "9";
        resolve();
      });
    });
  });
  setTimeout(() => (s += "5"));
  Promise.resolve().then(() => (s += "2"));
  Promise.resolve().then(() =>
    setTimeout(() => {
      s += "6";
      Promise.resolve().then(() => (s += "7"));
    })
  );
  Promise.resolve().then(() => Promise.resolve().then(() => (s += "3")));
  s += "1";
  await promise;
  assertEquals(s, "0123456789");
});
