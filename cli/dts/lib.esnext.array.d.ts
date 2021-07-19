/*! *****************************************************************************
Copyright (c) Microsoft Corporation. All rights reserved.
Licensed under the Apache License, Version 2.0 (the "License"); you may not use
this file except in compliance with the License. You may obtain a copy of the
License at http://www.apache.org/licenses/LICENSE-2.0

THIS CODE IS PROVIDED ON AN *AS IS* BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
KIND, EITHER EXPRESS OR IMPLIED, INCLUDING WITHOUT LIMITATION ANY IMPLIED
WARRANTIES OR CONDITIONS OF TITLE, FITNESS FOR A PARTICULAR PURPOSE,
MERCHANTABLITY OR NON-INFRINGEMENT.

See the Apache Version 2.0 License for specific language governing permissions
and limitations under the License.
***************************************************************************** */

/// <reference no-default-lib="true"/>

interface Array<T> {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): T | undefined;
}

interface ReadonlyArray<T> {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): T | undefined;
}

interface Int8Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Uint8Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Uint8ClampedArray {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Int16Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Uint16Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Int32Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Uint32Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Float32Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface Float64Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): number | undefined;
}

interface BigInt64Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): bigint | undefined;
}

interface BigUint64Array {
  /**
   * Access item by relative indexing.
   * @param index index to access.
   */
  at(index: number): bigint | undefined;
}
