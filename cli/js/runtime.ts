// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { core } from "./core.ts";
import * as dispatch from "./dispatch.ts";
import { sendSync } from "./dispatch_json.ts";
import { assert } from "./util.ts";
import * as util from "./util.ts";
import { OperatingSystem, Arch } from "./build.ts";
import { setBuildInfo } from "./build.ts";
import { setVersions } from "./version.ts";
import { setLocation } from "./location.ts";
import { setPrepareStackTrace } from "./error_stack.ts";

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

// TODO(bartlomieju): temporary solution, must be fixed when moving
// dispatches to separate crates
export function initOps(): void {
  const ops = core.ops();
  for (const [name, opId] of Object.entries(ops)) {
    const opName = `OP_${name.toUpperCase()}`;
    // Assign op ids to actual variables
    // TODO(ry) This type casting is gross and should be fixed.
    ((dispatch as unknown) as { [key: string]: number })[opName] = opId;
    core.setAsyncHandler(opId, dispatch.getAsyncHandler(opName));
  }
}

/**
 * This function bootstraps JS runtime, unfortunately some of runtime
 * code depends on information like "os" and thus getting this information
 * is required at startup.
 */
export function start(preserveDenoNamespace = true, source?: string): Start {
  initOps();
  // First we send an empty `Start` message to let the privileged side know we
  // are ready. The response should be a `StartRes` message containing the CLI
  // args and other info.
  const s = sendSync(dispatch.OP_START);

  setVersions(s.denoVersion, s.v8Version, s.tsVersion);
  setBuildInfo(s.os, s.arch);
  util.setLogDebug(s.debugFlag, source);

  // TODO(bartlomieju): this field should always be set
  if (s.mainModule) {
    assert(s.mainModule.length > 0);
    setLocation(s.mainModule);
  }
  setPrepareStackTrace(Error);

  // TODO(bartlomieju): I don't like that it's mixed in here, when
  // compiler and worker runtimes call this funtion and they don't use
  // Deno namespace (sans shared queue - Deno.core)

  // pid and noColor need to be set in the Deno module before it's set to be
  // frozen.
  util.immutableDefine(globalThis.Deno, "pid", s.pid);
  util.immutableDefine(globalThis.Deno, "noColor", s.noColor);
  Object.freeze(globalThis.Deno);

  if (preserveDenoNamespace) {
    util.immutableDefine(globalThis, "Deno", globalThis.Deno);
    // Deno.core could ONLY be safely frozen here (not in globals.ts)
    // since shared_queue.js will modify core properties.
    Object.freeze(globalThis.Deno.core);
    // core.sharedQueue is an object so we should also freeze it.
    Object.freeze(globalThis.Deno.core.sharedQueue);
  } else {
    // Remove globalThis.Deno
    delete globalThis.Deno;
    assert(globalThis.Deno === undefined);
  }

  return s;
}
