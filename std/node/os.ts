// Copyright Joyent, Inc. and other Node contributors.
//
// Permission is hereby granted, free of charge, to any person obtaining a
// copy of this software and associated documentation files (the
// "Software"), to deal in the Software without restriction, including
// without limitation the rights to use, copy, modify, merge, publish,
// distribute, sublicense, and/or sell copies of the Software, and to permit
// persons to whom the Software is furnished to do so, subject to the
// following conditions:
//
// The above copyright notice and this permission notice shall be included
// in all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS
// OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
// MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN
// NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
// DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR
// OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE
// USE OR OTHER DEALINGS IN THE SOFTWARE.
import { notImplemented } from "./_utils.ts";
import { EOL as fsEOL } from "../fs/eol.ts";

const SEE_GITHUB_ISSUE = "See https://github.com/denoland/deno/issues/3802";

interface CPUTimes {
  /** The number of milliseconds the CPU has spent in user mode */
  user: number;

  /** The number of milliseconds the CPU has spent in nice mode */
  nice: number;

  /** The number of milliseconds the CPU has spent in sys mode */
  sys: number;

  /** The number of milliseconds the CPU has spent in idle mode */
  idle: number;

  /** The number of milliseconds the CPU has spent in irq mode */
  irq: number;
}

interface CPUCoreInfo {
  model: string;

  /** in MHz */
  speed: number;

  times: CPUTimes;
}

interface NetworkAddress {
  /** The assigned IPv4 or IPv6 address */
  address: string;

  /** The IPv4 or IPv6 network mask */
  netmask: string;

  family: "IPv4" | "IPv6";

  /** The MAC address of the network interface */
  mac: string;

  /** true if the network interface is a loopback or similar interface that is not remotely accessible; otherwise false */
  internal: boolean;

  /** The numeric IPv6 scope ID (only specified when family is IPv6) */
  scopeid?: number;

  /** The assigned IPv4 or IPv6 address with the routing prefix in CIDR notation. If the netmask is invalid, this property is set to null. */
  cidr: string;
}

interface NetworkInterfaces {
  [key: string]: NetworkAddress[];
}

export interface UserInfoOptions {
  encoding: string;
}

interface UserInfo {
  username: string;
  uid: number;
  gid: number;
  shell: string;
  homedir: string;
}

/** Returns the operating system CPU architecture for which the Deno binary was compiled */
export function arch(): string {
  return Deno.build.arch;
}

/** Not yet implemented */
export function cpus(): CPUCoreInfo[] {
  notImplemented(SEE_GITHUB_ISSUE);
}

/**
 * Returns a string identifying the endianness of the CPU for which the Deno
 * binary was compiled. Possible values are 'BE' for big endian and 'LE' for
 * little endian.
 **/
export function endianness(): "BE" | "LE" {
  // Source: https://developer.mozilla.org/en-US/docs/Web/JavaScript/Reference/Global_Objects/DataView#Endianness
  const buffer = new ArrayBuffer(2);
  new DataView(buffer).setInt16(0, 256, true /* littleEndian */);
  // Int16Array uses the platform's endianness.
  return new Int16Array(buffer)[0] === 256 ? "LE" : "BE";
}

/** Not yet implemented */
export function freemem(): number {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function getPriority(pid = 0): number {
  validateInt32(pid, "pid");
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Returns the string path of the current user's home directory. */
export function homedir(): string {
  return Deno.dir("home");
}

/** Returns the host name of the operating system as a string. */
export function hostname(): string {
  return Deno.hostname();
}

/** Not yet implemented */
export function loadavg(): number[] {
  if (Deno.build.os == "win") {
    return [0, 0, 0];
  }
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function networkInterfaces(): NetworkInterfaces {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function platform(): string {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function release(): string {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function setPriority(pid: number, priority?: number): void {
  /* The node API has the 'pid' as the first parameter and as optional.  
       This makes for a problematic implementation in Typescript. */
  if (priority === undefined) {
    priority = pid;
    pid = 0;
  }
  validateInt32(pid, "pid");
  validateInt32(priority, "priority", -20, 19);

  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function tmpdir(): string {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function totalmem(): number {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function type(): string {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function uptime(): number {
  notImplemented(SEE_GITHUB_ISSUE);
}

/** Not yet implemented */
export function userInfo(
  /* eslint-disable-next-line @typescript-eslint/no-unused-vars */
  options: UserInfoOptions = { encoding: "utf-8" }
): UserInfo {
  notImplemented(SEE_GITHUB_ISSUE);
}

export const constants = {
  // UV_UDP_REUSEADDR: 4,  //see https://nodejs.org/docs/latest-v12.x/api/os.html#os_libuv_constants
  dlopen: {
    // see https://nodejs.org/docs/latest-v12.x/api/os.html#os_dlopen_constants
  },
  errno: {
    // see https://nodejs.org/docs/latest-v12.x/api/os.html#os_error_constants
  },
  signals: Deno.Signal,
  priority: {
    // see https://nodejs.org/docs/latest-v12.x/api/os.html#os_priority_constants
  }
};

export const EOL = Deno.build.os == "win" ? fsEOL.CRLF : fsEOL.LF;

const validateInt32 = (
  value: number,
  name: string,
  min = -2147483648,
  max = 2147483647
): void => {
  // The defaults for min and max correspond to the limits of 32-bit integers.
  if (!Number.isInteger(value)) {
    throw new Error(`${name} must be 'an integer' but was ${value}`);
  }
  if (value < min || value > max) {
    throw new Error(
      `${name} must be >= ${min} && <= ${max}.  Value was ${value}`
    );
  }
};
