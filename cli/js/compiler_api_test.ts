// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

import { assert, assertEquals, unitTest } from "./test_util.ts";

const { compile, transpileOnly, bundle } = Deno;

unitTest(async function compilerApiCompileSources() {
  const [diagnostics, actual] = await compile("/foo.ts", {
    "/foo.ts": `import * as bar from "./bar.ts";\n\nconsole.log(bar);\n`,
    "/bar.ts": `export const bar = "bar";\n`
  });
  assert(diagnostics == null);
  assert(actual);
  assertEquals(Object.keys(actual), [
    "/bar.js.map",
    "/bar.js",
    "/foo.js.map",
    "/foo.js"
  ]);
});

unitTest(async function compilerApiCompileNoSources() {
  const [diagnostics, actual] = await compile("./cli/tests/subdir/mod1.ts");
  assert(diagnostics == null);
  assert(actual);
  const keys = Object.keys(actual);
  assertEquals(keys.length, 6);
  assert(keys[0].endsWith("print_hello.js.map"));
  assert(keys[1].endsWith("print_hello.js"));
});

unitTest(async function compilerApiCompileOptions() {
  const [diagnostics, actual] = await compile(
    "/foo.ts",
    {
      "/foo.ts": `export const foo = "foo";`
    },
    {
      module: "amd",
      sourceMap: false
    }
  );
  assert(diagnostics == null);
  assert(actual);
  assertEquals(Object.keys(actual), ["/foo.js"]);
  assert(actual["/foo.js"].startsWith("define("));
});

unitTest(async function compilerApiCompileLib() {
  const [diagnostics, actual] = await compile(
    "/foo.ts",
    {
      "/foo.ts": `console.log(document.getElementById("foo"));
        console.log(Deno.args);`
    },
    {
      lib: ["dom", "es2018", "deno.ns"]
    }
  );
  assert(diagnostics == null);
  assert(actual);
  assertEquals(Object.keys(actual), ["/foo.js.map", "/foo.js"]);
});

unitTest(async function compilerApiCompileTypes() {
  const [diagnostics, actual] = await compile(
    "/foo.ts",
    {
      "/foo.ts": `console.log(Foo.bar);`
    },
    {
      types: ["./cli/tests/subdir/foo_types.d.ts"]
    }
  );
  assert(diagnostics == null);
  assert(actual);
  assertEquals(Object.keys(actual), ["/foo.js.map", "/foo.js"]);
});

unitTest(async function transpileOnlyApi() {
  const actual = await transpileOnly({
    "foo.ts": `export enum Foo { Foo, Bar, Baz };\n`
  });
  assert(actual);
  assertEquals(Object.keys(actual), ["foo.ts"]);
  assert(actual["foo.ts"].source.startsWith("export var Foo;"));
  assert(actual["foo.ts"].map);
});

unitTest(async function transpileOnlyApiConfig() {
  const actual = await transpileOnly(
    {
      "foo.ts": `export enum Foo { Foo, Bar, Baz };\n`
    },
    {
      sourceMap: false,
      module: "amd"
    }
  );
  assert(actual);
  assertEquals(Object.keys(actual), ["foo.ts"]);
  assert(actual["foo.ts"].source.startsWith("define("));
  assert(actual["foo.ts"].map == null);
});

unitTest(async function bundleApiSources() {
  const [diagnostics, actual] = await bundle("/foo.ts", {
    "/foo.ts": `export * from "./bar.ts";\n`,
    "/bar.ts": `export const bar = "bar";\n`
  });
  assert(diagnostics == null);
  assert(actual.includes(`__instantiate("foo")`));
  assert(actual.includes(`__exp["bar"]`));
});

unitTest(async function bundleApiNoSources() {
  const [diagnostics, actual] = await bundle("./cli/tests/subdir/mod1.ts");
  assert(diagnostics == null);
  assert(actual.includes(`__instantiate("mod1")`));
  assert(actual.includes(`__exp["printHello3"]`));
});

unitTest(async function bundleApiConfig() {
  const [diagnostics, actual] = await bundle(
    "/foo.ts",
    {
      "/foo.ts": `// random comment\nexport * from "./bar.ts";\n`,
      "/bar.ts": `export const bar = "bar";\n`
    },
    {
      removeComments: true
    }
  );
  assert(diagnostics == null);
  assert(!actual.includes(`random`));
});

unitTest(async function bundleApiJsModules() {
  const [diagnostics, actual] = await bundle("/foo.js", {
    "/foo.js": `export * from "./bar.js";\n`,
    "/bar.js": `export const bar = "bar";\n`
  });
  assert(diagnostics == null);
  assert(actual.includes(`System.register("bar",`));
});

unitTest(async function diagnosticsTest() {
  const [diagnostics] = await compile("/foo.ts", {
    "/foo.ts": `document.getElementById("foo");`
  });
  assert(Array.isArray(diagnostics));
  assert(diagnostics.length === 1);
});
