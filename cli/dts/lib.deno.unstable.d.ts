// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

/// <reference no-default-lib="true" />
/// <reference lib="deno.ns" />

declare namespace Deno {
  export interface BenchDefinition {
    fn: () => void | Promise<void>;
    name: string;
    ignore?: boolean;
    /** Group name for the benchmark.
     * Grouped benchmarks produce a time summary */
    group?: string;
    /** Benchmark should be used as the baseline for other benchmarks
     * If there are multiple baselines in a group, the first one is used as the baseline */
    baseline?: boolean;
    /** If at least one bench has `only` set to true, only run benches that have
     * `only` set to true and fail the bench suite. */
    only?: boolean;
    /** Ensure the bench case does not prematurely cause the process to exit,
     * for example via a call to `Deno.exit`. Defaults to true. */
    sanitizeExit?: boolean;

    /** Specifies the permissions that should be used to run the bench.
     * Set this to "inherit" to keep the calling thread's permissions.
     * Set this to "none" to revoke all permissions.
     *
     * Defaults to "inherit".
     */
    permissions?: Deno.PermissionOptions;
  }

  /** Register a bench which will be run when `deno bench` is used on the command
   * line and the containing module looks like a bench module.
   * `fn` can be async if required.
   * ```ts
   * import {assert, fail, assertEquals} from "https://deno.land/std/testing/asserts.ts";
   *
   * Deno.bench({
   *   name: "example test",
   *   fn(): void {
   *     assertEquals("world", "world");
   *   },
   * });
   *
   * Deno.bench({
   *   name: "example ignored test",
   *   ignore: Deno.build.os === "windows",
   *   fn(): void {
   *     // This test is ignored only on Windows machines
   *   },
   * });
   *
   * Deno.bench({
   *   name: "example async test",
   *   async fn() {
   *     const decoder = new TextDecoder("utf-8");
   *     const data = await Deno.readFile("hello_world.txt");
   *     assertEquals(decoder.decode(data), "Hello world");
   *   }
   * });
   * ```
   */
  export function bench(t: BenchDefinition): void;

  /** Register a bench which will be run when `deno bench` is used on the command
   * line and the containing module looks like a bench module.
   * `fn` can be async if required.
   *
   * ```ts
   * import {assert, fail, assertEquals} from "https://deno.land/std/testing/asserts.ts";
   *
   * Deno.bench("My test description", (): void => {
   *   assertEquals("hello", "hello");
   * });
   *
   * Deno.bench("My async test description", async (): Promise<void> => {
   *   const decoder = new TextDecoder("utf-8");
   *   const data = await Deno.readFile("hello_world.txt");
   *   assertEquals(decoder.decode(data), "Hello world");
   * });
   * ```
   */
  export function bench(
    name: string,
    fn: () => void | Promise<void>,
  ): void;

  /** Register a bench which will be run when `deno bench` is used on the command
   * line and the containing module looks like a bench module.
   * `fn` can be async if required. Declared function must have a name.
   *
   * ```ts
   * import {assert, fail, assertEquals} from "https://deno.land/std/testing/asserts.ts";
   *
   * Deno.bench(function myTestName(): void {
   *   assertEquals("hello", "hello");
   * });
   *
   * Deno.bench(async function myOtherTestName(): Promise<void> {
   *   const decoder = new TextDecoder("utf-8");
   *   const data = await Deno.readFile("hello_world.txt");
   *   assertEquals(decoder.decode(data), "Hello world");
   * });
   * ```
   */
  export function bench(fn: () => void | Promise<void>): void;

  /** Register a bench which will be run when `deno bench` is used on the command
   * line and the containing module looks like a bench module.
   * `fn` can be async if required.
   *
   * ```ts
   * import {assert, fail, assertEquals} from "https://deno.land/std/testing/asserts.ts";
   *
   * Deno.bench("My test description", { permissions: { read: true } }, (): void => {
   *   assertEquals("hello", "hello");
   * });
   *
   * Deno.bench("My async test description", { permissions: { read: false } }, async (): Promise<void> => {
   *   const decoder = new TextDecoder("utf-8");
   *   const data = await Deno.readFile("hello_world.txt");
   *   assertEquals(decoder.decode(data), "Hello world");
   * });
   * ```
   */
  export function bench(
    name: string,
    options: Omit<BenchDefinition, "fn" | "name">,
    fn: () => void | Promise<void>,
  ): void;

