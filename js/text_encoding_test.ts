// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { test, assert, assertEquals } from "./test_util.ts";

test(function atobSuccess() {
  const text = "hello world";
  const encoded = btoa(text);
  assertEquals(encoded, "aGVsbG8gd29ybGQ=");
});

test(function btoaSuccess() {
  const encoded = "aGVsbG8gd29ybGQ=";
  const decoded = atob(encoded);
  assertEquals(decoded, "hello world");
});

test(function btoaFailed() {
  const text = "你好";
  let err;
  try {
    btoa(text);
  } catch (e) {
    err = e;
  }
  assert(!!err);
  assertEquals(err.name, "InvalidInput");
});

test(function textDecoder2() {
  // prettier-ignore
  const fixture = new Uint8Array([
    0xf0, 0x9d, 0x93, 0xbd,
    0xf0, 0x9d, 0x93, 0xae,
    0xf0, 0x9d, 0x94, 0x81,
    0xf0, 0x9d, 0x93, 0xbd
  ]);
  const decoder = new TextDecoder();
  assertEquals(decoder.decode(fixture), "𝓽𝓮𝔁𝓽");
});

test(function textDecoderASCII() {
  const fixture = new Uint8Array([0x89, 0x95, 0x9f, 0xbf]);
  const decoder = new TextDecoder("ascii");
  assertEquals(decoder.decode(fixture), "‰•Ÿ¿");
});

test(function textDecoderErrorEncoding() {
  let didThrow = false;
  try {
    const decoder = new TextDecoder("foo");
  } catch (e) {
    didThrow = true;
    assertEquals(e.message, "The encoding label provided ('foo') is invalid.");
  }
  assert(didThrow);
});

test(function textEncoder2() {
  const fixture = "𝓽𝓮𝔁𝓽";
  const encoder = new TextEncoder();
  // prettier-ignore
  assertEquals(Array.from(encoder.encode(fixture)), [
    0xf0, 0x9d, 0x93, 0xbd,
    0xf0, 0x9d, 0x93, 0xae,
    0xf0, 0x9d, 0x94, 0x81,
    0xf0, 0x9d, 0x93, 0xbd
  ]);
});
