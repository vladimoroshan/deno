import {
  assertEquals,
  assertNotEquals,
  assertStringIncludes,
} from "./test_util.ts";

Deno.test(function customInspectFunction() {
  const blob = new DOMException("test");
  assertEquals(
    Deno.inspect(blob),
    `DOMException: test`,
  );
  assertStringIncludes(Deno.inspect(DOMException.prototype), "DOMException");
});

Deno.test(function nameToCodeMappingPrototypeAccess() {
  const newCode = 100;
  const objectPrototype = Object.prototype as unknown as {
    pollution: number;
  };
  objectPrototype.pollution = newCode;
  assertNotEquals(newCode, new DOMException("test", "pollution").code);
  Reflect.deleteProperty(objectPrototype, "pollution");
});
