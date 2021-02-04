// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.
"use strict";

((window) => {
  const { open, openSync } = window.__bootstrap.files;
  const { readAll, readAllSync } = window.__bootstrap.buffer;

  function readFileSync(path) {
    const file = openSync(path);
    const contents = readAllSync(file);
    file.close();
    return contents;
  }

  async function readFile(path) {
    const file = await open(path);
    const contents = await readAll(file);
    file.close();
    return contents;
  }

  function readTextFileSync(path) {
    const file = openSync(path);
    const contents = readAllSync(file);
    file.close();
    const decoder = new TextDecoder();
    return decoder.decode(contents);
  }

  async function readTextFile(path) {
    const file = await open(path);
    const contents = await readAll(file);
    file.close();
    const decoder = new TextDecoder();
    return decoder.decode(contents);
  }

  window.__bootstrap.readFile = {
    readFile,
    readFileSync,
    readTextFileSync,
    readTextFile,
  };
})(this);
