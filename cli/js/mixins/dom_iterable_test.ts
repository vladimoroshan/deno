// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { test, assert, assertEquals } from "../test_util.ts";

// eslint-disable-next-line @typescript-eslint/explicit-function-return-type
function setup() {
  const dataSymbol = Symbol("data symbol");
  class Base {
    private [dataSymbol] = new Map<string, number>();

    constructor(
      data: Array<[string, number]> | IterableIterator<[string, number]>
    ) {
      for (const [key, value] of data) {
        this[dataSymbol].set(key, value);
      }
    }
  }

  return {
    Base,
    // This is using an internal API we don't want published as types, so having
    // to cast to any to "trick" TypeScript
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    DomIterable: (Deno[Deno.symbols.internal] as any).DomIterableMixin(
      Base,
      dataSymbol
    )
  };
}

test(function testDomIterable(): void {
  const { DomIterable, Base } = setup();

  const fixture: Array<[string, number]> = [
    ["foo", 1],
    ["bar", 2]
  ];

  const domIterable = new DomIterable(fixture);

  assertEquals(Array.from(domIterable.entries()), fixture);
  assertEquals(Array.from(domIterable.values()), [1, 2]);
  assertEquals(Array.from(domIterable.keys()), ["foo", "bar"]);

  let result: Array<[string, number]> = [];
  for (const [key, value] of domIterable) {
    assert(key != null);
    assert(value != null);
    result.push([key, value]);
  }
  assertEquals(fixture, result);

  result = [];
  const scope = {};
  function callback(value, key, parent): void {
    assertEquals(parent, domIterable);
    assert(key != null);
    assert(value != null);
    assert(this === scope);
    result.push([key, value]);
  }
  domIterable.forEach(callback, scope);
  assertEquals(fixture, result);

  assertEquals(DomIterable.name, Base.name);
});

test(function testDomIterableScope(): void {
  const { DomIterable } = setup();

  const domIterable = new DomIterable([["foo", 1]]);

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  function checkScope(thisArg: any, expected: any): void {
    function callback(): void {
      assertEquals(this, expected);
    }
    domIterable.forEach(callback, thisArg);
  }

  checkScope(0, Object(0));
  checkScope("", Object(""));
  checkScope(null, window);
  checkScope(undefined, window);
});
