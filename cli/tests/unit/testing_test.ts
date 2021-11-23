// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
import { assertRejects, assertThrows, unitTest } from "./test_util.ts";

unitTest(function testWrongOverloads() {
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test("some name", { fn: () => {} }, () => {});
    },
    TypeError,
    "Unexpected 'fn' field in options, test function is already provided as the third argument.",
  );
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test("some name", { name: "some name2" }, () => {});
    },
    TypeError,
    "Unexpected 'name' field in options, test name is already provided as the first argument.",
  );
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test(() => {});
    },
    TypeError,
    "The test function must have a name",
  );
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test(function foo() {}, {});
    },
    TypeError,
    "Unexpected second argument to Deno.test()",
  );
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test({ fn: () => {} }, function foo() {});
    },
    TypeError,
    "Unexpected 'fn' field in options, test function is already provided as the second argument.",
  );
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test({});
    },
    TypeError,
    "Expected 'fn' field in the first argument to be a test function.",
  );
  assertThrows(
    () => {
      // @ts-ignore Testing invalid overloads
      Deno.test({ fn: "boo!" });
    },
    TypeError,
    "Expected 'fn' field in the first argument to be a test function.",
  );
});

unitTest(function nameOfTestCaseCantBeEmpty() {
  assertThrows(
    () => {
      Deno.test("", () => {});
    },
    TypeError,
    "The test name can't be empty",
  );
  assertThrows(
    () => {
      Deno.test({
        name: "",
        fn: () => {},
      });
    },
    TypeError,
    "The test name can't be empty",
  );
});

unitTest(function invalidStepArguments(t) {
  assertRejects(
    async () => {
      // deno-lint-ignore no-explicit-any
      await (t as any).step("test");
    },
    TypeError,
    "Expected function for second argument.",
  );

  assertRejects(
    async () => {
      // deno-lint-ignore no-explicit-any
      await (t as any).step("test", "not a function");
    },
    TypeError,
    "Expected function for second argument.",
  );

  assertRejects(
    async () => {
      // deno-lint-ignore no-explicit-any
      await (t as any).step();
    },
    TypeError,
    "Expected a test definition or name and function.",
  );

  assertRejects(
    async () => {
      // deno-lint-ignore no-explicit-any
      await (t as any).step(() => {});
    },
    TypeError,
    "Expected a test definition or name and function.",
  );
});
