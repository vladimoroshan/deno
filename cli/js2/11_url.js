// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

((window) => {
  const { build } = window.__bootstrap.build;
  const { getRandomValues } = window.__bootstrap.crypto;
  const { customInspect } = window.__bootstrap.console;
  const { sendSync } = window.__bootstrap.dispatchJson;
  const { isIterable, requiredArguments } = window.__bootstrap.webUtil;

  /** https://url.spec.whatwg.org/#idna */
  function domainToAscii(
    domain,
    { beStrict = false } = {},
  ) {
    return sendSync("op_domain_to_ascii", { domain, beStrict });
  }

  const urls = new WeakMap();

  class URLSearchParams {
    #params = [];

    constructor(init = "") {
      if (typeof init === "string") {
        this.#handleStringInitialization(init);
        return;
      }

      if (Array.isArray(init) || isIterable(init)) {
        this.#handleArrayInitialization(init);
        return;
      }

      if (Object(init) !== init) {
        return;
      }

      if (init instanceof URLSearchParams) {
        this.#params = [...init.#params];
        return;
      }

      // Overload: record<USVString, USVString>
      for (const key of Object.keys(init)) {
        this.#append(key, init[key]);
      }

      urls.set(this, null);
    }

    #handleStringInitialization = (init) => {
      // Overload: USVString
      // If init is a string and starts with U+003F (?),
      // remove the first code point from init.
      if (init.charCodeAt(0) === 0x003f) {
        init = init.slice(1);
      }

      for (const pair of init.split("&")) {
        // Empty params are ignored
        if (pair.length === 0) {
          continue;
        }
        const position = pair.indexOf("=");
        const name = pair.slice(0, position === -1 ? pair.length : position);
        const value = pair.slice(name.length + 1);
        this.#append(decodeURIComponent(name), decodeURIComponent(value));
      }
    };

    #handleArrayInitialization = (
      init,
    ) => {
      // Overload: sequence<sequence<USVString>>
      for (const tuple of init) {
        // If pair does not contain exactly two items, then throw a TypeError.
        if (tuple.length !== 2) {
          throw new TypeError(
            "URLSearchParams.constructor tuple array argument must only contain pair elements",
          );
        }
        this.#append(tuple[0], tuple[1]);
      }
    };

    #updateSteps = () => {
      const url = urls.get(this);
      if (url == null) {
        return;
      }
      parts.get(url).query = this.toString();
    };

    #append = (name, value) => {
      this.#params.push([String(name), String(value)]);
    };

    append(name, value) {
      requiredArguments("URLSearchParams.append", arguments.length, 2);
      this.#append(name, value);
      this.#updateSteps();
    }

    delete(name) {
      requiredArguments("URLSearchParams.delete", arguments.length, 1);
      name = String(name);
      let i = 0;
      while (i < this.#params.length) {
        if (this.#params[i][0] === name) {
          this.#params.splice(i, 1);
        } else {
          i++;
        }
      }
      this.#updateSteps();
    }

    getAll(name) {
      requiredArguments("URLSearchParams.getAll", arguments.length, 1);
      name = String(name);
      const values = [];
      for (const entry of this.#params) {
        if (entry[0] === name) {
          values.push(entry[1]);
        }
      }

      return values;
    }

    get(name) {
      requiredArguments("URLSearchParams.get", arguments.length, 1);
      name = String(name);
      for (const entry of this.#params) {
        if (entry[0] === name) {
          return entry[1];
        }
      }

      return null;
    }

    has(name) {
      requiredArguments("URLSearchParams.has", arguments.length, 1);
      name = String(name);
      return this.#params.some((entry) => entry[0] === name);
    }

    set(name, value) {
      requiredArguments("URLSearchParams.set", arguments.length, 2);

      // If there are any name-value pairs whose name is name, in list,
      // set the value of the first such name-value pair to value
      // and remove the others.
      name = String(name);
      value = String(value);
      let found = false;
      let i = 0;
      while (i < this.#params.length) {
        if (this.#params[i][0] === name) {
          if (!found) {
            this.#params[i][1] = value;
            found = true;
            i++;
          } else {
            this.#params.splice(i, 1);
          }
        } else {
          i++;
        }
      }

      // Otherwise, append a new name-value pair whose name is name
      // and value is value, to list.
      if (!found) {
        this.#append(name, value);
      }

      this.#updateSteps();
    }

    sort() {
      this.#params.sort((a, b) => (a[0] === b[0] ? 0 : a[0] > b[0] ? 1 : -1));
      this.#updateSteps();
    }

    forEach(
      callbackfn,
      thisArg,
    ) {
      requiredArguments("URLSearchParams.forEach", arguments.length, 1);

      if (typeof thisArg !== "undefined") {
        callbackfn = callbackfn.bind(thisArg);
      }

      for (const [key, value] of this.#params) {
        callbackfn(value, key, this);
      }
    }

    *keys() {
      for (const [key] of this.#params) {
        yield key;
      }
    }

    *values() {
      for (const [, value] of this.#params) {
        yield value;
      }
    }

    *entries() {
      yield* this.#params;
    }

    *[Symbol.iterator]() {
      yield* this.#params;
    }

    toString() {
      return this.#params
        .map(
          (tuple) =>
            `${encodeURIComponent(tuple[0])}=${encodeURIComponent(tuple[1])}`,
        )
        .join("&");
    }
  }

  const searchParamsMethods = [
    "append",
    "delete",
    "set",
  ];

  const specialSchemes = ["ftp", "file", "http", "https", "ws", "wss"];

  // https://url.spec.whatwg.org/#special-scheme
  const schemePorts = {
    ftp: "21",
    file: "",
    http: "80",
    https: "443",
    ws: "80",
    wss: "443",
  };
  const MAX_PORT = 2 ** 16 - 1;

  // Remove the part of the string that matches the pattern and return the
  // remainder (RHS) as well as the first captured group of the matched substring
  // (LHS). e.g.
  //      takePattern("https://deno.land:80", /^([a-z]+):[/]{2}/)
  //        = ["http", "deno.land:80"]
  //      takePattern("deno.land:80", /^(\[[0-9a-fA-F.:]{2,}\]|[^:]+)/)
  //        = ["deno.land", "80"]
  function takePattern(string, pattern) {
    let capture = "";
    const rest = string.replace(pattern, (_, capture_) => {
      capture = capture_;
      return "";
    });
    return [capture, rest];
  }

  function parse(url, isBase = true) {
    const parts = {};
    let restUrl;
    [parts.protocol, restUrl] = takePattern(url.trim(), /^([a-z]+):/);
    if (isBase && parts.protocol == "") {
      return undefined;
    }
    const isSpecial = specialSchemes.includes(parts.protocol);
    if (parts.protocol == "file") {
      parts.slashes = "//";
      parts.username = "";
      parts.password = "";
      [parts.hostname, restUrl] = takePattern(restUrl, /^[/\\]{2}([^/\\?#]*)/);
      parts.port = "";
      if (build.os == "windows" && parts.hostname == "") {
        // UNC paths. e.g. "\\\\localhost\\foo\\bar" on Windows should be
        // representable as `new URL("file:////localhost/foo/bar")` which is
        // equivalent to: `new URL("file://localhost/foo/bar")`.
        [parts.hostname, restUrl] = takePattern(
          restUrl,
          /^[/\\]{2,}([^/\\?#]*)/,
        );
      }
    } else {
      let restAuthority;
      if (isSpecial) {
        parts.slashes = "//";
        [restAuthority, restUrl] = takePattern(
          restUrl,
          /^[/\\]{2,}([^/\\?#]*)/,
        );
      } else {
        parts.slashes = restUrl.match(/^[/\\]{2}/) ? "//" : "";
        [restAuthority, restUrl] = takePattern(restUrl, /^[/\\]{2}([^/\\?#]*)/);
      }
      let restAuthentication;
      [restAuthentication, restAuthority] = takePattern(
        restAuthority,
        /^(.*)@/,
      );
      [parts.username, restAuthentication] = takePattern(
        restAuthentication,
        /^([^:]*)/,
      );
      parts.username = encodeUserinfo(parts.username);
      [parts.password] = takePattern(restAuthentication, /^:(.*)/);
      parts.password = encodeUserinfo(parts.password);
      [parts.hostname, restAuthority] = takePattern(
        restAuthority,
        /^(\[[0-9a-fA-F.:]{2,}\]|[^:]+)/,
      );
      [parts.port] = takePattern(restAuthority, /^:(.*)/);
      if (!isValidPort(parts.port)) {
        return undefined;
      }
      if (parts.hostname == "" && isSpecial && isBase) {
        return undefined;
      }
    }
    try {
      parts.hostname = encodeHostname(parts.hostname, isSpecial);
    } catch {
      return undefined;
    }
    [parts.path, restUrl] = takePattern(restUrl, /^([^?#]*)/);
    parts.path = encodePathname(parts.path.replace(/\\/g, "/"));
    [parts.query, restUrl] = takePattern(restUrl, /^(\?[^#]*)/);
    parts.query = encodeSearch(parts.query);
    [parts.hash] = takePattern(restUrl, /^(#.*)/);
    parts.hash = encodeHash(parts.hash);
    return parts;
  }

  // Based on https://github.com/kelektiv/node-uuid
  // TODO(kevinkassimo): Use deno_std version once possible.
  function generateUUID() {
    return "00000000-0000-4000-8000-000000000000".replace(/[0]/g, () =>
      // random integer from 0 to 15 as a hex digit.
      (getRandomValues(new Uint8Array(1))[0] % 16).toString(16));
  }

  // Keep it outside of URL to avoid any attempts of access.
  const blobURLMap = new Map();

  function isAbsolutePath(path) {
    return path.startsWith("/");
  }

  // Resolves `.`s and `..`s where possible.
  // Preserves repeating and trailing `/`s by design.
  // On Windows, drive letter paths will be given a leading slash, and also a
  // trailing slash if there are no other components e.g. "C:" -> "/C:/".
  function normalizePath(path, isFilePath = false) {
    if (build.os == "windows" && isFilePath) {
      path = path.replace(/^\/*([A-Za-z]:)(\/|$)/, "/$1/");
    }
    const isAbsolute = isAbsolutePath(path);
    path = path.replace(/^\//, "");
    const pathSegments = path.split("/");

    const newPathSegments = [];
    for (let i = 0; i < pathSegments.length; i++) {
      const previous = newPathSegments[newPathSegments.length - 1];
      if (
        pathSegments[i] == ".." &&
        previous != ".." &&
        (previous != undefined || isAbsolute)
      ) {
        newPathSegments.pop();
      } else if (pathSegments[i] != ".") {
        newPathSegments.push(pathSegments[i]);
      }
    }

    let newPath = newPathSegments.join("/");
    if (!isAbsolute) {
      if (newPathSegments.length == 0) {
        newPath = ".";
      }
    } else {
      newPath = `/${newPath}`;
    }
    return newPath;
  }

  // Standard URL basing logic, applied to paths.
  function resolvePathFromBase(
    path,
    basePath,
    isFilePath = false,
  ) {
    let normalizedPath = normalizePath(path, isFilePath);
    let normalizedBasePath = normalizePath(basePath, isFilePath);

    let driveLetterPrefix = "";
    if (build.os == "windows" && isFilePath) {
      let driveLetter;
      let baseDriveLetter;
      [driveLetter, normalizedPath] = takePattern(
        normalizedPath,
        /^(\/[A-Za-z]:)(?=\/)/,
      );
      [baseDriveLetter, normalizedBasePath] = takePattern(
        normalizedBasePath,
        /^(\/[A-Za-z]:)(?=\/)/,
      );
      driveLetterPrefix = driveLetter || baseDriveLetter;
    }

    if (isAbsolutePath(normalizedPath)) {
      return `${driveLetterPrefix}${normalizedPath}`;
    }
    if (!isAbsolutePath(normalizedBasePath)) {
      throw new TypeError("Base path must be absolute.");
    }

    // Special case.
    if (path == "") {
      return `${driveLetterPrefix}${normalizedBasePath}`;
    }

    // Remove everything after the last `/` in `normalizedBasePath`.
    const prefix = normalizedBasePath.replace(/[^\/]*$/, "");
    // If `normalizedPath` ends with `.` or `..`, add a trailing slash.
    const suffix = normalizedPath.replace(/(?<=(^|\/)(\.|\.\.))$/, "/");

    return `${driveLetterPrefix}${normalizePath(prefix + suffix)}`;
  }

  function isValidPort(value) {
    // https://url.spec.whatwg.org/#port-state
    if (value === "") return true;

    const port = Number(value);
    return Number.isInteger(port) && port >= 0 && port <= MAX_PORT;
  }

  const parts = new WeakMap();

  class URL {
    #searchParams = null;

    [customInspect]() {
      const keys = [
        "href",
        "origin",
        "protocol",
        "username",
        "password",
        "host",
        "hostname",
        "port",
        "pathname",
        "hash",
        "search",
      ];
      const objectString = keys
        .map((key) => `${key}: "${this[key] || ""}"`)
        .join(", ");
      return `URL { ${objectString} }`;
    }

    #updateSearchParams = () => {
      const searchParams = new URLSearchParams(this.search);

      for (const methodName of searchParamsMethods) {
        const method = searchParams[methodName];
        searchParams[methodName] = (...args) => {
          method.apply(searchParams, args);
          this.search = searchParams.toString();
        };
      }
      this.#searchParams = searchParams;

      urls.set(searchParams, this);
    };

    get hash() {
      return parts.get(this).hash;
    }

    set hash(value) {
      value = unescape(String(value));
      if (!value) {
        parts.get(this).hash = "";
      } else {
        if (value.charAt(0) !== "#") {
          value = `#${value}`;
        }
        // hashes can contain % and # unescaped
        parts.get(this).hash = encodeHash(value);
      }
    }

    get host() {
      return `${this.hostname}${this.port ? `:${this.port}` : ""}`;
    }

    set host(value) {
      value = String(value);
      const url = new URL(`http://${value}`);
      parts.get(this).hostname = url.hostname;
      parts.get(this).port = url.port;
    }

    get hostname() {
      return parts.get(this).hostname;
    }

    set hostname(value) {
      value = String(value);
      try {
        const isSpecial = specialSchemes.includes(parts.get(this).protocol);
        parts.get(this).hostname = encodeHostname(value, isSpecial);
      } catch {}
    }

    get href() {
      const authentication = this.username || this.password
        ? `${this.username}${this.password ? ":" + this.password : ""}@`
        : "";
      const host = this.host;
      const slashes = host ? "//" : parts.get(this).slashes;
      let pathname = this.pathname;
      if (pathname.charAt(0) != "/" && pathname != "" && host != "") {
        pathname = `/${pathname}`;
      }
      return `${this.protocol}${slashes}${authentication}${host}${pathname}${this.search}${this.hash}`;
    }

    set href(value) {
      value = String(value);
      if (value !== this.href) {
        const url = new URL(value);
        parts.set(this, { ...parts.get(url) });
        this.#updateSearchParams();
      }
    }

    get origin() {
      if (this.host) {
        return `${this.protocol}//${this.host}`;
      }
      return "null";
    }

    get password() {
      return parts.get(this).password;
    }

    set password(value) {
      value = String(value);
      parts.get(this).password = encodeUserinfo(value);
    }

    get pathname() {
      let path = parts.get(this).path;
      if (specialSchemes.includes(parts.get(this).protocol)) {
        if (path.charAt(0) != "/") {
          path = `/${path}`;
        }
      }
      return path;
    }

    set pathname(value) {
      parts.get(this).path = encodePathname(String(value));
    }

    get port() {
      const port = parts.get(this).port;
      if (schemePorts[parts.get(this).protocol] === port) {
        return "";
      }

      return port;
    }

    set port(value) {
      if (!isValidPort(value)) {
        return;
      }
      parts.get(this).port = value.toString();
    }

    get protocol() {
      return `${parts.get(this).protocol}:`;
    }

    set protocol(value) {
      value = String(value);
      if (value) {
        if (value.charAt(value.length - 1) === ":") {
          value = value.slice(0, -1);
        }
        parts.get(this).protocol = encodeURIComponent(value);
      }
    }

    get search() {
      return parts.get(this).query;
    }

    set search(value) {
      value = String(value);
      const query = value == "" || value.charAt(0) == "?" ? value : `?${value}`;
      parts.get(this).query = encodeSearch(query);
      this.#updateSearchParams();
    }

    get username() {
      return parts.get(this).username;
    }

    set username(value) {
      value = String(value);
      parts.get(this).username = encodeUserinfo(value);
    }

    get searchParams() {
      return this.#searchParams;
    }

    constructor(url, base) {
      let baseParts;
      if (base) {
        baseParts = typeof base === "string" ? parse(base) : parts.get(base);
        if (baseParts === undefined) {
          throw new TypeError("Invalid base URL.");
        }
      }

      const urlParts = typeof url === "string"
        ? parse(url, !baseParts)
        : parts.get(url);
      if (urlParts == undefined) {
        throw new TypeError("Invalid URL.");
      }

      if (urlParts.protocol) {
        urlParts.path = normalizePath(
          urlParts.path,
          urlParts.protocol == "file",
        );
        parts.set(this, urlParts);
      } else if (baseParts) {
        parts.set(this, {
          protocol: baseParts.protocol,
          slashes: baseParts.slashes,
          username: baseParts.username,
          password: baseParts.password,
          hostname: baseParts.hostname,
          port: baseParts.port,
          path: resolvePathFromBase(
            urlParts.path,
            baseParts.path || "/",
            baseParts.protocol == "file",
          ),
          query: urlParts.query,
          hash: urlParts.hash,
        });
      } else {
        throw new TypeError("Invalid URL.");
      }

      this.#updateSearchParams();
    }

    toString() {
      return this.href;
    }

    toJSON() {
      return this.href;
    }

    // TODO(kevinkassimo): implement MediaSource version in the future.
    static createObjectURL(blob) {
      const origin = "http://deno-opaque-origin";
      const key = `blob:${origin}/${generateUUID()}`;
      blobURLMap.set(key, blob);
      return key;
    }

    static revokeObjectURL(url) {
      let urlObject;
      try {
        urlObject = new URL(url);
      } catch {
        throw new TypeError("Provided URL string is not valid");
      }
      if (urlObject.protocol !== "blob:") {
        return;
      }
      // Origin match check seems irrelevant for now, unless we implement
      // persisten storage for per globalThis.location.origin at some point.
      blobURLMap.delete(url);
    }
  }

  function parseIpv4Number(s) {
    if (s.match(/^(0[Xx])[0-9A-Za-z]+$/)) {
      return Number(s);
    }
    if (s.match(/^[0-9]+$/)) {
      return Number(s.startsWith("0") ? `0o${s}` : s);
    }
    return NaN;
  }

  function parseIpv4(s) {
    const parts = s.split(".");
    if (parts[parts.length - 1] == "" && parts.length > 1) {
      parts.pop();
    }
    if (parts.includes("") || parts.length > 4) {
      return s;
    }
    const numbers = parts.map(parseIpv4Number);
    if (numbers.includes(NaN)) {
      return s;
    }
    const last = numbers.pop();
    if (last >= 256 ** (4 - numbers.length) || numbers.find((n) => n >= 256)) {
      throw new TypeError("Invalid hostname.");
    }
    const ipv4 = numbers.reduce((sum, n, i) => sum + n * 256 ** (3 - i), last);
    const ipv4Hex = ipv4.toString(16).padStart(8, "0");
    const ipv4HexParts = ipv4Hex.match(/(..)(..)(..)(..)$/).slice(1);
    return ipv4HexParts.map((s) => String(Number(`0x${s}`))).join(".");
  }

  function charInC0ControlSet(c) {
    return (c >= "\u0000" && c <= "\u001F") || c > "\u007E";
  }

  function charInSearchSet(c) {
    // deno-fmt-ignore
    return charInC0ControlSet(c) || ["\u0020", "\u0022", "\u0023", "\u0027", "\u003C", "\u003E"].includes(c) || c > "\u007E";
  }

  function charInFragmentSet(c) {
    // deno-fmt-ignore
    return charInC0ControlSet(c) || ["\u0020", "\u0022", "\u003C", "\u003E", "\u0060"].includes(c);
  }

  function charInPathSet(c) {
    // deno-fmt-ignore
    return charInFragmentSet(c) || ["\u0023", "\u003F", "\u007B", "\u007D"].includes(c);
  }

  function charInUserinfoSet(c) {
    // "\u0027" ("'") seemingly isn't in the spec, but matches Chrome and Firefox.
    // deno-fmt-ignore
    return charInPathSet(c) || ["\u0027", "\u002F", "\u003A", "\u003B", "\u003D", "\u0040", "\u005B", "\u005C", "\u005D", "\u005E", "\u007C"].includes(c);
  }

  function charIsForbiddenInHost(c) {
    // deno-fmt-ignore
    return ["\u0000", "\u0009", "\u000A", "\u000D", "\u0020", "\u0023", "\u0025", "\u002F", "\u003A", "\u003C", "\u003E", "\u003F", "\u0040", "\u005B", "\u005C", "\u005D", "\u005E"].includes(c);
  }

  const encoder = new TextEncoder();

  function encodeChar(c) {
    return [...encoder.encode(c)]
      .map((n) => `%${n.toString(16)}`)
      .join("")
      .toUpperCase();
  }

  function encodeUserinfo(s) {
    return [...s].map((c) => (charInUserinfoSet(c) ? encodeChar(c) : c)).join(
      "",
    );
  }

  function encodeHostname(s, isSpecial = true) {
    // IPv6 parsing.
    if (s.startsWith("[") && s.endsWith("]")) {
      if (!s.match(/^\[[0-9A-Fa-f.:]{2,}\]$/)) {
        throw new TypeError("Invalid hostname.");
      }
      // IPv6 address compress
      return s.toLowerCase().replace(/\b:?(?:0+:?){2,}/, "::");
    }

    let result = s;

    if (!isSpecial) {
      // Check against forbidden host code points except for "%".
      for (const c of result) {
        if (charIsForbiddenInHost(c) && c != "\u0025") {
          throw new TypeError("Invalid hostname.");
        }
      }

      // Percent-encode C0 control set.
      result = [...result]
        .map((c) => (charInC0ControlSet(c) ? encodeChar(c) : c))
        .join("");

      return result;
    }

    // Percent-decode.
    if (result.match(/%(?![0-9A-Fa-f]{2})/) != null) {
      throw new TypeError("Invalid hostname.");
    }
    result = result.replace(
      /%(.{2})/g,
      (_, hex) => String.fromCodePoint(Number(`0x${hex}`)),
    );

    // IDNA domain to ASCII.
    result = domainToAscii(result);

    // Check against forbidden host code points.
    for (const c of result) {
      if (charIsForbiddenInHost(c)) {
        throw new TypeError("Invalid hostname.");
      }
    }

    // IPv4 parsing.
    if (isSpecial) {
      result = parseIpv4(result);
    }

    return result;
  }

  function encodePathname(s) {
    return [...s].map((c) => (charInPathSet(c) ? encodeChar(c) : c)).join("");
  }

  function encodeSearch(s) {
    return [...s].map((c) => (charInSearchSet(c) ? encodeChar(c) : c)).join("");
  }

  function encodeHash(s) {
    return [...s].map((c) => (charInFragmentSet(c) ? encodeChar(c) : c)).join(
      "",
    );
  }

  window.__bootstrap.url = {
    URL,
    URLSearchParams,
    blobURLMap,
  };
})(this);