  /** Register a bench which will be run when `deno bench` is used on the command
   * line and the containing module looks like a bench module.
   * `fn` can be async if required.
   *
   * ```ts
   * import {assert, fail, assertEquals} from "https://deno.land/std/testing/asserts.ts";
   *
   * Deno.bench({ name: "My test description", permissions: { read: true } }, (): void => {
   *   assertEquals("hello", "hello");
   * });
   *
   * Deno.bench({ name: "My async test description", permissions: { read: false } }, async (): Promise<void> => {
   *   const decoder = new TextDecoder("utf-8");
   *   const data = await Deno.readFile("hello_world.txt");
   *   assertEquals(decoder.decode(data), "Hello world");
   * });
   * ```
   */
  export function bench(
    options: Omit<BenchDefinition, "fn">,
    fn: () => void | Promise<void>,
  ): void;

  /** Register a bench which will be run when `deno bench` is used on the command
   * line and the containing module looks like a bench module.
   * `fn` can be async if required. Declared function must have a name.
   *
   * ```ts
   * import {assert, fail, assertEquals} from "https://deno.land/std/testing/asserts.ts";
   *
   * Deno.bench({ permissions: { read: true } }, function myTestName(): void {
   *   assertEquals("hello", "hello");
   * });
   *
   * Deno.bench({ permissions: { read: false } }, async function myOtherTestName(): Promise<void> {
   *   const decoder = new TextDecoder("utf-8");
   *   const data = await Deno.readFile("hello_world.txt");
   *   assertEquals(decoder.decode(data), "Hello world");
   * });
   * ```
   */
  export function bench(
    options: Omit<BenchDefinition, "fn" | "name">,
    fn: () => void | Promise<void>,
  ): void;

  /**
   * **UNSTABLE**: New API, yet to be vetted.  This API is under consideration to
   * determine if permissions are required to call it.
   *
   * Retrieve the process umask.  If `mask` is provided, sets the process umask.
   * This call always returns what the umask was before the call.
   *
   * ```ts
   * console.log(Deno.umask());  // e.g. 18 (0o022)
   * const prevUmaskValue = Deno.umask(0o077);  // e.g. 18 (0o022)
   * console.log(Deno.umask());  // e.g. 63 (0o077)
   * ```
   *
   * NOTE:  This API is not implemented on Windows
   */
  export function umask(mask?: number): number;

  /** **UNSTABLE**: New API, yet to be vetted.
   *
   * Gets the size of the console as columns/rows.
   *
   * ```ts
   * const { columns, rows } = Deno.consoleSize(Deno.stdout.rid);
   * ```
   */
  export function consoleSize(
    rid: number,
  ): {
    columns: number;
    rows: number;
  };

  /** **Unstable**  There are questions around which permission this needs. And
   * maybe should be renamed (loadAverage?)
   *
   * Returns an array containing the 1, 5, and 15 minute load averages. The
   * load average is a measure of CPU and IO utilization of the last one, five,
   * and 15 minute periods expressed as a fractional number.  Zero means there
   * is no load. On Windows, the three values are always the same and represent
   * the current load, not the 1, 5 and 15 minute load averages.
   *
   * ```ts
   * console.log(Deno.loadavg());  // e.g. [ 0.71, 0.44, 0.44 ]
   * ```
   *
   * Requires `allow-env` permission.
   */
  export function loadavg(): number[];

  /** **Unstable** new API. yet to be vetted. Under consideration to possibly move to
   * Deno.build or Deno.versions and if it should depend sys-info, which may not
   * be desireable.
   *
   * Returns the release version of the Operating System.
   *
   * ```ts
   * console.log(Deno.osRelease());
   * ```
   *
   * Requires `allow-env` permission.
   */
  export function osRelease(): string;

  /** **Unstable** new API. yet to be vetted.
   *
   * Displays the total amount of free and used physical and swap memory in the
   * system, as well as the buffers and caches used by the kernel.
   *
   * This is similar to the `free` command in Linux
   *
   * ```ts
   * console.log(Deno.systemMemoryInfo());
   * ```
   *
   * Requires `allow-env` permission.
   */
  export function systemMemoryInfo(): SystemMemoryInfo;

  export interface SystemMemoryInfo {
    /** Total installed memory */
    total: number;
    /** Unused memory */
    free: number;
    /** Estimation of how much memory is available  for  starting  new
     * applications, without  swapping. Unlike the data provided by the cache or
     * free fields, this field takes into account page cache and also that not
     * all reclaimable memory slabs will be reclaimed due to items being in use
     */
    available: number;
    /** Memory used by kernel buffers */
    buffers: number;
    /** Memory  used  by  the  page  cache  and  slabs */
    cached: number;
    /** Total swap memory */
    swapTotal: number;
    /** Unused swap memory */
    swapFree: number;
  }

  /** The information of the network interface */
  export interface NetworkInterfaceInfo {
    /** The network interface name */
    name: string;
    /** The IP protocol version */
    family: "IPv4" | "IPv6";
    /** The IP address */
    address: string;
    /** The netmask */
    netmask: string;
    /** The IPv6 scope id or null */
    scopeid: number | null;
    /** The CIDR range */
    cidr: string;
    /** The MAC address */
    mac: string;
  }

