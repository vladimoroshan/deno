// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

/// <reference no-default-lib="true"/>

interface ReadableStream<R = any> {
  [Symbol.asyncIterator](options?: {
    preventCancel?: boolean;
  }): AsyncIterableIterator<R>;
}
