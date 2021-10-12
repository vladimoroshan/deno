// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

// @ts-check
/// <reference path="../../core/internal.d.ts" />
/// <reference path="../../core/lib.deno_core.d.ts" />
/// <reference path="../webidl/internal.d.ts" />
/// <reference path="../web/lib.deno_web.d.ts" />

"use strict";

((window) => {
  const core = window.Deno.core;
  const webidl = window.__bootstrap.webidl;
  const { DOMException } = window.__bootstrap.domException;
  const { atob, btoa } = window.__bootstrap.base64;

  const {
    ArrayPrototypeFind,
    ArrayPrototypeEvery,
    ArrayPrototypeIncludes,
    ArrayBuffer,
    ArrayBufferIsView,
    BigInt64Array,
    StringPrototypeToUpperCase,
    StringPrototypeReplace,
    StringPrototypeCharCodeAt,
    StringFromCharCode,
    Symbol,
    SymbolFor,
    SyntaxError,
    WeakMap,
    WeakMapPrototypeGet,
    WeakMapPrototypeSet,
    Int8Array,
    Uint8Array,
    TypedArrayPrototypeSlice,
    Int16Array,
    Uint16Array,
    Int32Array,
    Uint32Array,
    Uint8ClampedArray,
    TypeError,
  } = window.__bootstrap.primordials;

  // P-521 is not yet supported.
  const supportedNamedCurves = ["P-256", "P-384"];
  const recognisedUsages = [
    "encrypt",
    "decrypt",
    "sign",
    "verify",
    "deriveKey",
    "deriveBits",
    "wrapKey",
    "unwrapKey",
  ];

  const simpleAlgorithmDictionaries = {
    RsaHashedKeyGenParams: { hash: "HashAlgorithmIdentifier" },
    EcKeyGenParams: {},
    HmacKeyGenParams: { hash: "HashAlgorithmIdentifier" },
    RsaPssParams: {},
    EcdsaParams: { hash: "HashAlgorithmIdentifier" },
    HmacImportParams: { hash: "HashAlgorithmIdentifier" },
    HkdfParams: {
      hash: "HashAlgorithmIdentifier",
      salt: "BufferSource",
      info: "BufferSource",
    },
    Pbkdf2Params: { hash: "HashAlgorithmIdentifier", salt: "BufferSource" },
    RsaOaepParams: { label: "BufferSource" },
    RsaHashedImportParams: { hash: "HashAlgorithmIdentifier" },
  };

  const supportedAlgorithms = {
    "digest": {
      "SHA-1": null,
      "SHA-256": null,
      "SHA-384": null,
      "SHA-512": null,
    },
    "generateKey": {
      "RSASSA-PKCS1-v1_5": "RsaHashedKeyGenParams",
      "RSA-PSS": "RsaHashedKeyGenParams",
      "RSA-OAEP": "RsaHashedKeyGenParams",
      "ECDSA": "EcKeyGenParams",
      "ECDH": "EcKeyGenParams",
      "AES-CTR": "AesKeyGenParams",
      "AES-CBC": "AesKeyGenParams",
      "AES-GCM": "AesKeyGenParams",
      "AES-KW": "AesKeyGenParams",
      "HMAC": "HmacKeyGenParams",
    },
    "sign": {
      "RSASSA-PKCS1-v1_5": null,
      "RSA-PSS": "RsaPssParams",
      "ECDSA": "EcdsaParams",
      "HMAC": null,
    },
    "verify": {
      "RSASSA-PKCS1-v1_5": null,
      "RSA-PSS": "RsaPssParams",
      "ECDSA": "EcdsaParams",
      "HMAC": null,
    },
    "importKey": {
      "RSASSA-PKCS1-v1_5": "RsaHashedImportParams",
      "RSA-PSS": "RsaHashedImportParams",
      "RSA-OAEP": "RsaHashedImportParams",
      "HMAC": "HmacImportParams",
      "HKDF": null,
      "PBKDF2": null,
    },
    "deriveBits": {
      "HKDF": "HkdfParams",
      "PBKDF2": "Pbkdf2Params",
      "ECDH": "EcdhKeyDeriveParams",
    },
    "encrypt": {
      "RSA-OAEP": "RsaOaepParams",
      "AES-CBC": "AesCbcParams",
    },
    "decrypt": {
      "RSA-OAEP": "RsaOaepParams",
      "AES-CBC": "AesCbcParams",
    },
    "get key length": {
      "AES-CBC": "AesDerivedKeyParams",
      "AES-GCM": "AesDerivedKeyParams",
      "AES-KW": "AesDerivedKeyParams",
      "HMAC": "HmacImportParams",
      "HKDF": null,
      "PBKDF2": null,
    },
    "wrapKey": {
      // TODO(@littledivy): Enable this once implemented.
      // "AES-KW": "AesKeyWrapParams",
    },
  };

  // Decodes the unpadded base64 to the octet sequence containing key value `k` defined in RFC7518 Section 6.4
  function decodeSymmetricKey(key) {
    // Decode from base64url without `=` padding.
    const base64 = StringPrototypeReplace(
      StringPrototypeReplace(key, /\-/g, "+"),
      /\_/g,
      "/",
    );
    const decodedKey = atob(base64);
    const keyLength = decodedKey.length;
    const keyBytes = new Uint8Array(keyLength);
    for (let i = 0; i < keyLength; i++) {
      keyBytes[i] = StringPrototypeCharCodeAt(decodedKey, i);
    }
    return keyBytes;
  }

  function unpaddedBase64(bytes) {
    let binaryString = "";
    for (let i = 0; i < bytes.length; i++) {
      binaryString += StringFromCharCode(bytes[i]);
    }
    const base64String = btoa(binaryString);
    return StringPrototypeReplace(base64String, /=/g, "");
  }

  // See https://www.w3.org/TR/WebCryptoAPI/#dfn-normalize-an-algorithm
  // 18.4.4
  function normalizeAlgorithm(algorithm, op) {
    if (typeof algorithm == "string") {
      return normalizeAlgorithm({ name: algorithm }, op);
    }

    // 1.
    const registeredAlgorithms = supportedAlgorithms[op];
    // 2. 3.
    const initialAlg = webidl.converters.Algorithm(algorithm, {
      prefix: "Failed to normalize algorithm",
      context: "passed algorithm",
    });
    // 4.
    let algName = initialAlg.name;

    // 5.
    let desiredType = undefined;
    for (const key in registeredAlgorithms) {
      if (
        StringPrototypeToUpperCase(key) === StringPrototypeToUpperCase(algName)
      ) {
        algName = key;
        desiredType = registeredAlgorithms[key];
      }
    }
    if (desiredType === undefined) {
      throw new DOMException(
        "Unrecognized algorithm name",
        "NotSupportedError",
      );
    }

    // Fast path everything below if the registered dictionary is "None".
    if (desiredType === null) {
      return { name: algName };
    }

    // 6.
    const normalizedAlgorithm = webidl.converters[desiredType](algorithm, {
      prefix: "Failed to normalize algorithm",
      context: "passed algorithm",
    });
    // 7.
    normalizedAlgorithm.name = algName;

    // 9.
    const dict = simpleAlgorithmDictionaries[desiredType];
    // 10.
    for (const member in dict) {
      const idlType = dict[member];
      const idlValue = normalizedAlgorithm[member];
      // 3.
      if (idlType === "BufferSource" && idlValue) {
        normalizedAlgorithm[member] = TypedArrayPrototypeSlice(
          new Uint8Array(
            (ArrayBufferIsView(idlValue) ? idlValue.buffer : idlValue),
            idlValue.byteOffset ?? 0,
            idlValue.byteLength,
          ),
        );
      } else if (idlType === "HashAlgorithmIdentifier") {
        normalizedAlgorithm[member] = normalizeAlgorithm(idlValue, "digest");
      } else if (idlType === "AlgorithmIdentifier") {
        // TODO(lucacasonato): implement
        throw new TypeError("unimplemented");
      }
    }

    return normalizedAlgorithm;
  }

  const _handle = Symbol("[[handle]]");
  const _algorithm = Symbol("[[algorithm]]");
  const _extractable = Symbol("[[extractable]]");
  const _usages = Symbol("[[usages]]");
  const _type = Symbol("[[type]]");

  class CryptoKey {
    /** @type {string} */
    [_type];
    /** @type {boolean} */
    [_extractable];
    /** @type {object} */
    [_algorithm];
    /** @type {string[]} */
    [_usages];
    /** @type {object} */
    [_handle];

    constructor() {
      webidl.illegalConstructor();
    }

    /** @returns {string} */
    get type() {
      webidl.assertBranded(this, CryptoKey);
      return this[_type];
    }

    /** @returns {boolean} */
    get extractable() {
      webidl.assertBranded(this, CryptoKey);
      return this[_extractable];
    }

    /** @returns {string[]} */
    get usages() {
      webidl.assertBranded(this, CryptoKey);
      // TODO(lucacasonato): return a SameObject copy
      return this[_usages];
    }

    /** @returns {object} */
    get algorithm() {
      webidl.assertBranded(this, CryptoKey);
      // TODO(lucacasonato): return a SameObject copy
      return this[_algorithm];
    }

    [SymbolFor("Deno.customInspect")](inspect) {
      return `${this.constructor.name} ${
        inspect({
          type: this.type,
          extractable: this.extractable,
          algorithm: this.algorithm,
          usages: this.usages,
        })
      }`;
    }
  }

  webidl.configurePrototype(CryptoKey);

  /**
   * @param {string} type
   * @param {boolean} extractable
   * @param {string[]} usages
   * @param {object} algorithm
   * @param {object} handle
   * @returns
   */
  function constructKey(type, extractable, usages, algorithm, handle) {
    const key = webidl.createBranded(CryptoKey);
    key[_type] = type;
    key[_extractable] = extractable;
    key[_usages] = usages;
    key[_algorithm] = algorithm;
    key[_handle] = handle;
    return key;
  }

  // https://w3c.github.io/webcrypto/#concept-usage-intersection
  /**
   * @param {string[]} a
   * @param {string[]} b
   * @returns
   */
  function usageIntersection(a, b) {
    return a.filter((i) => b.includes(i));
  }

  // TODO(lucacasonato): this should be moved to rust
  /** @type {WeakMap<object, object>} */
  const KEY_STORE = new WeakMap();

  function getKeyLength(algorithm) {
    switch (algorithm.name) {
      case "AES-CBC":
      case "AES-GCM":
      case "AES-KW": {
        // 1.
        if (!ArrayPrototypeIncludes([128, 192, 256], algorithm.length)) {
          throw new DOMException(
            "length must be 128, 192, or 256",
            "OperationError",
          );
        }

        // 2.
        return algorithm.length;
      }
      case "HMAC": {
        // 1.
        let length;
        if (algorithm.length === undefined) {
          switch (algorithm.hash.name) {
            case "SHA-1":
              length = 160;
              break;
            case "SHA-256":
              length = 256;
              break;
            case "SHA-384":
              length = 384;
              break;
            case "SHA-512":
              length = 512;
              break;
            default:
              throw new DOMException(
                "Unrecognized hash algorithm",
                "NotSupportedError",
              );
          }
        } else if (algorithm.length !== 0) {
          length = algorithm.length;
        } else {
          throw new TypeError("Invalid length.");
        }

        // 2.
        return length;
      }
      case "HKDF": {
        // 1.
        return null;
      }
      case "PBKDF2": {
        // 1.
        return null;
      }
      default:
        throw new TypeError("unreachable");
    }
  }

  class SubtleCrypto {
    constructor() {
      webidl.illegalConstructor();
    }

    /**
     * @param {string} algorithm
     * @param {BufferSource} data
     * @returns {Promise<Uint8Array>}
     */
    async digest(algorithm, data) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'digest' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 2, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      data = webidl.converters.BufferSource(data, {
        prefix,
        context: "Argument 2",
      });

      if (ArrayBufferIsView(data)) {
        data = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      } else {
        data = new Uint8Array(data);
      }

      data = TypedArrayPrototypeSlice(data);

      algorithm = normalizeAlgorithm(algorithm, "digest");

      const result = await core.opAsync(
        "op_crypto_subtle_digest",
        algorithm.name,
        data,
      );

      return result.buffer;
    }

    /**
     * @param {string} algorithm
     * @param {CryptoKey} key
     * @param {BufferSource} data
     * @returns {Promise<any>}
     */
    async encrypt(algorithm, key, data) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'encrypt' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 3, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      key = webidl.converters.CryptoKey(key, {
        prefix,
        context: "Argument 2",
      });
      data = webidl.converters.BufferSource(data, {
        prefix,
        context: "Argument 3",
      });

      // 2.
      if (ArrayBufferIsView(data)) {
        data = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      } else {
        data = new Uint8Array(data);
      }
      data = TypedArrayPrototypeSlice(data);

      // 3.
      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "encrypt");

      const handle = key[_handle];
      const keyData = WeakMapPrototypeGet(KEY_STORE, handle);

      switch (normalizedAlgorithm.name) {
        case "RSA-OAEP": {
          // 1.
          if (key[_type] !== "public") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          // 2.
          if (normalizedAlgorithm.label) {
            if (ArrayBufferIsView(normalizedAlgorithm.label)) {
              normalizedAlgorithm.label = new Uint8Array(
                normalizedAlgorithm.label.buffer,
                normalizedAlgorithm.label.byteOffset,
                normalizedAlgorithm.label.byteLength,
              );
            } else {
              normalizedAlgorithm.label = new Uint8Array(
                normalizedAlgorithm.label,
              );
            }
            normalizedAlgorithm.label = TypedArrayPrototypeSlice(
              normalizedAlgorithm.label,
            );
          } else {
            normalizedAlgorithm.label = new Uint8Array();
          }

          // 3-5.
          const hashAlgorithm = key[_algorithm].hash.name;
          const cipherText = await core.opAsync("op_crypto_encrypt_key", {
            key: keyData,
            algorithm: "RSA-OAEP",
            hash: hashAlgorithm,
          }, data);

          // 6.
          return cipherText.buffer;
        }
        case "AES-CBC": {
          if (ArrayBufferIsView(normalizedAlgorithm.iv)) {
            normalizedAlgorithm.iv = new Uint8Array(
              normalizedAlgorithm.iv.buffer,
              normalizedAlgorithm.iv.byteOffset,
              normalizedAlgorithm.iv.byteLength,
            );
          } else {
            normalizedAlgorithm.iv = new Uint8Array(
              normalizedAlgorithm.iv,
            );
          }
          normalizedAlgorithm.iv = TypedArrayPrototypeSlice(
            normalizedAlgorithm.iv,
          );

          // 1.
          if (normalizedAlgorithm.iv.byteLength !== 16) {
            throw new DOMException(
              "Initialization vector must be 16 bytes",
              "OperationError",
            );
          }

          // 2.
          const cipherText = await core.opAsync("op_crypto_encrypt_key", {
            key: keyData,
            algorithm: "AES-CBC",
            length: key[_algorithm].length,
            iv: normalizedAlgorithm.iv,
          }, data);

          // 4.
          return cipherText.buffer;
        }
        default:
          throw new DOMException("Not implemented", "NotSupportedError");
      }
    }

    /**
     * @param {string} algorithm
     * @param {CryptoKey} key
     * @param {BufferSource} data
     * @returns {Promise<any>}
     */
    async decrypt(algorithm, key, data) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'decrypt' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 3, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      key = webidl.converters.CryptoKey(key, {
        prefix,
        context: "Argument 2",
      });
      data = webidl.converters.BufferSource(data, {
        prefix,
        context: "Argument 3",
      });

      // 2.
      if (ArrayBufferIsView(data)) {
        data = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      } else {
        data = new Uint8Array(data);
      }
      data = TypedArrayPrototypeSlice(data);

      // 3.
      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "decrypt");

      const handle = key[_handle];
      const keyData = WeakMapPrototypeGet(KEY_STORE, handle);

      switch (normalizedAlgorithm.name) {
        case "RSA-OAEP": {
          // 1.
          if (key[_type] !== "private") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          // 2.
          if (normalizedAlgorithm.label) {
            if (ArrayBufferIsView(normalizedAlgorithm.label)) {
              normalizedAlgorithm.label = new Uint8Array(
                normalizedAlgorithm.label.buffer,
                normalizedAlgorithm.label.byteOffset,
                normalizedAlgorithm.label.byteLength,
              );
            } else {
              normalizedAlgorithm.label = new Uint8Array(
                normalizedAlgorithm.label,
              );
            }
            normalizedAlgorithm.label = TypedArrayPrototypeSlice(
              normalizedAlgorithm.label,
            );
          } else {
            normalizedAlgorithm.label = new Uint8Array();
          }

          // 3-5.
          const hashAlgorithm = key[_algorithm].hash.name;
          const plainText = await core.opAsync("op_crypto_decrypt_key", {
            key: keyData,
            algorithm: "RSA-OAEP",
            hash: hashAlgorithm,
            label: normalizedAlgorithm.label,
          }, data);

          // 6.
          return plainText.buffer;
        }
        case "AES-CBC": {
          if (ArrayBufferIsView(normalizedAlgorithm.iv)) {
            normalizedAlgorithm.iv = new Uint8Array(
              normalizedAlgorithm.iv.buffer,
              normalizedAlgorithm.iv.byteOffset,
              normalizedAlgorithm.iv.byteLength,
            );
          } else {
            normalizedAlgorithm.iv = new Uint8Array(
              normalizedAlgorithm.iv,
            );
          }
          normalizedAlgorithm.iv = TypedArrayPrototypeSlice(
            normalizedAlgorithm.iv,
          );

          // 1.
          if (normalizedAlgorithm.iv.byteLength !== 16) {
            throw new DOMException(
              "Counter must be 16 bytes",
              "OperationError",
            );
          }

          const plainText = await core.opAsync("op_crypto_decrypt_key", {
            key: keyData,
            algorithm: "AES-CBC",
            iv: normalizedAlgorithm.iv,
            length: key[_algorithm].length,
          }, data);

          // 6.
          return plainText.buffer;
        }
        default:
          throw new DOMException("Not implemented", "NotSupportedError");
      }
    }

    /**
     * @param {string} algorithm
     * @param {CryptoKey} key
     * @param {BufferSource} data
     * @returns {Promise<any>}
     */
    async sign(algorithm, key, data) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'sign' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 3, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      key = webidl.converters.CryptoKey(key, {
        prefix,
        context: "Argument 2",
      });
      data = webidl.converters.BufferSource(data, {
        prefix,
        context: "Argument 3",
      });

      // 1.
      if (ArrayBufferIsView(data)) {
        data = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      } else {
        data = new Uint8Array(data);
      }
      data = TypedArrayPrototypeSlice(data);

      // 2.
      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "sign");

      const handle = key[_handle];
      const keyData = WeakMapPrototypeGet(KEY_STORE, handle);

      // 8.
      if (normalizedAlgorithm.name !== key[_algorithm].name) {
        throw new DOMException(
          "Signing algorithm doesn't match key algorithm.",
          "InvalidAccessError",
        );
      }

      // 9.
      if (!ArrayPrototypeIncludes(key[_usages], "sign")) {
        throw new DOMException(
          "Key does not support the 'sign' operation.",
          "InvalidAccessError",
        );
      }

      switch (normalizedAlgorithm.name) {
        case "RSASSA-PKCS1-v1_5": {
          // 1.
          if (key[_type] !== "private") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          // 2.
          const hashAlgorithm = key[_algorithm].hash.name;
          const signature = await core.opAsync("op_crypto_sign_key", {
            key: keyData,
            algorithm: "RSASSA-PKCS1-v1_5",
            hash: hashAlgorithm,
          }, data);

          return signature.buffer;
        }
        case "RSA-PSS": {
          // 1.
          if (key[_type] !== "private") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          // 2.
          const hashAlgorithm = key[_algorithm].hash.name;
          const signature = await core.opAsync("op_crypto_sign_key", {
            key: keyData,
            algorithm: "RSA-PSS",
            hash: hashAlgorithm,
            saltLength: normalizedAlgorithm.saltLength,
          }, data);

          return signature.buffer;
        }
        case "ECDSA": {
          // 1.
          if (key[_type] !== "private") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          // 2.
          const hashAlgorithm = normalizedAlgorithm.hash.name;
          const namedCurve = key[_algorithm].namedCurve;
          if (!ArrayPrototypeIncludes(supportedNamedCurves, namedCurve)) {
            throw new DOMException("Curve not supported", "NotSupportedError");
          }

          const signature = await core.opAsync("op_crypto_sign_key", {
            key: keyData,
            algorithm: "ECDSA",
            hash: hashAlgorithm,
            namedCurve,
          }, data);

          return signature.buffer;
        }
        case "HMAC": {
          const hashAlgorithm = key[_algorithm].hash.name;

          const signature = await core.opAsync("op_crypto_sign_key", {
            key: keyData,
            algorithm: "HMAC",
            hash: hashAlgorithm,
          }, data);

          return signature.buffer;
        }
      }

      throw new TypeError("unreachable");
    }

    /**
     * @param {string} format
     * @param {BufferSource} keyData
     * @param {string} algorithm
     * @param {boolean} extractable
     * @param {KeyUsages[]} keyUsages
     * @returns {Promise<any>}
     */
    async importKey(format, keyData, algorithm, extractable, keyUsages) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'importKey' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 4, { prefix });
      format = webidl.converters.KeyFormat(format, {
        prefix,
        context: "Argument 1",
      });
      keyData = webidl.converters["BufferSource or JsonWebKey"](keyData, {
        prefix,
        context: "Argument 2",
      });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 3",
      });
      extractable = webidl.converters.boolean(extractable, {
        prefix,
        context: "Argument 4",
      });
      keyUsages = webidl.converters["sequence<KeyUsage>"](keyUsages, {
        prefix,
        context: "Argument 5",
      });

      // 2.
      if (format !== "jwk") {
        if (ArrayBufferIsView(keyData) || keyData instanceof ArrayBuffer) {
          if (ArrayBufferIsView(keyData)) {
            keyData = new Uint8Array(
              keyData.buffer,
              keyData.byteOffset,
              keyData.byteLength,
            );
          } else {
            keyData = new Uint8Array(keyData);
          }
          keyData = TypedArrayPrototypeSlice(keyData);
        } else {
          throw new TypeError("keyData is a JsonWebKey");
        }
      } else {
        if (ArrayBufferIsView(keyData) || keyData instanceof ArrayBuffer) {
          throw new TypeError("keyData is not a JsonWebKey");
        }
      }

      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "importKey");

      switch (normalizedAlgorithm.name) {
        case "HMAC": {
          // 2.
          if (
            ArrayPrototypeFind(
              keyUsages,
              (u) => !ArrayPrototypeIncludes(["sign", "verify"], u),
            ) !== undefined
          ) {
            throw new DOMException("Invalid key usages", "SyntaxError");
          }

          // 3.
          let hash;
          let data;

          // 4. https://w3c.github.io/webcrypto/#hmac-operations
          switch (format) {
            case "raw": {
              data = keyData;
              hash = normalizedAlgorithm.hash;
              break;
            }
            case "jwk": {
              // TODO(@littledivy): Why does the spec validate JWK twice?
              const jwk = keyData;
              // 2.
              if (jwk.kty !== "oct") {
                throw new DOMException(
                  "`kty` member of JsonWebKey must be `oct`",
                  "DataError",
                );
              }

              // Section 6.4.1 of RFC7518
              if (!jwk.k) {
                throw new DOMException(
                  "`k` member of JsonWebKey must be present",
                  "DataError",
                );
              }

              // 4.
              data = decodeSymmetricKey(jwk.k);
              // 5.
              hash = normalizedAlgorithm.hash;
              // 6.
              switch (hash.name) {
                case "SHA-1": {
                  if (jwk.alg !== undefined && jwk.alg !== "HS1") {
                    throw new DOMException(
                      "`alg` member of JsonWebKey must be `HS1`",
                      "DataError",
                    );
                  }
                  break;
                }
                case "SHA-256": {
                  if (jwk.alg !== undefined && jwk.alg !== "HS256") {
                    throw new DOMException(
                      "`alg` member of JsonWebKey must be `HS256`",
                      "DataError",
                    );
                  }
                  break;
                }
                case "SHA-384": {
                  if (jwk.alg !== undefined && jwk.alg !== "HS384") {
                    throw new DOMException(
                      "`alg` member of JsonWebKey must be `HS384`",
                      "DataError",
                    );
                  }
                  break;
                }
                case "SHA-512": {
                  if (jwk.alg !== undefined && jwk.alg !== "HS512") {
                    throw new DOMException(
                      "`alg` member of JsonWebKey must be `HS512`",
                      "DataError",
                    );
                  }
                  break;
                }
                default:
                  throw new TypeError("unreachable");
              }

              // 7.
              if (keyUsages.length > 0 && jwk.use && jwk.use !== "sign") {
                throw new DOMException(
                  "`use` member of JsonWebKey must be `sign`",
                  "DataError",
                );
              }

              // 8.
              // Section 4.3 of RFC7517
              if (jwk.key_ops) {
                if (
                  ArrayPrototypeFind(
                    jwk.key_ops,
                    (u) => !ArrayPrototypeIncludes(recognisedUsages, u),
                  ) !== undefined
                ) {
                  throw new DOMException(
                    "`key_ops` member of JsonWebKey is invalid",
                    "DataError",
                  );
                }

                if (
                  !ArrayPrototypeEvery(
                    jwk.key_ops,
                    (u) => ArrayPrototypeIncludes(keyUsages, u),
                  )
                ) {
                  throw new DOMException(
                    "`key_ops` member of JsonWebKey is invalid",
                    "DataError",
                  );
                }
              }

              // 9.
              if (jwk.ext === false && extractable == true) {
                throw new DOMException(
                  "`ext` member of JsonWebKey is invalid",
                  "DataError",
                );
              }

              break;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }

          // 5.
          let length = data.byteLength * 8;
          // 6.
          if (length === 0) {
            throw new DOMException("Key length is zero", "DataError");
          }
          // 7.
          if (normalizedAlgorithm.length !== undefined) {
            if (
              normalizedAlgorithm.length > length ||
              normalizedAlgorithm.length <= (length - 8)
            ) {
              throw new DOMException(
                "Key length is invalid",
                "DataError",
              );
            }
            length = normalizedAlgorithm.length;
          }

          if (keyUsages.length == 0) {
            throw new DOMException("Key usage is empty", "SyntaxError");
          }

          const handle = {};
          WeakMapPrototypeSet(KEY_STORE, handle, {
            type: "raw",
            data,
          });

          const algorithm = {
            name: "HMAC",
            length,
            hash,
          };

          const key = constructKey(
            "secret",
            extractable,
            usageIntersection(keyUsages, recognisedUsages),
            algorithm,
            handle,
          );

          return key;
        }
        // TODO(@littledivy): RSA-PSS
        case "ECDSA": {
          switch (format) {
            case "raw": {
              // 1.
              if (
                !ArrayPrototypeIncludes(
                  supportedNamedCurves,
                  normalizedAlgorithm.namedCurve,
                )
              ) {
                throw new DOMException(
                  "Invalid namedCurve",
                  "DataError",
                );
              }

              // 2.
              if (
                ArrayPrototypeFind(
                  keyUsages,
                  (u) => !ArrayPrototypeIncludes(["verify"], u),
                ) !== undefined
              ) {
                throw new DOMException("Invalid key usages", "SyntaxError");
              }

              // 3.
              const { data } = await core.opAsync("op_crypto_import_key", {
                algorithm: "ECDSA",
                namedCurve: normalizedAlgorithm.namedCurve,
              }, keyData);

              const handle = {};
              WeakMapPrototypeSet(KEY_STORE, handle, {
                type: "raw",
                data,
              });

              // 4-5.
              const algorithm = {
                name: "ECDSA",
                namedCurve: normalizedAlgorithm.namedCurve,
              };

              // 6-8.
              const key = constructKey(
                "public",
                extractable,
                usageIntersection(keyUsages, recognisedUsages),
                algorithm,
                handle,
              );

              return key;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        case "RSASSA-PKCS1-v1_5": {
          switch (format) {
            case "pkcs8": {
              // 1.
              if (
                ArrayPrototypeFind(
                  keyUsages,
                  (u) => !ArrayPrototypeIncludes(["sign"], u),
                ) !== undefined
              ) {
                throw new DOMException("Invalid key usages", "SyntaxError");
              }

              if (keyUsages.length == 0) {
                throw new DOMException("Key usage is empty", "SyntaxError");
              }

              // 2-9.
              const { modulusLength, publicExponent, data } = await core
                .opAsync(
                  "op_crypto_import_key",
                  {
                    algorithm: "RSASSA-PKCS1-v1_5",
                    format: "pkcs8",
                    // Needed to perform step 7 without normalization.
                    hash: normalizedAlgorithm.hash.name,
                  },
                  keyData,
                );

              const handle = {};
              WeakMapPrototypeSet(KEY_STORE, handle, {
                // PKCS#1 for RSA
                type: "raw",
                data,
              });

              const algorithm = {
                name: "RSASSA-PKCS1-v1_5",
                modulusLength,
                publicExponent,
                hash: normalizedAlgorithm.hash,
              };

              const key = constructKey(
                "private",
                extractable,
                usageIntersection(keyUsages, recognisedUsages),
                algorithm,
                handle,
              );

              return key;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        case "RSA-PSS": {
          switch (format) {
            case "pkcs8": {
              // 1.
              if (
                ArrayPrototypeFind(
                  keyUsages,
                  (u) => !ArrayPrototypeIncludes(["sign"], u),
                ) !== undefined
              ) {
                throw new DOMException("Invalid key usages", "SyntaxError");
              }

              if (keyUsages.length == 0) {
                throw new DOMException("Key usage is empty", "SyntaxError");
              }

              // 2-9.
              const { modulusLength, publicExponent, data } = await core
                .opAsync(
                  "op_crypto_import_key",
                  {
                    algorithm: "RSA-PSS",
                    format: "pkcs8",
                    // Needed to perform step 7 without normalization.
                    hash: normalizedAlgorithm.hash.name,
                  },
                  keyData,
                );

              const handle = {};
              WeakMapPrototypeSet(KEY_STORE, handle, {
                // PKCS#1 for RSA
                type: "raw",
                data,
              });

              const algorithm = {
                name: "RSA-PSS",
                modulusLength,
                publicExponent,
                hash: normalizedAlgorithm.hash,
              };

              const key = constructKey(
                "private",
                extractable,
                usageIntersection(keyUsages, recognisedUsages),
                algorithm,
                handle,
              );

              return key;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        case "RSA-OAEP": {
          switch (format) {
            case "pkcs8": {
              // 1.
              if (
                ArrayPrototypeFind(
                  keyUsages,
                  (u) => !ArrayPrototypeIncludes(["decrypt", "unwrapKey"], u),
                ) !== undefined
              ) {
                throw new DOMException("Invalid key usages", "SyntaxError");
              }

              if (keyUsages.length == 0) {
                throw new DOMException("Key usage is empty", "SyntaxError");
              }

              // 2-9.
              const { modulusLength, publicExponent, data } = await core
                .opAsync(
                  "op_crypto_import_key",
                  {
                    algorithm: "RSA-OAEP",
                    format: "pkcs8",
                    // Needed to perform step 7 without normalization.
                    hash: normalizedAlgorithm.hash.name,
                  },
                  keyData,
                );

              const handle = {};
              WeakMapPrototypeSet(KEY_STORE, handle, {
                // PKCS#1 for RSA
                type: "raw",
                data,
              });

              const algorithm = {
                name: "RSA-OAEP",
                modulusLength,
                publicExponent,
                hash: normalizedAlgorithm.hash,
              };

              const key = constructKey(
                "private",
                extractable,
                usageIntersection(keyUsages, recognisedUsages),
                algorithm,
                handle,
              );

              return key;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        case "HKDF": {
          if (format !== "raw") {
            throw new DOMException("Format not supported", "NotSupportedError");
          }

          // 1.
          if (
            ArrayPrototypeFind(
              keyUsages,
              (u) => !ArrayPrototypeIncludes(["deriveKey", "deriveBits"], u),
            ) !== undefined
          ) {
            throw new DOMException("Invalid key usages", "SyntaxError");
          }

          // 2.
          if (extractable !== false) {
            throw new DOMException(
              "Key must not be extractable",
              "SyntaxError",
            );
          }

          // 3.
          const handle = {};
          WeakMapPrototypeSet(KEY_STORE, handle, {
            type: "raw",
            data: keyData,
          });

          // 4-8.
          const algorithm = {
            name: "HKDF",
          };
          const key = constructKey(
            "secret",
            false,
            usageIntersection(keyUsages, recognisedUsages),
            algorithm,
            handle,
          );

          // 9.
          return key;
        }
        case "PBKDF2": {
          // 1.
          if (format !== "raw") {
            throw new DOMException("Format not supported", "NotSupportedError");
          }

          // 2.
          if (
            ArrayPrototypeFind(
              keyUsages,
              (u) => !ArrayPrototypeIncludes(["deriveKey", "deriveBits"], u),
            ) !== undefined
          ) {
            throw new DOMException("Invalid key usages", "SyntaxError");
          }

          // 3.
          if (extractable !== false) {
            throw new DOMException(
              "Key must not be extractable",
              "SyntaxError",
            );
          }

          // 4.
          const handle = {};
          WeakMapPrototypeSet(KEY_STORE, handle, {
            type: "raw",
            data: keyData,
          });

          // 5-9.
          const algorithm = {
            name: "PBKDF2",
          };
          const key = constructKey(
            "secret",
            false,
            usageIntersection(keyUsages, recognisedUsages),
            algorithm,
            handle,
          );

          // 10.
          return key;
        }
        default:
          throw new DOMException("Not implemented", "NotSupportedError");
      }
    }

    /**
     * @param {string} format
     * @param {CryptoKey} key
     * @returns {Promise<any>}
     */
    async exportKey(format, key) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'exportKey' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 2, { prefix });
      format = webidl.converters.KeyFormat(format, {
        prefix,
        context: "Argument 1",
      });
      key = webidl.converters.CryptoKey(key, {
        prefix,
        context: "Argument 2",
      });

      const handle = key[_handle];
      // 2.
      const innerKey = WeakMapPrototypeGet(KEY_STORE, handle);

      switch (key[_algorithm].name) {
        case "HMAC": {
          if (innerKey == null) {
            throw new DOMException("Key is not available", "OperationError");
          }
          switch (format) {
            // 3.
            case "raw": {
              const bits = innerKey.data;
              for (let _i = 7 & (8 - bits.length % 8); _i > 0; _i--) {
                bits.push(0);
              }
              // 4-5.
              return bits.buffer;
            }
            case "jwk": {
              // 1-3.
              const jwk = {
                kty: "oct",
                k: unpaddedBase64(innerKey.data),
              };
              // 4.
              const algorithm = key[_algorithm];
              // 5.
              const hash = algorithm.hash;
              // 6.
              switch (hash.name) {
                case "SHA-1":
                  jwk.alg = "HS1";
                  break;
                case "SHA-256":
                  jwk.alg = "HS256";
                  break;
                case "SHA-384":
                  jwk.alg = "HS384";
                  break;
                case "SHA-512":
                  jwk.alg = "HS512";
                  break;
                default:
                  throw new DOMException(
                    "Hash algorithm not supported",
                    "NotSupportedError",
                  );
              }
              // 7.
              jwk.key_ops = key.usages;
              // 8.
              jwk.ext = key[_extractable];
              // 9.
              return jwk;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
          // TODO(@littledivy): Redundant break but deno_lint complains without it
          break;
        }
        case "RSASSA-PKCS1-v1_5": {
          switch (format) {
            case "pkcs8": {
              // 1.
              if (key[_type] !== "private") {
                throw new DOMException(
                  "Key is not a private key",
                  "InvalidAccessError",
                );
              }

              // 2.
              const data = await core.opAsync(
                "op_crypto_export_key",
                {
                  key: innerKey,
                  format: "pkcs8",
                  algorithm: "RSASSA-PKCS1-v1_5",
                },
              );

              // 3.
              return data.buffer;
            }
            case "spki": {
              // 1.
              if (key[_type] !== "public") {
                throw new DOMException(
                  "Key is not a public key",
                  "InvalidAccessError",
                );
              }

              // 2.
              const data = await core.opAsync(
                "op_crypto_export_key",
                {
                  key: innerKey,
                  format: "spki",
                  algorithm: "RSASSA-PKCS1-v1_5",
                },
              );

              // 3.
              return data.buffer;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        case "RSA-PSS": {
          switch (format) {
            case "pkcs8": {
              // 1.
              if (key[_type] !== "private") {
                throw new DOMException(
                  "Key is not a private key",
                  "InvalidAccessError",
                );
              }

              // 2.
              const data = await core.opAsync(
                "op_crypto_export_key",
                {
                  key: innerKey,
                  format: "pkcs8",
                  algorithm: "RSA-PSS",
                  hash: key[_algorithm].hash.name,
                },
              );

              // 3.
              return data.buffer;
            }
            case "spki": {
              // 1.
              if (key[_type] !== "public") {
                throw new DOMException(
                  "Key is not a public key",
                  "InvalidAccessError",
                );
              }

              // 2.
              const data = await core.opAsync(
                "op_crypto_export_key",
                {
                  key: innerKey,
                  format: "spki",
                  algorithm: "RSA-PSS",
                  hash: key[_algorithm].hash.name,
                },
              );

              // 3.
              return data.buffer;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        case "RSA-OAEP": {
          switch (format) {
            case "pkcs8": {
              // 1.
              if (key[_type] !== "private") {
                throw new DOMException(
                  "Key is not a private key",
                  "InvalidAccessError",
                );
              }

              // 2.
              const data = await core.opAsync(
                "op_crypto_export_key",
                {
                  key: innerKey,
                  format: "pkcs8",
                  algorithm: "RSA-PSS",
                  hash: key[_algorithm].hash.name,
                },
              );

              // 3.
              return data.buffer;
            }
            case "spki": {
              // 1.
              if (key[_type] !== "public") {
                throw new DOMException(
                  "Key is not a public key",
                  "InvalidAccessError",
                );
              }

              // 2.
              const data = await core.opAsync(
                "op_crypto_export_key",
                {
                  key: innerKey,
                  format: "spki",
                  algorithm: "RSA-OAEP",
                  hash: key[_algorithm].hash.name,
                },
              );

              // 3.
              return data.buffer;
            }
            default:
              throw new DOMException("Not implemented", "NotSupportedError");
          }
        }
        // TODO(@littledivy): ECDSA
        default:
          throw new DOMException("Not implemented", "NotSupportedError");
      }
    }

    /**
     * @param {AlgorithmIdentifier} algorithm
     * @param {CryptoKey} baseKey
     * @param {number} length
     * @returns {Promise<ArrayBuffer>}
     */
    async deriveBits(algorithm, baseKey, length) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'deriveBits' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 3, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      baseKey = webidl.converters.CryptoKey(baseKey, {
        prefix,
        context: "Argument 2",
      });
      length = webidl.converters["unsigned long"](length, {
        prefix,
        context: "Argument 3",
      });

      // 2.
      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "deriveBits");
      // 4-6.
      const result = await deriveBits(normalizedAlgorithm, baseKey, length);
      // 7.
      if (normalizedAlgorithm.name !== baseKey[_algorithm].name) {
        throw new DOMException("Invalid algorithm name", "InvalidAccessError");
      }
      // 8.
      if (!ArrayPrototypeIncludes(baseKey[_usages], "deriveBits")) {
        throw new DOMException(
          "baseKey usages does not contain `deriveBits`",
          "InvalidAccessError",
        );
      }
      // 9-10.
      return result;
    }

    /**
     * @param {AlgorithmIdentifier} algorithm
     * @param {CryptoKey} baseKey
     * @param {number} length
     * @returns {Promise<ArrayBuffer>}
     */
    async deriveKey(
      algorithm,
      baseKey,
      derivedKeyType,
      extractable,
      keyUsages,
    ) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'deriveKey' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 5, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      baseKey = webidl.converters.CryptoKey(baseKey, {
        prefix,
        context: "Argument 2",
      });
      derivedKeyType = webidl.converters.AlgorithmIdentifier(derivedKeyType, {
        prefix,
        context: "Argument 3",
      });
      extractable = webidl.converters["boolean"](extractable, {
        prefix,
        context: "Argument 4",
      });
      keyUsages = webidl.converters["sequence<KeyUsage>"](keyUsages, {
        prefix,
        context: "Argument 5",
      });

      // 2-3.
      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "deriveBits");

      // 4-5.
      const normalizedDerivedKeyAlgorithmImport = normalizeAlgorithm(
        derivedKeyType,
        "importKey",
      );

      // 6-7.
      const normalizedDerivedKeyAlgorithmLength = normalizeAlgorithm(
        derivedKeyType,
        "get key length",
      );

      // 8-10.

      // 11.
      if (normalizedAlgorithm.name !== baseKey[_algorithm].name) {
        throw new DOMException(
          "Invalid algorithm name",
          "InvalidAccessError",
        );
      }

      // 12.
      if (!ArrayPrototypeIncludes(baseKey[_usages], "deriveKey")) {
        throw new DOMException(
          "baseKey usages does not contain `deriveKey`",
          "InvalidAccessError",
        );
      }

      // 13.
      const length = getKeyLength(normalizedDerivedKeyAlgorithmLength);

      // 14.
      const secret = await this.deriveBits(
        normalizedAlgorithm,
        baseKey,
        length,
      );

      // 15.
      const result = await this.importKey(
        "raw",
        secret,
        normalizedDerivedKeyAlgorithmImport,
        extractable,
        keyUsages,
      );

      // 16.
      if (
        ArrayPrototypeIncludes(["private", "secret"], result[_type]) &&
        keyUsages.length == 0
      ) {
        throw new SyntaxError("Invalid key usages");
      }
      // 17.
      return result;
    }

    /**
     * @param {string} algorithm
     * @param {CryptoKey} key
     * @param {BufferSource} signature
     * @param {BufferSource} data
     * @returns {Promise<boolean>}
     */
    async verify(algorithm, key, signature, data) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'verify' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 4, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      key = webidl.converters.CryptoKey(key, {
        prefix,
        context: "Argument 2",
      });
      signature = webidl.converters.BufferSource(signature, {
        prefix,
        context: "Argument 3",
      });
      data = webidl.converters.BufferSource(data, {
        prefix,
        context: "Argument 4",
      });

      // 2.
      if (ArrayBufferIsView(signature)) {
        signature = new Uint8Array(
          signature.buffer,
          signature.byteOffset,
          signature.byteLength,
        );
      } else {
        signature = new Uint8Array(signature);
      }
      signature = TypedArrayPrototypeSlice(signature);

      // 3.
      if (ArrayBufferIsView(data)) {
        data = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      } else {
        data = new Uint8Array(data);
      }
      data = TypedArrayPrototypeSlice(data);

      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "verify");

      const handle = key[_handle];
      const keyData = WeakMapPrototypeGet(KEY_STORE, handle);

      if (normalizedAlgorithm.name !== key[_algorithm].name) {
        throw new DOMException(
          "Verifying algorithm doesn't match key algorithm.",
          "InvalidAccessError",
        );
      }

      if (!ArrayPrototypeIncludes(key[_usages], "verify")) {
        throw new DOMException(
          "Key does not support the 'verify' operation.",
          "InvalidAccessError",
        );
      }

      switch (normalizedAlgorithm.name) {
        case "RSASSA-PKCS1-v1_5": {
          if (key[_type] !== "public") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          const hashAlgorithm = key[_algorithm].hash.name;
          return await core.opAsync("op_crypto_verify_key", {
            key: keyData,
            algorithm: "RSASSA-PKCS1-v1_5",
            hash: hashAlgorithm,
            signature,
          }, data);
        }
        case "RSA-PSS": {
          if (key[_type] !== "public") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }

          const hashAlgorithm = key[_algorithm].hash.name;
          const saltLength = normalizedAlgorithm.saltLength;
          return await core.opAsync("op_crypto_verify_key", {
            key: keyData,
            algorithm: "RSA-PSS",
            hash: hashAlgorithm,
            saltLength,
            signature,
          }, data);
        }
        case "HMAC": {
          const hash = key[_algorithm].hash.name;
          return await core.opAsync("op_crypto_verify_key", {
            key: keyData,
            algorithm: "HMAC",
            hash,
            signature,
          }, data);
        }
        case "ECDSA": {
          // 1.
          if (key[_type] !== "public") {
            throw new DOMException(
              "Key type not supported",
              "InvalidAccessError",
            );
          }
          // 2.
          const hash = normalizedAlgorithm.hash.name;
          // 3-8.
          return await core.opAsync("op_crypto_verify_key", {
            key: keyData,
            algorithm: "ECDSA",
            hash,
            signature,
            namedCurve: key[_algorithm].namedCurve,
          }, data);
        }
      }

      throw new TypeError("unreachable");
    }

    /**
     * @param {string} algorithm
     * @param {boolean} extractable
     * @param {KeyUsage[]} keyUsages
     * @returns {Promise<any>}
     */
    async wrapKey(format, key, wrappingKey, wrapAlgorithm) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'wrapKey' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 4, { prefix });
      format = webidl.converters.KeyFormat(format, {
        prefix,
        context: "Argument 1",
      });
      key = webidl.converters.CryptoKey(key, {
        prefix,
        context: "Argument 2",
      });
      wrappingKey = webidl.converters.CryptoKey(wrappingKey, {
        prefix,
        context: "Argument 3",
      });
      wrapAlgorithm = webidl.converters.AlgorithmIdentifier(wrapAlgorithm, {
        prefix,
        context: "Argument 4",
      });

      let normalizedAlgorithm;

      try {
        // 2.
        normalizedAlgorithm = normalizeAlgorithm(wrapAlgorithm, "wrapKey");
      } catch (_) {
        // 3.
        normalizedAlgorithm = normalizeAlgorithm(wrapAlgorithm, "encrypt");
      }

      // 8.
      if (normalizedAlgorithm.name !== wrappingKey[_algorithm].name) {
        throw new DOMException(
          "Wrapping algorithm doesn't match key algorithm.",
          "InvalidAccessError",
        );
      }

      // 9.
      if (!ArrayPrototypeIncludes(wrappingKey[_usages], "wrapKey")) {
        throw new DOMException(
          "Key does not support the 'wrapKey' operation.",
          "InvalidAccessError",
        );
      }

      // 10. NotSupportedError will be thrown in step 12.
      // 11.
      if (key[_extractable] === false) {
        throw new DOMException(
          "Key is not extractable",
          "InvalidAccessError",
        );
      }

      // 12.
      const exportedKey = await this.exportKey(format, key);

      let bytes;
      // 13.
      if (format !== "jwk") {
        bytes = exportedKey;
      } else {
        // TODO(@littledivy): Implement JWK.
        throw new DOMException(
          "Not implemented",
          "NotSupportedError",
        );
      }

      // 14-15.
      if (
        supportedAlgorithms["wrapKey"][normalizedAlgorithm.name] !== undefined
      ) {
        // TODO(@littledivy): Implement this for AES-KW.
        throw new DOMException(
          "Not implemented",
          "NotSupportedError",
        );
      } else if (
        supportedAlgorithms["encrypt"][normalizedAlgorithm.name] !== undefined
      ) {
        return this.encrypt(normalizedAlgorithm, wrappingKey, bytes);
      } else {
        throw new DOMException(
          "Algorithm not supported",
          "NotSupportedError",
        );
      }
    }

    /**
     * @param {string} algorithm
     * @param {boolean} extractable
     * @param {KeyUsage[]} keyUsages
     * @returns {Promise<any>}
     */
    async generateKey(algorithm, extractable, keyUsages) {
      webidl.assertBranded(this, SubtleCrypto);
      const prefix = "Failed to execute 'generateKey' on 'SubtleCrypto'";
      webidl.requiredArguments(arguments.length, 3, { prefix });
      algorithm = webidl.converters.AlgorithmIdentifier(algorithm, {
        prefix,
        context: "Argument 1",
      });
      extractable = webidl.converters["boolean"](extractable, {
        prefix,
        context: "Argument 2",
      });
      keyUsages = webidl.converters["sequence<KeyUsage>"](keyUsages, {
        prefix,
        context: "Argument 3",
      });

      const usages = keyUsages;

      const normalizedAlgorithm = normalizeAlgorithm(algorithm, "generateKey");

      const result = await generateKey(
        normalizedAlgorithm,
        extractable,
        usages,
      );

      if (result instanceof CryptoKey) {
        const type = result[_type];
        if ((type === "secret" || type === "private") && usages.length === 0) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }
      } else if (result.privateKey instanceof CryptoKey) {
        if (result.privateKey[_usages].length === 0) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }
      }

      return result;
    }
  }

  async function generateKey(normalizedAlgorithm, extractable, usages) {
    switch (normalizedAlgorithm.name) {
      case "RSASSA-PKCS1-v1_5":
      case "RSA-PSS": {
        // 1.
        if (
          ArrayPrototypeFind(
            usages,
            (u) => !ArrayPrototypeIncludes(["sign", "verify"], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        // 2.
        const keyData = await core.opAsync(
          "op_crypto_generate_key",
          {
            name: normalizedAlgorithm.name,
            modulusLength: normalizedAlgorithm.modulusLength,
            publicExponent: normalizedAlgorithm.publicExponent,
          },
        );
        const handle = {};
        WeakMapPrototypeSet(KEY_STORE, handle, {
          // PKCS#1 for RSA
          type: "raw",
          data: keyData,
        });

        // 4-8.
        const algorithm = {
          name: normalizedAlgorithm.name,
          modulusLength: normalizedAlgorithm.modulusLength,
          publicExponent: normalizedAlgorithm.publicExponent,
          hash: normalizedAlgorithm.hash,
        };

        // 9-13.
        const publicKey = constructKey(
          "public",
          true,
          usageIntersection(usages, ["verify"]),
          algorithm,
          handle,
        );

        // 14-18.
        const privateKey = constructKey(
          "private",
          extractable,
          usageIntersection(usages, ["sign"]),
          algorithm,
          handle,
        );

        // 19-22.
        return { publicKey, privateKey };
      }
      case "RSA-OAEP": {
        if (
          ArrayPrototypeFind(
            usages,
            (u) =>
              !ArrayPrototypeIncludes([
                "encrypt",
                "decrypt",
                "wrapKey",
                "unwrapKey",
              ], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        // 2.
        const keyData = await core.opAsync(
          "op_crypto_generate_key",
          {
            name: normalizedAlgorithm.name,
            modulusLength: normalizedAlgorithm.modulusLength,
            publicExponent: normalizedAlgorithm.publicExponent,
          },
        );
        const handle = {};
        WeakMapPrototypeSet(KEY_STORE, handle, {
          // PKCS#1 for RSA
          type: "raw",
          data: keyData,
        });

        // 4-8.
        const algorithm = {
          name: normalizedAlgorithm.name,
          modulusLength: normalizedAlgorithm.modulusLength,
          publicExponent: normalizedAlgorithm.publicExponent,
          hash: normalizedAlgorithm.hash,
        };

        // 9-13.
        const publicKey = constructKey(
          "public",
          true,
          usageIntersection(usages, ["encrypt", "wrapKey"]),
          algorithm,
          handle,
        );

        // 14-18.
        const privateKey = constructKey(
          "private",
          extractable,
          usageIntersection(usages, ["decrypt", "unwrapKey"]),
          algorithm,
          handle,
        );

        // 19-22.
        return { publicKey, privateKey };
      }
      case "ECDSA": {
        // 1.
        if (
          ArrayPrototypeFind(
            usages,
            (u) => !ArrayPrototypeIncludes(["sign", "verify"], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        // 2-3.
        const handle = {};
        if (
          ArrayPrototypeIncludes(
            supportedNamedCurves,
            normalizedAlgorithm.namedCurve,
          )
        ) {
          const keyData = await core.opAsync("op_crypto_generate_key", {
            name: "ECDSA",
            namedCurve: normalizedAlgorithm.namedCurve,
          });
          WeakMapPrototypeSet(KEY_STORE, handle, {
            type: "pkcs8",
            data: keyData,
          });
        } else {
          throw new DOMException("Curve not supported", "NotSupportedError");
        }

        // 4-6.
        const algorithm = {
          name: "ECDSA",
          namedCurve: normalizedAlgorithm.namedCurve,
        };

        // 7-11.
        const publicKey = constructKey(
          "public",
          true,
          usageIntersection(usages, ["verify"]),
          algorithm,
          handle,
        );

        // 12-16.
        const privateKey = constructKey(
          "private",
          extractable,
          usageIntersection(usages, ["sign"]),
          algorithm,
          handle,
        );

        // 17-20.
        return { publicKey, privateKey };
      }
      case "ECDH": {
        // 1.
        if (
          ArrayPrototypeFind(
            usages,
            (u) => !ArrayPrototypeIncludes(["deriveKey", "deriveBits"], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        // 2-3.
        const handle = {};
        if (
          ArrayPrototypeIncludes(
            supportedNamedCurves,
            normalizedAlgorithm.namedCurve,
          )
        ) {
          const keyData = await core.opAsync("op_crypto_generate_key", {
            name: "ECDH",
            namedCurve: normalizedAlgorithm.namedCurve,
          });
          WeakMapPrototypeSet(KEY_STORE, handle, {
            type: "pkcs8",
            data: keyData,
          });
        } else {
          throw new DOMException("Curve not supported", "NotSupportedError");
        }

        // 4-6.
        const algorithm = {
          name: "ECDH",
          namedCurve: normalizedAlgorithm.namedCurve,
        };

        // 7-11.
        const publicKey = constructKey(
          "public",
          true,
          usageIntersection(usages, []),
          algorithm,
          handle,
        );

        // 12-16.
        const privateKey = constructKey(
          "private",
          extractable,
          usageIntersection(usages, ["deriveKey", "deriveBits"]),
          algorithm,
          handle,
        );

        // 17-20.
        return { publicKey, privateKey };
      }
      case "AES-CTR":
      case "AES-CBC":
      case "AES-GCM": {
        // 1.
        if (
          ArrayPrototypeFind(
            usages,
            (u) =>
              !ArrayPrototypeIncludes([
                "encrypt",
                "decrypt",
                "wrapKey",
                "unwrapKey",
              ], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        return generateKeyAES(normalizedAlgorithm, extractable, usages);
      }
      case "AES-KW": {
        // 1.
        if (
          ArrayPrototypeFind(
            usages,
            (u) => !ArrayPrototypeIncludes(["wrapKey", "unwrapKey"], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        return generateKeyAES(normalizedAlgorithm, extractable, usages);
      }
      case "HMAC": {
        // 1.
        if (
          ArrayPrototypeFind(
            usages,
            (u) => !ArrayPrototypeIncludes(["sign", "verify"], u),
          ) !== undefined
        ) {
          throw new DOMException("Invalid key usages", "SyntaxError");
        }

        // 2.
        let length;
        if (normalizedAlgorithm.length === undefined) {
          length = null;
        } else if (normalizedAlgorithm.length !== 0) {
          length = normalizedAlgorithm.length;
        } else {
          throw new DOMException("Invalid length", "OperationError");
        }

        // 3-4.
        const keyData = await core.opAsync("op_crypto_generate_key", {
          name: "HMAC",
          hash: normalizedAlgorithm.hash.name,
          length,
        });
        const handle = {};
        WeakMapPrototypeSet(KEY_STORE, handle, { type: "raw", data: keyData });

        // 6-10.
        const algorithm = {
          name: "HMAC",
          hash: {
            name: normalizedAlgorithm.hash.name,
          },
          length: keyData.byteLength * 8,
        };

        // 5, 11-13.
        const key = constructKey(
          "secret",
          extractable,
          usages,
          algorithm,
          handle,
        );

        // 14.
        return key;
      }
    }
  }

  async function generateKeyAES(normalizedAlgorithm, extractable, usages) {
    // 2.
    if (!ArrayPrototypeIncludes([128, 192, 256], normalizedAlgorithm.length)) {
      throw new DOMException("Invalid key length", "OperationError");
    }

    // 3.
    const keyData = await core.opAsync("op_crypto_generate_key", {
      name: normalizedAlgorithm.name,
      length: normalizedAlgorithm.length,
    });
    const handle = {};
    WeakMapPrototypeSet(KEY_STORE, handle, {
      type: "raw",
      data: keyData,
    });

    // 6-8.
    const algorithm = {
      name: normalizedAlgorithm.name,
      length: normalizedAlgorithm.length,
    };

    // 9-11.
    const key = constructKey(
      "secret",
      extractable,
      usages,
      algorithm,
      handle,
    );

    // 12.
    return key;
  }

  async function deriveBits(normalizedAlgorithm, baseKey, length) {
    switch (normalizedAlgorithm.name) {
      case "PBKDF2": {
        // 1.
        if (length == null || length == 0 || length % 8 !== 0) {
          throw new DOMException("Invalid length", "OperationError");
        }

        if (normalizedAlgorithm.iterations == 0) {
          throw new DOMException(
            "iterations must not be zero",
            "OperationError",
          );
        }

        const handle = baseKey[_handle];
        const keyData = WeakMapPrototypeGet(KEY_STORE, handle);

        if (ArrayBufferIsView(normalizedAlgorithm.salt)) {
          normalizedAlgorithm.salt = new Uint8Array(
            normalizedAlgorithm.salt.buffer,
            normalizedAlgorithm.salt.byteOffset,
            normalizedAlgorithm.salt.byteLength,
          );
        } else {
          normalizedAlgorithm.salt = new Uint8Array(normalizedAlgorithm.salt);
        }
        normalizedAlgorithm.salt = TypedArrayPrototypeSlice(
          normalizedAlgorithm.salt,
        );

        const buf = await core.opAsync("op_crypto_derive_bits", {
          key: keyData,
          algorithm: "PBKDF2",
          hash: normalizedAlgorithm.hash.name,
          iterations: normalizedAlgorithm.iterations,
          length,
        }, normalizedAlgorithm.salt);

        return buf.buffer;
      }
      case "ECDH": {
        // 1.
        if (baseKey[_type] !== "private") {
          throw new DOMException("Invalid key type", "InvalidAccessError");
        }
        // 2.
        const publicKey = normalizedAlgorithm.public;
        // 3.
        if (publicKey[_type] !== "public") {
          throw new DOMException("Invalid key type", "InvalidAccessError");
        }
        // 4.
        if (publicKey[_algorithm].name !== baseKey[_algorithm].name) {
          throw new DOMException(
            "Algorithm mismatch",
            "InvalidAccessError",
          );
        }
        // 5.
        if (
          publicKey[_algorithm].namedCurve !== baseKey[_algorithm].namedCurve
        ) {
          throw new DOMException(
            "namedCurve mismatch",
            "InvalidAccessError",
          );
        }
        // 6.
        if (
          ArrayPrototypeIncludes(
            supportedNamedCurves,
            publicKey[_algorithm].namedCurve,
          )
        ) {
          const baseKeyhandle = baseKey[_handle];
          const baseKeyData = WeakMapPrototypeGet(KEY_STORE, baseKeyhandle);
          const publicKeyhandle = baseKey[_handle];
          const publicKeyData = WeakMapPrototypeGet(KEY_STORE, publicKeyhandle);

          const buf = await core.opAsync("op_crypto_derive_bits", {
            key: baseKeyData,
            publicKey: publicKeyData,
            algorithm: "ECDH",
            namedCurve: publicKey[_algorithm].namedCurve,
            length,
          });

          return buf.buffer;
        } else {
          throw new DOMException("Not implemented", "NotSupportedError");
        }
      }
      case "HKDF": {
        // 1.
        if (length === null || length === 0 || length % 8 !== 0) {
          throw new DOMException("Invalid length", "OperationError");
        }

        const handle = baseKey[_handle];
        const keyDerivationKey = WeakMapPrototypeGet(KEY_STORE, handle);

        if (ArrayBufferIsView(normalizedAlgorithm.salt)) {
          normalizedAlgorithm.salt = new Uint8Array(
            normalizedAlgorithm.salt.buffer,
            normalizedAlgorithm.salt.byteOffset,
            normalizedAlgorithm.salt.byteLength,
          );
        } else {
          normalizedAlgorithm.salt = new Uint8Array(normalizedAlgorithm.salt);
        }
        normalizedAlgorithm.salt = TypedArrayPrototypeSlice(
          normalizedAlgorithm.salt,
        );

        if (ArrayBufferIsView(normalizedAlgorithm.info)) {
          normalizedAlgorithm.info = new Uint8Array(
            normalizedAlgorithm.info.buffer,
            normalizedAlgorithm.info.byteOffset,
            normalizedAlgorithm.info.byteLength,
          );
        } else {
          normalizedAlgorithm.info = new Uint8Array(normalizedAlgorithm.info);
        }
        normalizedAlgorithm.info = TypedArrayPrototypeSlice(
          normalizedAlgorithm.info,
        );

        const buf = await core.opAsync("op_crypto_derive_bits", {
          key: keyDerivationKey,
          algorithm: "HKDF",
          hash: normalizedAlgorithm.hash.name,
          info: normalizedAlgorithm.info,
          length,
        }, normalizedAlgorithm.salt);

        return buf.buffer;
      }
      default:
        throw new DOMException("Not implemented", "NotSupportedError");
    }
  }

  webidl.configurePrototype(SubtleCrypto);
  const subtle = webidl.createBranded(SubtleCrypto);

  class Crypto {
    constructor() {
      webidl.illegalConstructor();
    }

    getRandomValues(arrayBufferView) {
      webidl.assertBranded(this, Crypto);
      const prefix = "Failed to execute 'getRandomValues' on 'Crypto'";
      webidl.requiredArguments(arguments.length, 1, { prefix });
      arrayBufferView = webidl.converters.ArrayBufferView(arrayBufferView, {
        prefix,
        context: "Argument 1",
      });
      if (
        !(
          arrayBufferView instanceof Int8Array ||
          arrayBufferView instanceof Uint8Array ||
          arrayBufferView instanceof Uint8ClampedArray ||
          arrayBufferView instanceof Int16Array ||
          arrayBufferView instanceof Uint16Array ||
          arrayBufferView instanceof Int32Array ||
          arrayBufferView instanceof Uint32Array ||
          arrayBufferView instanceof BigInt64Array ||
          arrayBufferView instanceof BigUint64Array
        )
      ) {
        throw new DOMException(
          "The provided ArrayBufferView is not an integer array type",
          "TypeMismatchError",
        );
      }
      const ui8 = new Uint8Array(
        arrayBufferView.buffer,
        arrayBufferView.byteOffset,
        arrayBufferView.byteLength,
      );
      core.opSync("op_crypto_get_random_values", ui8);
      return arrayBufferView;
    }

    randomUUID() {
      webidl.assertBranded(this, Crypto);
      return core.opSync("op_crypto_random_uuid");
    }

    get subtle() {
      webidl.assertBranded(this, Crypto);
      return subtle;
    }

    [SymbolFor("Deno.customInspect")](inspect) {
      return `${this.constructor.name} ${inspect({})}`;
    }
  }

  webidl.configurePrototype(Crypto);

  window.__bootstrap.crypto = {
    SubtleCrypto,
    crypto: webidl.createBranded(Crypto),
    Crypto,
    CryptoKey,
  };
})(this);