  /** **Unstable** new API. yet to be vetted.
   *
   * Returns an array of the network interface informations.
   *
   * ```ts
   * console.log(Deno.networkInterfaces());
   * ```
   *
   * Requires `allow-env` permission.
   */
  export function networkInterfaces(): NetworkInterfaceInfo[];

  /** **Unstable** new API. yet to be vetted.
   *
   * Returns the user id of the process on POSIX platforms. Returns null on windows.
   *
   * ```ts
   * console.log(Deno.getUid());
   * ```
   *
   * Requires `allow-env` permission.
   */
  export function getUid(): number | null;

  /** **Unstable** new API. yet to be vetted.
   *
   * Returns the group id of the process on POSIX platforms. Returns null on windows.
   *
   * ```ts
   * console.log(Deno.getGid());
   * ```
   *
   * Requires `allow-env` permission.
   */
  export function getGid(): number | null;

  /** All possible types for interfacing with foreign functions */
  export type NativeType =
    | "void"
    | "u8"
    | "i8"
    | "u16"
    | "i16"
    | "u32"
    | "i32"
    | "u64"
    | "i64"
    | "usize"
    | "isize"
    | "f32"
    | "f64"
    | "pointer";

  /** A foreign function as defined by its parameter and result types */
  export interface ForeignFunction<
    Parameters extends readonly NativeType[] = readonly NativeType[],
    Result extends NativeType = NativeType,
    NonBlocking extends boolean = boolean,
  > {
    /** Name of the symbol, defaults to the key name in symbols object. */
    name?: string;
    parameters: Parameters;
    result: Result;
    /** When true, function calls will run on a dedicated blocking thread and will return a Promise resolving to the `result`. */
    nonblocking?: NonBlocking;
  }

  export interface ForeignStatic<Type extends NativeType = NativeType> {
    /** Name of the symbol, defaults to the key name in symbols object. */
    name?: string;
    type: Exclude<Type, "void">;
  }

  /** A foreign library interface descriptor */
  export interface ForeignLibraryInterface {
    [name: string]: ForeignFunction | ForeignStatic;
  }

  /** All possible number types interfacing with foreign functions */
  type StaticNativeNumberType = Exclude<
    NativeType,
    "void" | "pointer" | StaticNativeBigIntType
  >;

  /** All possible bigint types interfacing with foreign functions */
  type StaticNativeBigIntType = "u64" | "i64" | "usize" | "isize";

  /** Infers a foreign function return type */
  type StaticForeignFunctionResult<T extends NativeType> = T extends "void"
    ? void
    : T extends StaticNativeBigIntType ? bigint
    : T extends StaticNativeNumberType ? number
    : T extends "pointer" ? UnsafePointer
    : never;

  type StaticForeignFunctionParameter<T> = T extends "void" ? void
    : T extends StaticNativeNumberType | StaticNativeBigIntType
      ? number | bigint
    : T extends "pointer" ? Deno.UnsafePointer | Deno.TypedArray | null
    : unknown;

  /** Infers a foreign function parameter list. */
  type StaticForeignFunctionParameters<T extends readonly NativeType[]> = [
    ...{
      [K in keyof T]: StaticForeignFunctionParameter<T[K]>;
    },
  ];

  /** Infers a foreign symbol */
  type StaticForeignSymbol<T extends ForeignFunction | ForeignStatic> =
    T extends ForeignFunction ? (
      ...args: StaticForeignFunctionParameters<T["parameters"]>
    ) => ConditionalAsync<
      T["nonblocking"],
      StaticForeignFunctionResult<T["result"]>
    >
      : T extends ForeignStatic ? StaticForeignFunctionResult<T["type"]>
      : never;

  type ConditionalAsync<IsAsync extends boolean | undefined, T> =
    IsAsync extends true ? Promise<T> : T;

  /** Infers a foreign library interface */
  type StaticForeignLibraryInterface<T extends ForeignLibraryInterface> = {
    [K in keyof T]: StaticForeignSymbol<T[K]>;
  };

  type TypedArray =
    | Int8Array
    | Uint8Array
    | Int16Array
    | Uint16Array
    | Int32Array
    | Uint32Array
    | Uint8ClampedArray
    | Float32Array
    | Float64Array
    | BigInt64Array
    | BigUint64Array;

  /** **UNSTABLE**: Unsafe and new API, beware!
   *
   * An unsafe pointer to a memory location for passing and returning pointers to and from the ffi
   */
  export class UnsafePointer {
    constructor(value: bigint);

    value: bigint;

    /**
     * Return the direct memory pointer to the typed array in memory
     */
    static of(typedArray: TypedArray): UnsafePointer;

