// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
import * as domTypes from "./dom_types.ts";
import * as blob from "./blob.ts";

export class DomFileImpl extends blob.DenoBlob implements domTypes.DomFile {
  lastModified: number;
  name: string;

  constructor(
    fileBits: domTypes.BlobPart[],
    fileName: string,
    options?: domTypes.FilePropertyBag
  ) {
    options = options || {};
    super(fileBits, options);

    // 4.1.2.1 Replace any "/" character (U+002F SOLIDUS)
    // with a ":" (U + 003A COLON)
    this.name = String(fileName).replace(/\u002F/g, "\u003A");
    // 4.1.3.3 If lastModified is not provided, set lastModified to the current
    // date and time represented in number of milliseconds since the Unix Epoch.
    this.lastModified = options.lastModified || Date.now();
  }
}
