// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.
// Copyright Joyent, Inc. and Node.js contributors. All rights reserved. MIT license.

import {
  TextDecoder,
  TextEncoder,
} from "internal:deno_web/08_text_encoding.js";
import { Buffer } from "internal:deno_node/polyfills/buffer.ts";
import { Transform } from "internal:deno_node/polyfills/stream.ts";
import { encode as encodeToHex } from "internal:deno_node/polyfills/internal/crypto/_hex.ts";
import {
  forgivingBase64Encode as encodeToBase64,
  forgivingBase64UrlEncode as encodeToBase64Url,
} from "internal:deno_web/00_infra.js";
import type { TransformOptions } from "internal:deno_node/polyfills/_stream.d.ts";
import { validateString } from "internal:deno_node/polyfills/internal/validators.mjs";
import type {
  BinaryToTextEncoding,
  Encoding,
} from "internal:deno_node/polyfills/internal/crypto/types.ts";
import {
  KeyObject,
  prepareSecretKey,
} from "internal:deno_node/polyfills/internal/crypto/keys.ts";
import { notImplemented } from "internal:deno_node/polyfills/_utils.ts";

const { ops } = globalThis.__bootstrap.core;

const coerceToBytes = (data: string | BufferSource): Uint8Array => {
  if (data instanceof Uint8Array) {
    return data;
  } else if (typeof data === "string") {
    // This assumes UTF-8, which may not be correct.
    return new TextEncoder().encode(data);
  } else if (ArrayBuffer.isView(data)) {
    return new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
  } else if (data instanceof ArrayBuffer) {
    return new Uint8Array(data);
  } else {
    throw new TypeError("expected data to be string | BufferSource");
  }
};

/**
 * The Hash class is a utility for creating hash digests of data. It can be used in one of two ways:
 *
 * - As a stream that is both readable and writable, where data is written to produce a computed hash digest on the readable side, or
 * - Using the hash.update() and hash.digest() methods to produce the computed hash.
 *
 * The crypto.createHash() method is used to create Hash instances. Hash objects are not to be created directly using the new keyword.
 */
export class Hash extends Transform {
  #context: number;

  constructor(
    algorithm: string | number,
    _opts?: TransformOptions,
  ) {
    super({
      transform(chunk: string, _encoding: string, callback: () => void) {
        ops.op_node_hash_update(context, coerceToBytes(chunk));
        callback();
      },
      flush(callback: () => void) {
        this.push(context.digest(undefined));
        callback();
      },
    });

    if (typeof algorithm === "string") {
      this.#context = ops.op_node_create_hash(
        algorithm,
      );
    } else {
      this.#context = algorithm;
    }

    const context = this.#context;
  }

  copy(): Hash {
    return new Hash(ops.op_node_clone_hash(this.#context));
  }

  /**
   * Updates the hash content with the given data.
   */
  update(data: string | ArrayBuffer, _encoding?: string): this {
    let bytes;
    if (typeof data === "string") {
      data = new TextEncoder().encode(data);
      bytes = coerceToBytes(data);
    } else {
      bytes = coerceToBytes(data);
    }

    ops.op_node_hash_update(this.#context, bytes);

    return this;
  }

  /**
   * Calculates the digest of all of the data.
   *
   * If encoding is provided a string will be returned; otherwise a Buffer is returned.
   *
   * Supported encodings are currently 'hex', 'binary', 'base64', 'base64url'.
   */
  digest(encoding?: string): Buffer | string {
    const digest = this.#context.digest(undefined);
    if (encoding === undefined) {
      return Buffer.from(digest);
    }

    switch (encoding) {
      case "hex":
        return new TextDecoder().decode(encodeToHex(new Uint8Array(digest)));
      case "binary":
        return String.fromCharCode(...digest);
      case "base64":
        return encodeToBase64(digest);
      case "base64url":
        return encodeToBase64Url(digest);
      case "buffer":
        return Buffer.from(digest);
      default:
        return Buffer.from(digest).toString(encoding);
    }
  }
}

export function Hmac(
  hmac: string,
  key: string | ArrayBuffer | KeyObject,
  options?: TransformOptions,
): Hmac {
  return new HmacImpl(hmac, key, options);
}

type Hmac = HmacImpl;

class HmacImpl extends Transform {
  #ipad: Uint8Array;
  #opad: Uint8Array;
  #ZEROES = Buffer.alloc(128);
  #algorithm: string;
  #hash: Hash;

  constructor(
    hmac: string,
    key: string | ArrayBuffer | KeyObject,
    options?: TransformOptions,
  ) {
    super({
      transform(chunk: string, encoding: string, callback: () => void) {
        // deno-lint-ignore no-explicit-any
        self.update(coerceToBytes(chunk), encoding as any);
        callback();
      },
      flush(callback: () => void) {
        this.push(self.digest());
        callback();
      },
    });
    // deno-lint-ignore no-this-alias
    const self = this;
    if (key instanceof KeyObject) {
      notImplemented("Hmac: KeyObject key is not implemented");
    }

    validateString(hmac, "hmac");
    const u8Key = prepareSecretKey(key, options?.encoding) as Buffer;

    const alg = hmac.toLowerCase();
    this.#hash = new Hash(alg, options);
    this.#algorithm = alg;
    const blockSize = (alg === "sha512" || alg === "sha384") ? 128 : 64;
    const keySize = u8Key.length;

    let bufKey: Buffer;

    if (keySize > blockSize) {
      bufKey = this.#hash.update(u8Key).digest() as Buffer;
    } else {
      bufKey = Buffer.concat([u8Key, this.#ZEROES], blockSize);
    }

    this.#ipad = Buffer.allocUnsafe(blockSize);
    this.#opad = Buffer.allocUnsafe(blockSize);

    for (let i = 0; i < blockSize; i++) {
      this.#ipad[i] = bufKey[i] ^ 0x36;
      this.#opad[i] = bufKey[i] ^ 0x5C;
    }

    this.#hash = new Hash(alg);
    this.#hash.update(this.#ipad);
  }

  digest(): Buffer;
  digest(encoding: BinaryToTextEncoding): string;
  digest(encoding?: BinaryToTextEncoding): Buffer | string {
    const result = this.#hash.digest();

    return new Hash(this.#algorithm).update(this.#opad).update(result).digest(
      encoding,
    );
  }

  update(data: string | ArrayBuffer, inputEncoding?: Encoding): this {
    this.#hash.update(data, inputEncoding);
    return this;
  }
}

Hmac.prototype = HmacImpl.prototype;

/**
 * Creates and returns a Hash object that can be used to generate hash digests
 * using the given `algorithm`. Optional `options` argument controls stream behavior.
 */
export function createHash(algorithm: string, opts?: TransformOptions) {
  return new Hash(algorithm, opts);
}

export default {
  Hash,
  Hmac,
  createHash,
};