    /**
     * Returns the value of the pointer which is useful in certain scenarios.
     */
    valueOf(): bigint;
  }

  /** **UNSTABLE**: Unsafe and new API, beware!
   *
   * An unsafe pointer view to a memory location as specified by the `pointer`
   * value. The `UnsafePointerView` API mimics the standard built in interface
   * `DataView` for accessing the underlying types at an memory location
   * (numbers, strings and raw bytes).
   */
  export class UnsafePointerView {
    constructor(pointer: UnsafePointer);

    pointer: UnsafePointer;

    /** Gets an unsigned 8-bit integer at the specified byte offset from the pointer. */
    getUint8(offset?: number): number;
    /** Gets a signed 8-bit integer at the specified byte offset from the pointer. */
    getInt8(offset?: number): number;
    /** Gets an unsigned 16-bit integer at the specified byte offset from the pointer. */
    getUint16(offset?: number): number;
    /** Gets a signed 16-bit integer at the specified byte offset from the pointer. */
    getInt16(offset?: number): number;
    /** Gets an unsigned 32-bit integer at the specified byte offset from the pointer. */
    getUint32(offset?: number): number;
    /** Gets a signed 32-bit integer at the specified byte offset from the pointer. */
    getInt32(offset?: number): number;
    /** Gets an unsigned 64-bit integer at the specified byte offset from the pointer. */
    getBigUint64(offset?: number): bigint;
    /** Gets a signed 64-bit integer at the specified byte offset from the pointer. */
    getBigInt64(offset?: number): bigint;
    /** Gets a signed 32-bit float at the specified byte offset from the pointer. */
    getFloat32(offset?: number): number;
    /** Gets a signed 64-bit float at the specified byte offset from the pointer. */
    getFloat64(offset?: number): number;
    /** Gets a C string (null terminated string) at the specified byte offset from the pointer. */
    getCString(offset?: number): string;
    /** Gets an ArrayBuffer of length `byteLength` at the specified byte offset from the pointer. */
    getArrayBuffer(byteLength: number, offset?: number): ArrayBuffer;
    /** Copies the memory of the pointer into a typed array. Length is determined from the typed array's `byteLength`. Also takes optional offset from the pointer. */
    copyInto(destination: TypedArray, offset?: number): void;
  }

  /**
   * **UNSTABLE**: Unsafe and new API, beware!
   *
   * An unsafe pointer to a function, for calling functions that are not
   * present as symbols.
   */
  export class UnsafeFnPointer<Fn extends ForeignFunction> {
    pointer: UnsafePointer;
    definition: Fn;

    constructor(pointer: UnsafePointer, definition: Fn);

    call(
      ...args: StaticForeignFunctionParameters<Fn["parameters"]>
    ): ConditionalAsync<
      Fn["nonblocking"],
      StaticForeignFunctionResult<Fn["result"]>
    >;
  }

  /** A dynamic library resource */
  export interface DynamicLibrary<S extends ForeignLibraryInterface> {
    /** All of the registered library along with functions for calling them */
    symbols: StaticForeignLibraryInterface<S>;
    close(): void;
  }

  /** **UNSTABLE**: Unsafe and new API, beware!
   *
   * Opens a dynamic library and registers symbols
   */
  export function dlopen<S extends ForeignLibraryInterface>(
    filename: string | URL,
    symbols: S,
  ): DynamicLibrary<S>;

  /** The log category for a diagnostic message. */
  export enum DiagnosticCategory {
    Warning = 0,
    Error = 1,
    Suggestion = 2,
    Message = 3,
  }

  export interface DiagnosticMessageChain {
    messageText: string;
    category: DiagnosticCategory;
    code: number;
    next?: DiagnosticMessageChain[];
  }

  export interface Diagnostic {
    /** A string message summarizing the diagnostic. */
    messageText?: string;
    /** An ordered array of further diagnostics. */
    messageChain?: DiagnosticMessageChain;
    /** Information related to the diagnostic. This is present when there is a
     * suggestion or other additional diagnostic information */
    relatedInformation?: Diagnostic[];
    /** The text of the source line related to the diagnostic. */
    sourceLine?: string;
    source?: string;
    /** The start position of the error. Zero based index. */
    start?: {
      line: number;
      character: number;
    };
    /** The end position of the error.  Zero based index. */
    end?: {
      line: number;
      character: number;
    };
    /** The filename of the resource related to the diagnostic message. */
    fileName?: string;
    /** The category of the diagnostic. */
    category: DiagnosticCategory;
    /** A number identifier. */
    code: number;
  }

  export type SetRawOptions = {
    cbreak: boolean;
  };

