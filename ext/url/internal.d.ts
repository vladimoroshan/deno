// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

/// <reference no-default-lib="true" />
/// <reference lib="esnext" />

declare module "internal:deno_url/00_url.js" {
  const URL: typeof URL;
  const URLSearchParams: typeof URLSearchParams;
  function parseUrlEncoded(bytes: Uint8Array): [string, string][];
}

declare module "internal:deno_url/01_urlpattern.js" {
  const URLPattern: typeof URLPattern;
}
