// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { assertThrows, unitTest } from "./test_util.ts";

unitTest(function streamReadableHwmError() {
  // deno-lint-ignore no-explicit-any
  const invalidHwm: any[] = [NaN, Number("NaN"), {}, -1, "two"];
  for (const highWaterMark of invalidHwm) {
    assertThrows(
      () => {
        new ReadableStream<number>(undefined, { highWaterMark });
      },
      RangeError,
      "highWaterMark must be a positive number or Infinity.  Received:",
    );
  }

  assertThrows(() => {
    new ReadableStream<number>(
      undefined,
      // deno-lint-ignore no-explicit-any
      { highWaterMark: Symbol("hwk") as any },
    );
  }, TypeError);
});

unitTest(function streamWriteableHwmError() {
  // deno-lint-ignore no-explicit-any
  const invalidHwm: any[] = [NaN, Number("NaN"), {}, -1, "two"];
  for (const highWaterMark of invalidHwm) {
    assertThrows(
      () => {
        new WritableStream(
          undefined,
          new CountQueuingStrategy({ highWaterMark }),
        );
      },
      RangeError,
      "highWaterMark must be a positive number or Infinity.  Received:",
    );
  }

  assertThrows(() => {
    new WritableStream(
      undefined,
      // deno-lint-ignore no-explicit-any
      new CountQueuingStrategy({ highWaterMark: Symbol("hwmk") as any }),
    );
  }, TypeError);
});

unitTest(function streamTransformHwmError() {
  // deno-lint-ignore no-explicit-any
  const invalidHwm: any[] = [NaN, Number("NaN"), {}, -1, "two"];
  for (const highWaterMark of invalidHwm) {
    assertThrows(
      () => {
        new TransformStream(undefined, undefined, { highWaterMark });
      },
      RangeError,
      "highWaterMark must be a positive number or Infinity.  Received:",
    );
  }

  assertThrows(() => {
    new TransformStream(
      undefined,
      undefined,
      // deno-lint-ignore no-explicit-any
      { highWaterMark: Symbol("hwmk") as any },
    );
  }, TypeError);
});