  /** **UNSTABLE**: new API, yet to be vetted
   *
   * Set TTY to be under raw mode or not. In raw mode, characters are read and
   * returned as is, without being processed. All special processing of
   * characters by the terminal is disabled, including echoing input characters.
   * Reading from a TTY device in raw mode is faster than reading from a TTY
   * device in canonical mode.
   *
   * The `cbreak` option can be used to indicate that characters that correspond
   * to a signal should still be generated. When disabling raw mode, this option
   * is ignored. This functionality currently only works on Linux and Mac OS.
   *
   * ```ts
   * Deno.setRaw(Deno.stdin.rid, true, { cbreak: true });
   * ```
   */
  export function setRaw(
    rid: number,
    mode: boolean,
    options?: SetRawOptions,
  ): void;

  /** **UNSTABLE**: needs investigation into high precision time.
   *
   * Synchronously changes the access (`atime`) and modification (`mtime`) times
   * of a file system object referenced by `path`. Given times are either in
   * seconds (UNIX epoch time) or as `Date` objects.
   *
   * ```ts
   * Deno.utimeSync("myfile.txt", 1556495550, new Date());
   * ```
   *
   * Requires `allow-write` permission. */
  export function utimeSync(
    path: string | URL,
    atime: number | Date,
    mtime: number | Date,
  ): void;

  /** **UNSTABLE**: needs investigation into high precision time.
   *
   * Changes the access (`atime`) and modification (`mtime`) times of a file
   * system object referenced by `path`. Given times are either in seconds
   * (UNIX epoch time) or as `Date` objects.
   *
   * ```ts
   * await Deno.utime("myfile.txt", 1556495550, new Date());
   * ```
   *
   * Requires `allow-write` permission. */
  export function utime(
    path: string | URL,
    atime: number | Date,
    mtime: number | Date,
  ): Promise<void>;

  export function run<
    T extends RunOptions & {
      clearEnv?: boolean;
      gid?: number;
      uid?: number;
    } = RunOptions & {
      clearEnv?: boolean;
      gid?: number;
      uid?: number;
    },
  >(opt: T): Process<T>;

  /**  **UNSTABLE**: New API, yet to be vetted.  Additional consideration is still
   * necessary around the permissions required.
   *
   * Get the `hostname` of the machine the Deno process is running on.
   *
   * ```ts
   * console.log(Deno.hostname());
   * ```
   *
   *  Requires `allow-env` permission.
   */
  export function hostname(): string;

  /** **UNSTABLE**: New API, yet to be vetted.
   * A custom HttpClient for use with `fetch`.
   *
   * ```ts
   * const caCert = await Deno.readTextFile("./ca.pem");
   * const client = Deno.createHttpClient({ caCerts: [ caCert ] });
   * const req = await fetch("https://myserver.com", { client });
   * ```
   */
  export class HttpClient {
    rid: number;
    close(): void;
  }

  /** **UNSTABLE**: New API, yet to be vetted.
   * The options used when creating a [HttpClient].
   */
  export interface CreateHttpClientOptions {
    /** A list of root certificates that will be used in addition to the
     * default root certificates to verify the peer's certificate.
     *
     * Must be in PEM format. */
    caCerts?: string[];
    /** A HTTP proxy to use for new connections. */
    proxy?: Proxy;
    /** PEM formatted client certificate chain. */
    certChain?: string;
    /** PEM formatted (RSA or PKCS8) private key of client certificate. */
    privateKey?: string;
  }

  export interface Proxy {
    url: string;
    basicAuth?: BasicAuth;
  }

  export interface BasicAuth {
    username: string;
    password: string;
  }

  /** **UNSTABLE**: New API, yet to be vetted.
   * Create a custom HttpClient for to use with `fetch`.
   *
   * ```ts
   * const caCert = await Deno.readTextFile("./ca.pem");
   * const client = Deno.createHttpClient({ caCerts: [ caCert ] });
   * const response = await fetch("https://myserver.com", { client });
   * ```
   *
   * ```ts
   * const client = Deno.createHttpClient({ proxy: { url: "http://myproxy.com:8080" } });
   * const response = await fetch("https://myserver.com", { client });
   * ```
   */
  export function createHttpClient(
    options: CreateHttpClientOptions,
  ): HttpClient;

  /** **UNSTABLE**: needs investigation into high precision time.
   *
   * Synchronously changes the access (`atime`) and modification (`mtime`) times
   * of a file stream resource referenced by `rid`. Given times are either in
   * seconds (UNIX epoch time) or as `Date` objects.
   *
   * ```ts
   * const file = Deno.openSync("file.txt", { create: true, write: true });
   * Deno.futimeSync(file.rid, 1556495550, new Date());
   * ```
   */
  export function futimeSync(
    rid: number,
    atime: number | Date,
    mtime: number | Date,
  ): void;

