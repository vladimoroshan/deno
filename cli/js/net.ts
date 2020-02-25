// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import { EOF, Reader, Writer, Closer } from "./io.ts";
import { read, write, close } from "./files.ts";
import { sendSync, sendAsync } from "./dispatch_json.ts";

export type Transport = "tcp" | "udp";
// TODO support other types:
// export type Transport = "tcp" | "tcp4" | "tcp6" | "unix" | "unixpacket";

export interface Addr {
  transport: Transport;
  hostname: string;
  port: number;
}

export interface UDPAddr {
  transport?: Transport;
  hostname?: string;
  port: number;
}

/** A socket is a generic transport listener for message-oriented protocols */
export interface UDPConn extends AsyncIterator<[Uint8Array, Addr]> {
  /** Waits for and resolves to the next message to the `Socket`. */
  receive(p?: Uint8Array): Promise<[Uint8Array, Addr]>;

  /** Sends a message to the target. */
  send(p: Uint8Array, addr: UDPAddr): Promise<void>;

  /** Close closes the socket. Any pending message promises will be rejected
   * with errors.
   */
  close(): void;

  /** Return the address of the `Socket`. */
  addr: Addr;

  [Symbol.asyncIterator](): AsyncIterator<[Uint8Array, Addr]>;
}

/** A Listener is a generic transport listener for stream-oriented protocols. */
export interface Listener extends AsyncIterator<Conn> {
  /** Waits for and resolves to the next connection to the `Listener`. */
  accept(): Promise<Conn>;

  /** Close closes the listener. Any pending accept promises will be rejected
   * with errors.
   */
  close(): void;

  /** Return the address of the `Listener`. */
  addr: Addr;

  [Symbol.asyncIterator](): AsyncIterator<Conn>;
}

export enum ShutdownMode {
  // See http://man7.org/linux/man-pages/man2/shutdown.2.html
  // Corresponding to SHUT_RD, SHUT_WR, SHUT_RDWR
  Read = 0,
  Write,
  ReadWrite // unused
}

/** Shut down socket send and receive operations.
 *
 * Matches behavior of POSIX shutdown(3).
 *
 *       const listener = Deno.listen({ port: 80 });
 *       const conn = await listener.accept();
 *       Deno.shutdown(conn.rid, Deno.ShutdownMode.Write);
 */
export function shutdown(rid: number, how: ShutdownMode): void {
  sendSync("op_shutdown", { rid, how });
}

export class ConnImpl implements Conn {
  constructor(
    readonly rid: number,
    readonly remoteAddr: Addr,
    readonly localAddr: Addr
  ) {}

  write(p: Uint8Array): Promise<number> {
    return write(this.rid, p);
  }

  read(p: Uint8Array): Promise<number | EOF> {
    return read(this.rid, p);
  }

  close(): void {
    close(this.rid);
  }

  /** closeRead shuts down (shutdown(2)) the reading side of the TCP connection.
   * Most callers should just use close().
   */
  closeRead(): void {
    shutdown(this.rid, ShutdownMode.Read);
  }

  /** closeWrite shuts down (shutdown(2)) the writing side of the TCP
   * connection. Most callers should just use close().
   */
  closeWrite(): void {
    shutdown(this.rid, ShutdownMode.Write);
  }
}

export class ListenerImpl implements Listener {
  constructor(
    readonly rid: number,
    readonly addr: Addr,
    private closing: boolean = false
  ) {}

  async accept(): Promise<Conn> {
    const res = await sendAsync("op_accept", { rid: this.rid });
    return new ConnImpl(res.rid, res.remoteAddr, res.localAddr);
  }

  close(): void {
    this.closing = true;
    close(this.rid);
  }

  async next(): Promise<IteratorResult<Conn>> {
    if (this.closing) {
      return { value: undefined, done: true };
    }
    return await this.accept()
      .then(value => ({ value, done: false }))
      .catch(e => {
        // It wouldn't be correct to simply check this.closing here.
        // TODO: Get a proper error kind for this case, don't check the message.
        // The current error kind is Other.
        if (e.message == "Listener has been closed") {
          return { value: undefined, done: true };
        }
        throw e;
      });
  }

  [Symbol.asyncIterator](): AsyncIterator<Conn> {
    return this;
  }
}

export async function recvfrom(
  rid: number,
  p: Uint8Array
): Promise<[number, Addr]> {
  const { size, remoteAddr } = await sendAsync("op_receive", { rid }, p);
  return [size, remoteAddr];
}

export class UDPConnImpl implements UDPConn {
  constructor(
    readonly rid: number,
    readonly addr: Addr,
    public bufSize: number = 1024,
    private closing: boolean = false
  ) {}

