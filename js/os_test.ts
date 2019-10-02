// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import {
  test,
  testPerm,
  assert,
  assertEquals,
  assertNotEquals
} from "./test_util.ts";

testPerm({ env: true }, function envSuccess(): void {
  const env = Deno.env();
  assert(env !== null);
  // eslint-disable-next-line @typescript-eslint/camelcase
  env.test_var = "Hello World";
  const newEnv = Deno.env();
  assertEquals(env.test_var, newEnv.test_var);
  assertEquals(Deno.env("test_var"), env.test_var);
});

testPerm({ env: true }, function envNotFound(): void {
  const r = Deno.env("env_var_does_not_exist!");
  assertEquals(r, undefined);
});

test(function envPermissionDenied1(): void {
  let err;
  try {
    Deno.env();
  } catch (e) {
    err = e;
  }
  assertNotEquals(err, undefined);
  assertEquals(err.kind, Deno.ErrorKind.PermissionDenied);
  assertEquals(err.name, "PermissionDenied");
});

test(function envPermissionDenied2(): void {
  let err;
  try {
    Deno.env("PATH");
  } catch (e) {
    err = e;
  }
  assertNotEquals(err, undefined);
  assertEquals(err.kind, Deno.ErrorKind.PermissionDenied);
  assertEquals(err.name, "PermissionDenied");
});

if (Deno.build.os === "win") {
  // This test verifies that on Windows, environment variables are
  // case-insensitive. Case normalization needs be done using the collation
  // that Windows uses, rather than naively using String.toLowerCase().
  testPerm({ env: true, run: true }, async function envCaseInsensitive() {
    // Utility function that runs a Deno subprocess with the environment
    // specified in `inputEnv`. The subprocess reads the environment variables
    // which are in the keys of `expectedEnv` and writes them to stdout as JSON.
    // It is then verified that these match with the values of `expectedEnv`.
    const checkChildEnv = async (inputEnv, expectedEnv): Promise<void> => {
      const src = `
        console.log(
          ${JSON.stringify(Object.keys(expectedEnv))}.map(k => Deno.env(k))
        )`;
      const proc = Deno.run({
        args: [Deno.execPath(), "eval", src],
        env: inputEnv,
        stdout: "piped"
      });
      const status = await proc.status();
      assertEquals(status.success, true);
      const expectedValues = Object.values(expectedEnv);
      const actualValues = JSON.parse(
        new TextDecoder().decode(await proc.output())
      );
      assertEquals(actualValues, expectedValues);
    };

    assertEquals(Deno.env("path"), Deno.env("PATH"));
    assertEquals(Deno.env("Path"), Deno.env("PATH"));

    // Check 'foo', 'Foo' and 'Foo' are case folded.
    await checkChildEnv({ foo: "X" }, { foo: "X", Foo: "X", FOO: "X" });

    // Check that 'µ' and 'Μ' are not case folded.
    const lc1 = "µ";
    const uc1 = lc1.toUpperCase();
    assertNotEquals(lc1, uc1);
    await checkChildEnv(
      { [lc1]: "mu", [uc1]: "MU" },
      { [lc1]: "mu", [uc1]: "MU" }
    );

    // Check that 'ǆ' and 'Ǆ' are folded, but 'ǅ' is preserved.
    const c2 = "ǅ";
    const lc2 = c2.toLowerCase();
    const uc2 = c2.toUpperCase();
    assertNotEquals(c2, lc2);
    assertNotEquals(c2, uc2);
    await checkChildEnv(
      { [c2]: "Dz", [lc2]: "dz" },
      { [c2]: "Dz", [lc2]: "dz", [uc2]: "dz" }
    );
    await checkChildEnv(
      { [c2]: "Dz", [uc2]: "DZ" },
      { [c2]: "Dz", [uc2]: "DZ", [lc2]: "DZ" }
    );
  });
}

test(function osPid(): void {
  console.log("pid", Deno.pid);
  assert(Deno.pid > 0);
});

// See complete tests in tools/is_tty_test.py
test(function osIsTTYSmoke(): void {
  console.log(Deno.isTTY());
});

testPerm({ env: true }, function homeDir(): void {
  assertNotEquals(Deno.homeDir(), "");
});

testPerm({ env: false }, function homeDirPerm(): void {
  let caughtError = false;
  try {
    Deno.homeDir();
  } catch (err) {
    caughtError = true;
    assertEquals(err.kind, Deno.ErrorKind.PermissionDenied);
    assertEquals(err.name, "PermissionDenied");
  }
  assert(caughtError);
});

testPerm({ env: true }, function execPath(): void {
  assertNotEquals(Deno.execPath(), "");
});

testPerm({ env: false }, function execPathPerm(): void {
  let caughtError = false;
  try {
    Deno.execPath();
  } catch (err) {
    caughtError = true;
    assertEquals(err.kind, Deno.ErrorKind.PermissionDenied);
    assertEquals(err.name, "PermissionDenied");
  }
  assert(caughtError);
});

testPerm({ env: true }, function hostnameDir(): void {
  assertNotEquals(Deno.hostname(), "");
});

testPerm({ env: false }, function hostnamePerm(): void {
  let caughtError = false;
  try {
    Deno.hostname();
  } catch (err) {
    caughtError = true;
    assertEquals(err.kind, Deno.ErrorKind.PermissionDenied);
    assertEquals(err.name, "PermissionDenied");
  }
  assert(caughtError);
});