  /** **UNSTABLE**: needs investigation into high precision time.
   *
   * Changes the access (`atime`) and modification (`mtime`) times of a file
   * stream resource referenced by `rid`. Given times are either in seconds
   * (UNIX epoch time) or as `Date` objects.
   *
   * ```ts
   * const file = await Deno.open("file.txt", { create: true, write: true });
   * await Deno.futime(file.rid, 1556495550, new Date());
   * ```
   */
  export function futime(
    rid: number,
    atime: number | Date,
    mtime: number | Date,
  ): Promise<void>;

  /** **UNSTABLE**: new API, yet to be vetted.
   *
   * A generic transport listener for message-oriented protocols. */
  export interface DatagramConn extends AsyncIterable<[Uint8Array, Addr]> {
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Waits for and resolves to the next message to the `UDPConn`. */
    receive(p?: Uint8Array): Promise<[Uint8Array, Addr]>;
    /** UNSTABLE: new API, yet to be vetted.
     *
     * Sends a message to the target. */
    send(p: Uint8Array, addr: Addr): Promise<number>;
    /** UNSTABLE: new API, yet to be vetted.
     *
     * Close closes the socket. Any pending message promises will be rejected
     * with errors. */
    close(): void;
    /** Return the address of the `UDPConn`. */
    readonly addr: Addr;
    [Symbol.asyncIterator](): AsyncIterableIterator<[Uint8Array, Addr]>;
  }

  export interface UnixListenOptions {
    /** A Path to the Unix Socket. */
    path: string;
  }

  /** **UNSTABLE**: new API, yet to be vetted.
   *
   * Listen announces on the local transport address.
   *
   * ```ts
   * const listener = Deno.listen({ path: "/foo/bar.sock", transport: "unix" })
   * ```
   *
   * Requires `allow-read` and `allow-write` permission. */
  export function listen(
    options: UnixListenOptions & { transport: "unix" },
  ): Listener;

  /** **UNSTABLE**: new API, yet to be vetted
   *
   * Listen announces on the local transport address.
   *
   * ```ts
   * const listener1 = Deno.listenDatagram({
   *   port: 80,
   *   transport: "udp"
   * });
   * const listener2 = Deno.listenDatagram({
   *   hostname: "golang.org",
   *   port: 80,
   *   transport: "udp"
   * });
   * ```
   *
   * Requires `allow-net` permission. */
  export function listenDatagram(
    options: ListenOptions & { transport: "udp" },
  ): DatagramConn;

  /** **UNSTABLE**: new API, yet to be vetted
   *
   * Listen announces on the local transport address.
   *
   * ```ts
   * const listener = Deno.listenDatagram({
   *   path: "/foo/bar.sock",
   *   transport: "unixpacket"
   * });
   * ```
   *
   * Requires `allow-read` and `allow-write` permission. */
  export function listenDatagram(
    options: UnixListenOptions & { transport: "unixpacket" },
  ): DatagramConn;

  export interface UnixConnectOptions {
    transport: "unix";
    path: string;
  }

  /** **UNSTABLE**:  The unix socket transport is unstable as a new API yet to
   * be vetted.  The TCP transport is considered stable.
   *
   * Connects to the hostname (default is "127.0.0.1") and port on the named
   * transport (default is "tcp"), and resolves to the connection (`Conn`).
   *
   * ```ts
   * const conn1 = await Deno.connect({ port: 80 });
   * const conn2 = await Deno.connect({ hostname: "192.0.2.1", port: 80 });
   * const conn3 = await Deno.connect({ hostname: "[2001:db8::1]", port: 80 });
   * const conn4 = await Deno.connect({ hostname: "golang.org", port: 80, transport: "tcp" });
   * const conn5 = await Deno.connect({ path: "/foo/bar.sock", transport: "unix" });
   * ```
   *
   * Requires `allow-net` permission for "tcp" and `allow-read` for "unix". */
  export function connect(
    options: ConnectOptions,
  ): Promise<TcpConn>;
  export function connect(
    options: UnixConnectOptions,
  ): Promise<UnixConn>;

  export interface ConnectTlsOptions {
    /** PEM formatted client certificate chain. */
    certChain?: string;
    /** PEM formatted (RSA or PKCS8) private key of client certificate. */
    privateKey?: string;
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Application-Layer Protocol Negotiation (ALPN) protocols supported by
     * the client. If not specified, no ALPN extension will be included in the
     * TLS handshake.
     */
    alpnProtocols?: string[];
  }

  export interface TlsHandshakeInfo {
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Contains the ALPN protocol selected during negotiation with the server.
     * If no ALPN protocol selected, returns `null`.
     */
    alpnProtocol: string | null;
  }

  export interface TlsConn extends Conn {
    /** Runs the client or server handshake protocol to completion if that has
     * not happened yet. Calling this method is optional; the TLS handshake
     * will be completed automatically as soon as data is sent or received. */
    handshake(): Promise<TlsHandshakeInfo>;
  }

