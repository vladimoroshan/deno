// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { test, assert, assertEquals } from "./test_util.ts";

// Logic heavily copied from web-platform-tests, make
// sure pass mostly header basic test
// ref: https://github.com/web-platform-tests/wpt/blob/7c50c216081d6ea3c9afe553ee7b64534020a1b2/fetch/api/headers/headers-basic.html
test(function newHeaderTest(): void {
  new Headers();
  new Headers(undefined);
  new Headers({});
  try {
    new Headers(null);
  } catch (e) {
    assertEquals(
      e.message,
      "Failed to construct 'Headers'; The provided value was not valid"
    );
  }
});

const headerDict = {
  name1: "value1",
  name2: "value2",
  name3: "value3",
  name4: undefined,
  "Content-Type": "value4"
};
const headerSeq = [];
for (const name in headerDict) {
  headerSeq.push([name, headerDict[name]]);
}

test(function newHeaderWithSequence(): void {
  const headers = new Headers(headerSeq);
  for (const name in headerDict) {
    assertEquals(headers.get(name), String(headerDict[name]));
  }
  assertEquals(headers.get("length"), null);
});

test(function newHeaderWithRecord(): void {
  const headers = new Headers(headerDict);
  for (const name in headerDict) {
    assertEquals(headers.get(name), String(headerDict[name]));
  }
});

test(function newHeaderWithHeadersInstance(): void {
  const headers = new Headers(headerDict);
  const headers2 = new Headers(headers);
  for (const name in headerDict) {
    assertEquals(headers2.get(name), String(headerDict[name]));
  }
});

test(function headerAppendSuccess(): void {
  const headers = new Headers();
  for (const name in headerDict) {
    headers.append(name, headerDict[name]);
    assertEquals(headers.get(name), String(headerDict[name]));
  }
});

test(function headerSetSuccess(): void {
  const headers = new Headers();
  for (const name in headerDict) {
    headers.set(name, headerDict[name]);
    assertEquals(headers.get(name), String(headerDict[name]));
  }
});

test(function headerHasSuccess(): void {
  const headers = new Headers(headerDict);
  for (const name in headerDict) {
    assert(headers.has(name), "headers has name " + name);
    assert(
      !headers.has("nameNotInHeaders"),
      "headers do not have header: nameNotInHeaders"
    );
  }
});

test(function headerDeleteSuccess(): void {
  const headers = new Headers(headerDict);
  for (const name in headerDict) {
    assert(headers.has(name), "headers have a header: " + name);
    headers.delete(name);
    assert(!headers.has(name), "headers do not have anymore a header: " + name);
  }
});

test(function headerGetSuccess(): void {
  const headers = new Headers(headerDict);
  for (const name in headerDict) {
    assertEquals(headers.get(name), String(headerDict[name]));
    assertEquals(headers.get("nameNotInHeaders"), null);
  }
});

test(function headerEntriesSuccess(): void {
  const headers = new Headers(headerDict);
  const iterators = headers.entries();
  for (const it of iterators) {
    const key = it[0];
    const value = it[1];
    assert(headers.has(key));
    assertEquals(value, headers.get(key));
  }
});

test(function headerKeysSuccess(): void {
  const headers = new Headers(headerDict);
  const iterators = headers.keys();
  for (const it of iterators) {
    assert(headers.has(it));
  }
});

test(function headerValuesSuccess(): void {
  const headers = new Headers(headerDict);
  const iterators = headers.values();
  const entries = headers.entries();
  const values = [];
  for (const pair of entries) {
    values.push(pair[1]);
  }
  for (const it of iterators) {
    assert(values.includes(it));
  }
});

const headerEntriesDict = {
  name1: "value1",
  Name2: "value2",
  name: "value3",
  "content-Type": "value4",
  "Content-Typ": "value5",
  "Content-Types": "value6"
};

test(function headerForEachSuccess(): void {
  const headers = new Headers(headerEntriesDict);
  const keys = Object.keys(headerEntriesDict);
  keys.forEach(
    (key): void => {
      const value = headerEntriesDict[key];
      const newkey = key.toLowerCase();
      headerEntriesDict[newkey] = value;
    }
  );
  let callNum = 0;
  headers.forEach(
    (value, key, container): void => {
      assertEquals(headers, container);
      assertEquals(value, headerEntriesDict[key]);
      callNum++;
    }
  );
  assertEquals(callNum, keys.length);
});

