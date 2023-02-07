// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

/// <reference no-default-lib="true" />
/// <reference lib="esnext" />

declare module "internal:ext/console/02_console.js" {
  function createFilteredInspectProxy<TObject>(params: {
    object: TObject;
    keys: (keyof TObject)[];
    evaluate: boolean;
  }): Record<string, unknown>;
}