  /** **UNSTABLE** New API, yet to be vetted.
   *
   * Create a TLS connection with an attached client certificate.
   *
   * ```ts
   * const conn = await Deno.connectTls({
   *   hostname: "deno.land",
   *   port: 443,
   *   certChain: "---- BEGIN CERTIFICATE ----\n ...",
   *   privateKey: "---- BEGIN PRIVATE KEY ----\n ...",
   * });
   * ```
   *
   * Requires `allow-net` permission.
   */
  export function connectTls(options: ConnectTlsOptions): Promise<TlsConn>;

  export interface ListenTlsOptions {
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Application-Layer Protocol Negotiation (ALPN) protocols to announce to
     * the client. If not specified, no ALPN extension will be included in the
     * TLS handshake.
     */
    alpnProtocols?: string[];
  }

  export interface StartTlsOptions {
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Application-Layer Protocol Negotiation (ALPN) protocols to announce to
     * the client. If not specified, no ALPN extension will be included in the
     * TLS handshake.
     */
    alpnProtocols?: string[];
  }

  export interface Listener extends AsyncIterable<Conn> {
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Make the listener block the event loop from finishing.
     *
     * Note: the listener blocks the event loop from finishing by default.
     * This method is only meaningful after `.unref()` is called.
     */
    ref(): void;
    /** **UNSTABLE**: new API, yet to be vetted.
     *
     * Make the listener not block the event loop from finishing.
     */
    unref(): void;
  }

  /** **UNSTABLE**: New API should be tested first.
   *
   * Acquire an advisory file-system lock for the provided file. `exclusive`
   * defaults to `false`.
   */
  export function flock(rid: number, exclusive?: boolean): Promise<void>;

  /** **UNSTABLE**: New API should be tested first.
   *
   * Acquire an advisory file-system lock for the provided file. `exclusive`
   * defaults to `false`.
   */
  export function flockSync(rid: number, exclusive?: boolean): void;

  /** **UNSTABLE**: New API should be tested first.
   *
   * Release an advisory file-system lock for the provided file.
   */
  export function funlock(rid: number): Promise<void>;

  /** **UNSTABLE**: New API should be tested first.
   *
   * Release an advisory file-system lock for the provided file.
   */
  export function funlockSync(rid: number): void;

  /** **UNSTABLE**: new API, yet to be vetted.
   *
   * Make the timer of the given id blocking the event loop from finishing
   */
  export function refTimer(id: number): void;

  /** **UNSTABLE**: new API, yet to be vetted.
   *
   * Make the timer of the given id not blocking the event loop from finishing
   */
  export function unrefTimer(id: number): void;

  /** **UNSTABLE**: new API, yet to be vetter.
   *
   * Allows to "hijack" a connection that the request is associated with.
   * Can be used to implement protocols that build on top of HTTP (eg.
   * WebSockets).
   *
   * The returned promise returns underlying connection and first packet
   * received. The promise shouldn't be awaited before responding to the
   * `request`, otherwise event loop might deadlock.
   */
  export function upgradeHttp(
    request: Request,
  ): Promise<[Deno.Conn, Uint8Array]>;

  export interface SpawnOptions {
    /** Arguments to pass to the process. */
    args?: string[];
    /**
     * The working directory of the process.
     * If not specified, the cwd of the parent process is used.
     */
    cwd?: string | URL;
    /**
     * Clear environmental variables from parent process.
     * Doesn't guarantee that only `opt.env` variables are present,
     * as the OS may set environmental variables for processes.
     */
    clearEnv?: boolean;
    /** Environmental variables to pass to the subprocess. */
    env?: Record<string, string>;
    /**
     * Sets the child process’s user ID. This translates to a setuid call
     * in the child process. Failure in the setuid call will cause the spawn to fail.
     */
    uid?: number;
    /** Similar to `uid`, but sets the group ID of the child process. */
    gid?: number;
    /**
     * An AbortSignal that allows closing the process using the corresponding
     * AbortController by sending the process a SIGTERM signal.
     * Not Supported by execSync.
     */
    signal?: AbortSignal;

    /** Defaults to "null". */
    stdin?: "piped" | "inherit" | "null";
    /** Defaults to "piped". */
    stdout?: "piped" | "inherit" | "null";
    /** Defaults to "piped". */
    stderr?: "piped" | "inherit" | "null";
  }

  /**
   * Spawns a child process.
   *
   * If stdin is set to "piped", the stdin WritableStream needs to be closed manually.
   *
   * ```ts
   * const child = Deno.spawnChild(Deno.execPath(), {
   *   args: [
   *     "eval",
   *     "console.log('Hello World')",
   *   ],
   *   stdin: "piped",
   * });
   *
   * // open a file and pipe the subprocess output to it.
   * child.stdout.pipeTo(Deno.openSync("output").writable);
   *
   * // manually close stdin
   * child.stdin.close();
   * const status = await child.status;
   * ```
   */
  export function spawnChild<T extends SpawnOptions = SpawnOptions>(
    command: string | URL,
    options?: T,
  ): Child<T>;