test(function headerSymbolIteratorSuccess(): void {
  assert(Symbol.iterator in Headers.prototype);
  const headers = new Headers(headerEntriesDict);
  for (const header of headers) {
    const key = header[0];
    const value = header[1];
    assert(headers.has(key));
    assertEquals(value, headers.get(key));
  }
});

test(function headerTypesAvailable(): void {
  function newHeaders(): Headers {
    return new Headers();
  }
  const headers = newHeaders();
  assert(headers instanceof Headers);
});

// Modified from https://github.com/bitinn/node-fetch/blob/7d3293200a91ad52b5ca7962f9d6fd1c04983edb/test/test.js#L2001-L2014
// Copyright (c) 2016 David Frank. MIT License.
test(function headerIllegalReject(): void {
  let errorCount = 0;
  try {
    new Headers({ "He y": "ok" });
  } catch (e) {
    errorCount++;
  }
  try {
    new Headers({ "Hé-y": "ok" });
  } catch (e) {
    errorCount++;
  }
  try {
    new Headers({ "He-y": "ăk" });
  } catch (e) {
    errorCount++;
  }
  const headers = new Headers();
  try {
    headers.append("Hé-y", "ok");
  } catch (e) {
    errorCount++;
  }
  try {
    headers.delete("Hé-y");
  } catch (e) {
    errorCount++;
  }
  try {
    headers.get("Hé-y");
  } catch (e) {
    errorCount++;
  }
  try {
    headers.has("Hé-y");
  } catch (e) {
    errorCount++;
  }
  try {
    headers.set("Hé-y", "ok");
  } catch (e) {
    errorCount++;
  }
  try {
    headers.set("", "ok");
  } catch (e) {
    errorCount++;
  }
  assertEquals(errorCount, 9);
  // 'o k' is valid value but invalid name
  new Headers({ "He-y": "o k" });
});

// If pair does not contain exactly two items,then throw a TypeError.
test(function headerParamsShouldThrowTypeError(): void {
  let hasThrown = 0;

  try {
    new Headers(([["1"]] as unknown) as Array<[string, string]>);
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

test(function headerParamsArgumentsCheck(): void {
  const methodRequireOneParam = ["delete", "get", "has", "forEach"];

  const methodRequireTwoParams = ["append", "set"];

  methodRequireOneParam.forEach(
    (method): void => {
      const headers = new Headers();
      let hasThrown = 0;
      let errMsg = "";
      try {
        headers[method]();
        hasThrown = 1;
      } catch (err) {
        errMsg = err.message;
        if (err instanceof TypeError) {
          hasThrown = 2;
        } else {
          hasThrown = 3;
        }
      }
      assertEquals(hasThrown, 2);
      assertEquals(
        errMsg,
        `Headers.${method} requires at least 1 argument, but only 0 present`
      );
    }
  );

  methodRequireTwoParams.forEach(
    (method): void => {
      const headers = new Headers();
      let hasThrown = 0;
      let errMsg = "";

      try {
        headers[method]();
        hasThrown = 1;
      } catch (err) {
        errMsg = err.message;
        if (err instanceof TypeError) {
          hasThrown = 2;
        } else {
          hasThrown = 3;
        }
      }
      assertEquals(hasThrown, 2);
      assertEquals(
        errMsg,
        `Headers.${method} requires at least 2 arguments, but only 0 present`
      );

      hasThrown = 0;
      errMsg = "";
      try {
        headers[method]("foo");
        hasThrown = 1;
      } catch (err) {
        errMsg = err.message;
        if (err instanceof TypeError) {
          hasThrown = 2;
        } else {
          hasThrown = 3;
        }
      }
      assertEquals(hasThrown, 2);
      assertEquals(
        errMsg,
        `Headers.${method} requires at least 2 arguments, but only 1 present`
      );
    }
  );
});

test(function toStringShouldBeWebCompatibility(): void {
  const headers = new Headers();
  assertEquals(headers.toString(), "[object Headers]");
});