  async receive(p?: Uint8Array): Promise<[Uint8Array, Addr]> {
    const buf = p || new Uint8Array(this.bufSize);
    const [size, remoteAddr] = await recvfrom(this.rid, buf);
    const sub = buf.subarray(0, size);
    return [sub, remoteAddr];
  }

  async send(p: Uint8Array, addr: UDPAddr): Promise<void> {
    const remote = { hostname: "127.0.0.1", transport: "udp", ...addr };
    if (remote.transport !== "udp") throw Error("Remote transport must be UDP");
    const args = { ...remote, rid: this.rid };
    await sendAsync("op_send", args, p);
  }

  close(): void {
    this.closing = true;
    close(this.rid);
  }

  async next(): Promise<IteratorResult<[Uint8Array, Addr]>> {
    if (this.closing) {
      return { value: undefined, done: true };
    }
    return await this.receive()
      .then(value => ({ value, done: false }))
      .catch(e => {
        // It wouldn't be correct to simply check this.closing here.
        // TODO: Get a proper error kind for this case, don't check the message.
        // The current error kind is Other.
        if (e.message == "Socket has been closed") {
          return { value: undefined, done: true };
        }
        throw e;
      });
  }

  [Symbol.asyncIterator](): AsyncIterator<[Uint8Array, Addr]> {
    return this;
  }
}

export interface Conn extends Reader, Writer, Closer {
  /** The local address of the connection. */
  localAddr: Addr;
  /** The remote address of the connection. */
  remoteAddr: Addr;
  /** The resource ID of the connection. */
  rid: number;
  /** Shuts down (`shutdown(2)`) the reading side of the TCP connection. Most
   * callers should just use `close()`.
   */
  closeRead(): void;
  /** Shuts down (`shutdown(2)`) the writing side of the TCP connection. Most
   * callers should just use `close()`.
   */
  closeWrite(): void;
}

export interface ListenOptions {
  port: number;
  hostname?: string;
  transport?: Transport;
}

const listenDefaults = { hostname: "0.0.0.0", transport: "tcp" };

/** Listen announces on the local transport address.
 *
 * @param options
 * @param options.port The port to connect to. (Required.)
 * @param options.hostname A literal IP address or host name that can be
 *   resolved to an IP address. If not specified, defaults to 0.0.0.0
 * @param options.transport Must be "tcp" or "udp". Defaults to "tcp". Later we plan to add "tcp4",
 *   "tcp6", "udp4", "udp6", "ip", "ip4", "ip6", "unix", "unixgram" and
 *   "unixpacket".
 *
 * Examples:
 *
 *     listen({ port: 80 })
 *     listen({ hostname: "192.0.2.1", port: 80 })
 *     listen({ hostname: "[2001:db8::1]", port: 80 });
 *     listen({ hostname: "golang.org", port: 80, transport: "tcp" })
 */
export function listen(
  options: ListenOptions & { transport?: "tcp" }
): Listener;
export function listen(options: ListenOptions & { transport: "udp" }): UDPConn;
export function listen(options: ListenOptions): Listener | UDPConn {
  const args = { ...listenDefaults, ...options };
  const res = sendSync("op_listen", args);

  if (args.transport === "tcp") {
    return new ListenerImpl(res.rid, res.localAddr);
  } else {
    return new UDPConnImpl(res.rid, res.localAddr);
  }
}

export interface ConnectOptions {
  port: number;
  hostname?: string;
  transport?: Transport;
}

const connectDefaults = { hostname: "127.0.0.1", transport: "tcp" };

/** Connects to the address on the named transport.
 *
 * @param options
 * @param options.port The port to connect to. (Required.)
 * @param options.hostname A literal IP address or host name that can be
 *   resolved to an IP address. If not specified, defaults to 127.0.0.1
 * @param options.transport Must be "tcp" or "udp". Defaults to "tcp". Later we plan to add "tcp4",
 *   "tcp6", "udp4", "udp6", "ip", "ip4", "ip6", "unix", "unixgram" and
 *   "unixpacket".
 *
 * Examples:
 *
 *     connect({ port: 80 })
 *     connect({ hostname: "192.0.2.1", port: 80 })
 *     connect({ hostname: "[2001:db8::1]", port: 80 });
 *     connect({ hostname: "golang.org", port: 80, transport: "tcp" })
 */
export async function connect(options: ConnectOptions): Promise<Conn> {
  options = Object.assign(connectDefaults, options);
  const res = await sendAsync("op_connect", options);
  return new ConnImpl(res.rid, res.remoteAddr!, res.localAddr!);
}