  export class Child<T extends SpawnOptions> {
    readonly stdin: T["stdin"] extends "piped" ? WritableStream<Uint8Array>
      : null;
    readonly stdout: T["stdout"] extends "inherit" | "null" ? null
      : ReadableStream<Uint8Array>;
    readonly stderr: T["stderr"] extends "inherit" | "null" ? null
      : ReadableStream<Uint8Array>;

    readonly pid: number;
    /** Get the status of the child. */
    readonly status: Promise<ChildStatus>;

    /** Waits for the child to exit completely, returning all its output and status. */
    output(): Promise<SpawnOutput<T>>;
    /** Kills the process with given Signal. Defaults to SIGTERM. */
    kill(signo?: Signal): void;
  }

  /**
   * Executes a subprocess, waiting for it to finish and
   * collecting all of its output.
   * Will throw an error if `stdin: "piped"` is passed.
   *
   * ```ts
   * const { status, stdout, stderr } = await Deno.spawn(Deno.execPath(), {
   *   args: [
   *     "eval",
   *        "console.log('hello'); console.error('world')",
   *   ],
   * });
   * console.assert(status.code === 0);
   * console.assert("hello\n" === new TextDecoder().decode(stdout));
   * console.assert("world\n" === new TextDecoder().decode(stderr));
   * ```
   */
  export function spawn<T extends SpawnOptions = SpawnOptions>(
    command: string | URL,
    options?: T,
  ): Promise<SpawnOutput<T>>;

  /**
   * Synchronously executes a subprocess, waiting for it to finish and
   * collecting all of its output.
   * Will throw an error if `stdin: "piped"` is passed.
   *
   * ```ts
   * const { status, stdout, stderr } = Deno.spawnSync(Deno.execPath(), {
   *   args: [
   *     "eval",
   *       "console.log('hello'); console.error('world')",
   *   ],
   * });
   * console.assert(status.code === 0);
   * console.assert("hello\n" === new TextDecoder().decode(stdout));
   * console.assert("world\n" === new TextDecoder().decode(stderr));
   * ```
   */
  export function spawnSync<T extends SpawnOptions = SpawnOptions>(
    command: string | URL,
    options?: T,
  ): SpawnOutput<T>;

  export type ChildStatus =
    | {
      success: true;
      code: 0;
      signal: null;
    }
    | {
      success: false;
      code: number;
      signal: Signal | null;
    };

  export interface SpawnOutput<T extends SpawnOptions> {
    status: ChildStatus;
    stdout: T["stdout"] extends "inherit" | "null" ? null : Uint8Array;
    stderr: T["stderr"] extends "inherit" | "null" ? null : Uint8Array;
  }
}

declare function fetch(
  input: Request | URL | string,
  init?: RequestInit & { client: Deno.HttpClient },
): Promise<Response>;

declare interface WorkerOptions {
  /** UNSTABLE: New API.
   *
   * Configure permissions options to change the level of access the worker will
   * have. By default it will have no permissions. Note that the permissions
   * of a worker can't be extended beyond its parent's permissions reach.
   * - "inherit" will take the permissions of the thread the worker is created in
   * - "none" will use the default behavior and have no permission
   * - You can provide a list of routes relative to the file the worker
   *   is created in to limit the access of the worker (read/write permissions only)
   *
   * Example:
   *
   * ```ts
   * // mod.ts
   * const worker = new Worker(
   *   new URL("deno_worker.ts", import.meta.url).href, {
   *     type: "module",
   *     deno: {
   *       permissions: {
   *         read: true,
   *       },
   *     },
   *   }
   * );
   * ```
   */
  deno?: {
    /** Set to `"none"` to disable all the permissions in the worker. */
    permissions?: Deno.PermissionOptions;
  };
}

declare interface WebSocketStreamOptions {
  protocols?: string[];
  signal?: AbortSignal;
  headers?: HeadersInit;
}

declare interface WebSocketConnection {
  readable: ReadableStream<string | Uint8Array>;
  writable: WritableStream<string | Uint8Array>;
  extensions: string;
  protocol: string;
}

declare interface WebSocketCloseInfo {
  code?: number;
  reason?: string;
}

declare class WebSocketStream {
  constructor(url: string, options?: WebSocketStreamOptions);
  url: string;
  connection: Promise<WebSocketConnection>;
  closed: Promise<WebSocketCloseInfo>;
  close(closeInfo?: WebSocketCloseInfo): void;
}
