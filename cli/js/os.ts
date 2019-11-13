// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
import { core } from "./core.ts";
import * as dispatch from "./dispatch.ts";
import { sendSync } from "./dispatch_json.ts";
import { assert } from "./util.ts";
import * as util from "./util.ts";
import { window } from "./window.ts";
import { OperatingSystem, Arch } from "./build.ts";

/** Check if running in terminal.
 *
 *       console.log(Deno.isTTY().stdout);
 */
export function isTTY(): { stdin: boolean; stdout: boolean; stderr: boolean } {
  return sendSync(dispatch.OP_IS_TTY);
}

/** Get the hostname.
 * Requires the `--allow-env` flag.
 *
 *       console.log(Deno.hostname());
 */
export function hostname(): string {
  return sendSync(dispatch.OP_HOSTNAME);
}

/** Exit the Deno process with optional exit code. */
export function exit(code = 0): never {
  sendSync(dispatch.OP_EXIT, { code });
  return util.unreachable();
}

function setEnv(key: string, value: string): void {
  sendSync(dispatch.OP_SET_ENV, { key, value });
}

function getEnv(key: string): string | undefined {
  return sendSync(dispatch.OP_GET_ENV, { key })[0];
}

/** Returns a snapshot of the environment variables at invocation. Mutating a
 * property in the object will set that variable in the environment for
 * the process. The environment object will only accept `string`s
 * as values.
 *
 *       console.log(Deno.env("SHELL"));
 *       const myEnv = Deno.env();
 *       console.log(myEnv.SHELL);
 *       myEnv.TEST_VAR = "HELLO";
 *       const newEnv = Deno.env();
 *       console.log(myEnv.TEST_VAR == newEnv.TEST_VAR);
 */
export function env(): { [index: string]: string };
export function env(key: string): string | undefined;
export function env(
  key?: string
): { [index: string]: string } | string | undefined {
  if (key) {
    return getEnv(key);
  }
  const env = sendSync(dispatch.OP_ENV);
  return new Proxy(env, {
    set(obj, prop: string, value: string): boolean {
      setEnv(prop, value);
      return Reflect.set(obj, prop, value);
    }
  });
}

interface Start {
  cwd: string;
  pid: number;
  argv: string[];
  mainModule: string; // Absolute URL.
  debugFlag: boolean;
  depsFlag: boolean;
  typesFlag: boolean;
  versionFlag: boolean;
  denoVersion: string;
  v8Version: string;
  tsVersion: string;
  noColor: boolean;
  os: OperatingSystem;
  arch: Arch;
}

// This function bootstraps an environment within Deno, it is shared both by
// the runtime and the compiler environments.
// @internal
export function start(preserveDenoNamespace = true, source?: string): Start {
  core.setAsyncHandler(dispatch.asyncMsgFromRust);
  const ops = core.ops();
  // TODO(bartlomieju): this is a prototype, we should come up with
  // something a bit more sophisticated
  for (const [name, opId] of Object.entries(ops)) {
    const opName = `OP_${name.toUpperCase()}`;
    // Assign op ids to actual variables
    dispatch[opName] = opId;
  }
  // First we send an empty `Start` message to let the privileged side know we
  // are ready. The response should be a `StartRes` message containing the CLI
  // args and other info.
  const startResponse = sendSync(dispatch.OP_START);
  const { pid, noColor, debugFlag } = startResponse;

  util.setLogDebug(debugFlag, source);

  // pid and noColor need to be set in the Deno module before it's set to be
  // frozen.
  util.immutableDefine(window.Deno, "pid", pid);
  util.immutableDefine(window.Deno, "noColor", noColor);
  Object.freeze(window.Deno);

  if (preserveDenoNamespace) {
    util.immutableDefine(window, "Deno", window.Deno);
    // Deno.core could ONLY be safely frozen here (not in globals.ts)
    // since shared_queue.js will modify core properties.
    Object.freeze(window.Deno.core);
    // core.sharedQueue is an object so we should also freeze it.
    Object.freeze(window.Deno.core.sharedQueue);
  } else {
    // Remove window.Deno
    delete window.Deno;
    assert(window.Deno === undefined);
  }

  return startResponse;
}

/**
 * Returns the current user's home directory.
 * Requires the `--allow-env` flag.
 */
export function homeDir(): string {
  const path = sendSync(dispatch.OP_HOME_DIR);
  if (!path) {
    throw new Error("Could not get home directory.");
  }
  return path;
}

/**
 * Returns the path to the current deno executable.
 * Requires the `--allow-env` flag.
 */
export function execPath(): string {
  return sendSync(dispatch.OP_EXEC_PATH);
}
