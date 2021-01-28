// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

/// <reference no-default-lib="true" />
/// <reference lib="esnext" />

declare namespace globalThis {
  declare namespace __bootstrap {
    declare var url: {
      URLSearchParams: typeof URLSearchParams;
    };

    declare var location: {
      getLocationHref(): string | undefined;
    };
  }
}
