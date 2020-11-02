// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

// This module is the entry point for "compiler" isolate, ie. the one
// that is created when Deno needs to compile TS/WASM to JS.
//
// It provides two functions that should be called by Rust:
//  - `startup`
// This functions must be called when creating isolate
// to properly setup runtime.
//  - `tsCompilerOnMessage`
// This function must be called when sending a request
// to the compiler.

// Removes the `__proto__` for security reasons.  This intentionally makes
// Deno non compliant with ECMA-262 Annex B.2.2.1
//
delete Object.prototype.__proto__;

((window) => {
  const core = window.Deno.core;

  let logDebug = false;
  let logSource = "JS";

  /** Instructs the host to behave in a legacy fashion, with the legacy
   * pipeline for handling code.  Setting the value to `true` will cause the
   * host to behave in the modern way. */
  let legacy = true;

  function setLogDebug(debug, source) {
    logDebug = debug;
    if (source) {
      logSource = source;
    }
  }

  function debug(...args) {
    if (logDebug) {
      const stringifiedArgs = args.map((arg) => JSON.stringify(arg)).join(" ");
      core.print(`DEBUG ${logSource} - ${stringifiedArgs}\n`);
    }
  }

  class AssertionError extends Error {
    constructor(msg) {
      super(msg);
      this.name = "AssertionError";
    }
  }

  function assert(cond, msg = "Assertion failed.") {
    if (!cond) {
      throw new AssertionError(msg);
    }
  }

  /** @type {Map<string, ts.SourceFile>} */
  const sourceFileCache = new Map();

  /**
   * @param {import("../dts/typescript").DiagnosticRelatedInformation} diagnostic
   */
  function fromRelatedInformation({
    start,
    length,
    file,
    messageText: msgText,
    ...ri
  }) {
    let messageText;
    let messageChain;
    if (typeof msgText === "object") {
      messageChain = msgText;
    } else {
      messageText = msgText;
    }
    if (start !== undefined && length !== undefined && file) {
      const startPos = file.getLineAndCharacterOfPosition(start);
      const sourceLine = file.getFullText().split("\n")[startPos.line];
      const fileName = file.fileName;
      return {
        start: startPos,
        end: file.getLineAndCharacterOfPosition(start + length),
        fileName,
        messageChain,
        messageText,
        sourceLine,
        ...ri,
      };
    } else {
      return {
        messageChain,
        messageText,
        ...ri,
      };
    }
  }

  /**
   * @param {import("../dts/typescript").Diagnostic[]} diagnostics 
   */
  function fromTypeScriptDiagnostic(diagnostics) {
    return diagnostics.map(({ relatedInformation: ri, source, ...diag }) => {
      const value = fromRelatedInformation(diag);
      value.relatedInformation = ri
        ? ri.map(fromRelatedInformation)
        : undefined;
      value.source = source;
      return value;
    });
  }

  // We really don't want to depend on JSON dispatch during snapshotting, so
  // this op exchanges strings with Rust as raw byte arrays.
  function getAsset(name) {
    const opId = core.ops()["op_fetch_asset"];
    const sourceCodeBytes = core.dispatch(opId, core.encode(name));
    return core.decode(sourceCodeBytes);
  }

  // Using incremental compile APIs requires that all
  // paths must be either relative or absolute. Since
  // analysis in Rust operates on fully resolved URLs,
  // it makes sense to use the same scheme here.
  const ASSETS = "asset:///";
  const OUT_DIR = "deno://";
  const CACHE = "cache:///";
  // This constant is passed to compiler settings when
  // doing incremental compiles. Contents of this
  // file are passed back to Rust and saved to $DENO_DIR.
  const TS_BUILD_INFO = "cache:///tsbuildinfo.json";

  const DEFAULT_COMPILE_OPTIONS = {
    allowJs: false,
    allowNonTsExtensions: true,
    checkJs: false,
    esModuleInterop: true,
    jsx: ts.JsxEmit.React,
    module: ts.ModuleKind.ESNext,
    outDir: OUT_DIR,
    sourceMap: true,
    strict: true,
    removeComments: true,
    target: ts.ScriptTarget.ESNext,
  };

  const CompilerHostTarget = {
    Main: "main",
    Runtime: "runtime",
    Worker: "worker",
  };

  // Warning! The values in this enum are duplicated in `cli/msg.rs`
  // Update carefully!
  const MediaType = {
    0: "JavaScript",
    1: "JSX",
    2: "TypeScript",
    3: "Dts",
    4: "TSX",
    5: "Json",
    6: "Wasm",
    7: "TsBuildInfo",
    8: "SourceMap",
    9: "Unknown",
    JavaScript: 0,
    JSX: 1,
    TypeScript: 2,
    Dts: 3,
    TSX: 4,
    Json: 5,
    Wasm: 6,
    TsBuildInfo: 7,
    SourceMap: 8,
    Unknown: 9,
  };

  function getExtension(fileName, mediaType) {
    switch (mediaType) {
      case MediaType.JavaScript:
        return ts.Extension.Js;
      case MediaType.JSX:
        return ts.Extension.Jsx;
      case MediaType.TypeScript:
        return ts.Extension.Ts;
      case MediaType.Dts:
        return ts.Extension.Dts;
      case MediaType.TSX:
        return ts.Extension.Tsx;
      case MediaType.Wasm:
        // Custom marker for Wasm type.
        return ts.Extension.Js;
      case MediaType.Unknown:
      default:
        throw TypeError(
          `Cannot resolve extension for "${fileName}" with mediaType "${
            MediaType[mediaType]
          }".`,
        );
    }
  }

  /** A global cache of module source files that have been loaded.
   * This cache will be rewritten to be populated on compiler startup
   * with files provided from Rust in request message.
   */
  const SOURCE_FILE_CACHE = new Map();
  /** A map of maps which cache resolved specifier for each import in a file.
   * This cache is used so `resolveModuleNames` ops is called as few times
   * as possible.
   *
   * First map's key is "referrer" URL ("file://a/b/c/mod.ts")
   * Second map's key is "raw" import specifier ("./foo.ts")
   * Second map's value is resolved import URL ("file:///a/b/c/foo.ts")
   */
  const RESOLVED_SPECIFIER_CACHE = new Map();

  class SourceFile {
    constructor(json) {
      this.processed = false;
      Object.assign(this, json);
      this.extension = getExtension(this.url, this.mediaType);
    }

    static addToCache(json) {
      if (SOURCE_FILE_CACHE.has(json.url)) {
        throw new TypeError("SourceFile already exists");
      }
      const sf = new SourceFile(json);
      SOURCE_FILE_CACHE.set(sf.url, sf);
      return sf;
    }

    static getCached(url) {
      return SOURCE_FILE_CACHE.get(url);
    }

    static cacheResolvedUrl(resolvedUrl, rawModuleSpecifier, containingFile) {
      containingFile = containingFile || "";
      let innerCache = RESOLVED_SPECIFIER_CACHE.get(containingFile);
      if (!innerCache) {
        innerCache = new Map();
        RESOLVED_SPECIFIER_CACHE.set(containingFile, innerCache);
      }
      innerCache.set(rawModuleSpecifier, resolvedUrl);
    }

    static getResolvedUrl(moduleSpecifier, containingFile) {
      const containingCache = RESOLVED_SPECIFIER_CACHE.get(containingFile);
      if (containingCache) {
        return containingCache.get(moduleSpecifier);
      }
      return undefined;
    }
  }

  function getAssetInternal(filename) {
    const lastSegment = filename.split("/").pop();
    const url = ts.libMap.has(lastSegment)
      ? ts.libMap.get(lastSegment)
      : lastSegment;
    const sourceFile = SourceFile.getCached(url);
    if (sourceFile) {
      return sourceFile;
    }
    const name = url.includes(".") ? url : `${url}.d.ts`;
    const sourceCode = getAsset(name);
    return SourceFile.addToCache({
      url,
      filename: `${ASSETS}/${name}`,
      mediaType: MediaType.TypeScript,
      versionHash: "1",
      sourceCode,
    });
  }

  /** There was some private state in the legacy host, that is moved out to
   * here which can then be refactored out later. */
  const legacyHostState = {
    buildInfo: "",
    target: CompilerHostTarget.Main,
    writeFile: (_fileName, _data, _sourceFiles) => {},
  };

  /** @type {import("../dts/typescript").CompilerHost} */
  const host = {
    fileExists(fileName) {
      debug(`host.fileExists("${fileName}")`);
      return false;
    },
    readFile(specifier) {
      debug(`host.readFile("${specifier}")`);
      if (legacy) {
        if (specifier == TS_BUILD_INFO) {
          return legacyHostState.buildInfo;
        }
        return unreachable();
      } else {
        return core.jsonOpSync("op_load", { specifier }).data;
      }
    },
    getSourceFile(
      specifier,
      languageVersion,
      onError,
      shouldCreateNewSourceFile,
    ) {
      debug(
        `host.getSourceFile("${specifier}", ${
          ts.ScriptTarget[languageVersion]
        })`,
      );
      if (legacy) {
        try {
          assert(!shouldCreateNewSourceFile);
          const sourceFile = specifier.startsWith(ASSETS)
            ? getAssetInternal(specifier)
            : SourceFile.getCached(specifier);
          assert(sourceFile != null);
          if (!sourceFile.tsSourceFile) {
            assert(sourceFile.sourceCode != null);
            const tsSourceFileName = specifier.startsWith(ASSETS)
              ? sourceFile.filename
              : specifier;

            sourceFile.tsSourceFile = ts.createSourceFile(
              tsSourceFileName,
              sourceFile.sourceCode,
              languageVersion,
            );
            sourceFile.tsSourceFile.version = sourceFile.versionHash;
            delete sourceFile.sourceCode;

            // This code is to support transition from the "legacy" compiler
            // to the new one, by populating the new source file cache.
            if (
              !sourceFileCache.has(specifier) && specifier.startsWith(ASSETS)
            ) {
              sourceFileCache.set(specifier, sourceFile.tsSourceFile);
            }
          }
          return sourceFile.tsSourceFile;
        } catch (e) {
          if (onError) {
            onError(String(e));
          } else {
            throw e;
          }
          return undefined;
        }
      } else {
        let sourceFile = sourceFileCache.get(specifier);
        if (sourceFile) {
          return sourceFile;
        }

        /** @type {{ data: string; hash?: string; scriptKind: ts.ScriptKind }} */
        const { data, hash, scriptKind } = core.jsonOpSync(
          "op_load",
          { specifier },
        );
        assert(
          data != null,
          `"data" is unexpectedly null for "${specifier}".`,
        );
        sourceFile = ts.createSourceFile(
          specifier,
          data,
          languageVersion,
          false,
          scriptKind,
        );
        sourceFile.moduleName = specifier;
        sourceFile.version = hash;
        sourceFileCache.set(specifier, sourceFile);
        return sourceFile;
      }
    },
    getDefaultLibFileName() {
      if (legacy) {
        switch (legacyHostState.target) {
          case CompilerHostTarget.Main:
          case CompilerHostTarget.Runtime:
            return `${ASSETS}/lib.deno.window.d.ts`;
          case CompilerHostTarget.Worker:
            return `${ASSETS}/lib.deno.worker.d.ts`;
        }
      } else {
        return `${ASSETS}/lib.esnext.d.ts`;
      }
    },
    getDefaultLibLocation() {
      return ASSETS;
    },
    writeFile(fileName, data, _writeByteOrderMark, _onError, sourceFiles) {
      debug(`host.writeFile("${fileName}")`);
      if (legacy) {
        legacyHostState.writeFile(fileName, data, sourceFiles);
      } else {
        let maybeSpecifiers;
        if (sourceFiles) {
          maybeSpecifiers = sourceFiles.map((sf) => sf.moduleName);
        }
        return core.jsonOpSync(
          "op_emit",
          { maybeSpecifiers, fileName, data },
        );
      }
    },
    getCurrentDirectory() {
      return CACHE;
    },
    getCanonicalFileName(fileName) {
      return fileName;
    },
    useCaseSensitiveFileNames() {
      return true;
    },
    getNewLine() {
      return "\n";
    },
    resolveModuleNames(specifiers, base) {
      debug(`host.resolveModuleNames()`);
      debug(`  base: ${base}`);
      debug(`  specifiers: ${specifiers.join(", ")}`);
      if (legacy) {
        const resolved = specifiers.map((specifier) => {
          const maybeUrl = SourceFile.getResolvedUrl(specifier, base);

          debug("compiler::host.resolveModuleNames maybeUrl", {
            specifier,
            maybeUrl,
          });

          let sourceFile = undefined;

          if (specifier.startsWith(ASSETS)) {
            sourceFile = getAssetInternal(specifier);
          } else if (typeof maybeUrl !== "undefined") {
            sourceFile = SourceFile.getCached(maybeUrl);
          }

          if (!sourceFile) {
            return undefined;
          }

          return {
            resolvedFileName: sourceFile.url,
            isExternalLibraryImport: specifier.startsWith(ASSETS),
            extension: sourceFile.extension,
          };
        });
        debug(resolved);
        return resolved;
      } else {
        /** @type {Array<[string, import("../dts/typescript").Extension]>} */
        const resolved = core.jsonOpSync("op_resolve", {
          specifiers,
          base,
        });
        let r = resolved.map(([resolvedFileName, extension]) => ({
          resolvedFileName,
          extension,
          isExternalLibraryImport: false,
        }));
        return r;
      }
    },
    createHash(data) {
      return core.jsonOpSync("op_create_hash", { data }).hash;
    },
  };

  // This is a hacky way of adding our libs to the libs available in TypeScript()
  // as these are internal APIs of TypeScript which maintain valid libs
  ts.libs.push("deno.ns", "deno.window", "deno.worker", "deno.shared_globals");
  ts.libMap.set("deno.ns", "lib.deno.ns.d.ts");
  ts.libMap.set("deno.web", "lib.deno.web.d.ts");
  ts.libMap.set("deno.fetch", "lib.deno.fetch.d.ts");
  ts.libMap.set("deno.window", "lib.deno.window.d.ts");
  ts.libMap.set("deno.worker", "lib.deno.worker.d.ts");
  ts.libMap.set("deno.shared_globals", "lib.deno.shared_globals.d.ts");
  ts.libMap.set("deno.unstable", "lib.deno.unstable.d.ts");

  // TODO(@kitsonk) remove once added to TypeScript
  ts.libs.push("esnext.weakref");
  ts.libMap.set("esnext.weakref", "lib.esnext.weakref.d.ts");

  // this pre-populates the cache at snapshot time of our library files, so they
  // are available in the future when needed.
  host.getSourceFile(
    `${ASSETS}lib.deno.ns.d.ts`,
    ts.ScriptTarget.ESNext,
  );
  host.getSourceFile(
    `${ASSETS}lib.deno.web.d.ts`,
    ts.ScriptTarget.ESNext,
  );
  host.getSourceFile(
    `${ASSETS}lib.deno.fetch.d.ts`,
    ts.ScriptTarget.ESNext,
  );
  host.getSourceFile(
    `${ASSETS}lib.deno.window.d.ts`,
    ts.ScriptTarget.ESNext,
  );
  host.getSourceFile(
    `${ASSETS}lib.deno.worker.d.ts`,
    ts.ScriptTarget.ESNext,
  );
  host.getSourceFile(
    `${ASSETS}lib.deno.shared_globals.d.ts`,
    ts.ScriptTarget.ESNext,
  );
  host.getSourceFile(
    `${ASSETS}lib.deno.unstable.d.ts`,
    ts.ScriptTarget.ESNext,
  );

  // We never use this program; it's only created
  // during snapshotting to hydrate and populate
  // source file cache with lib declaration files.
  const _TS_SNAPSHOT_PROGRAM = ts.createProgram({
    rootNames: [`${ASSETS}bootstrap.ts`],
    options: DEFAULT_COMPILE_OPTIONS,
    host,
  });

  const IGNORED_DIAGNOSTICS = [
    // TS2306: File 'file:///Users/rld/src/deno/cli/tests/subdir/amd_like.js' is
    // not a module.
    2306,
    // TS1375: 'await' expressions are only allowed at the top level of a file
    // when that file is a module, but this file has no imports or exports.
    // Consider adding an empty 'export {}' to make this file a module.
    1375,
    // TS1103: 'for-await-of' statement is only allowed within an async function
    // or async generator.
    1103,
    // TS2691: An import path cannot end with a '.ts' extension. Consider
    // importing 'bad-module' instead.
    2691,
    // TS5009: Cannot find the common subdirectory path for the input files.
    5009,
    // TS5055: Cannot write file
    // 'http://localhost:4545/cli/tests/subdir/mt_application_x_javascript.j4.js'
    // because it would overwrite input file.
    5055,
    // TypeScript is overly opinionated that only CommonJS modules kinds can
    // support JSON imports.  Allegedly this was fixed in
    // Microsoft/TypeScript#26825 but that doesn't seem to be working here,
    // so we will ignore complaints about this compiler setting.
    5070,
    // TS7016: Could not find a declaration file for module '...'. '...'
    // implicitly has an 'any' type.  This is due to `allowJs` being off by
    // default but importing of a JavaScript module.
    7016,
  ];

  const IGNORED_COMPILE_DIAGNOSTICS = [
    // TS1208: All files must be modules when the '--isolatedModules' flag is
    // provided.  We can ignore because we guarantuee that all files are
    // modules.
    1208,
  ];

  /** @type {Array<{ key: string, value: number }>} */
  const stats = [];
  let statsStart = 0;

  function performanceStart() {
    stats.length = 0;
    statsStart = new Date();
    ts.performance.enable();
  }

  function performanceProgram({ program, fileCount }) {
    if (program) {
      if ("getProgram" in program) {
        program = program.getProgram();
      }
      stats.push({ key: "Files", value: program.getSourceFiles().length });
      stats.push({ key: "Nodes", value: program.getNodeCount() });
      stats.push({ key: "Identifiers", value: program.getIdentifierCount() });
      stats.push({ key: "Symbols", value: program.getSymbolCount() });
      stats.push({ key: "Types", value: program.getTypeCount() });
      stats.push({
        key: "Instantiations",
        value: program.getInstantiationCount(),
      });
    } else if (fileCount != null) {
      stats.push({ key: "Files", value: fileCount });
    }
    const programTime = ts.performance.getDuration("Program");
    const bindTime = ts.performance.getDuration("Bind");
    const checkTime = ts.performance.getDuration("Check");
    const emitTime = ts.performance.getDuration("Emit");
    stats.push({ key: "Parse time", value: programTime });
    stats.push({ key: "Bind time", value: bindTime });
    stats.push({ key: "Check time", value: checkTime });
    stats.push({ key: "Emit time", value: emitTime });
    stats.push({
      key: "Total TS time",
      value: programTime + bindTime + checkTime + emitTime,
    });
  }

  function performanceEnd() {
    const duration = new Date() - statsStart;
    stats.push({ key: "Compile time", value: duration });
    return stats;
  }

  /**
   * @typedef {object} Request
   * @property {Record<string, any>} config
   * @property {boolean} debug
   * @property {string[]} rootNames
   */

  /** The API that is called by Rust when executing a request.
   * @param {Request} request 
   */
  function exec({ config, debug: debugFlag, rootNames }) {
    setLogDebug(debugFlag, "TS");
    performanceStart();
    debug(">>> exec start", { rootNames });
    debug(config);

    const { options, errors: configFileParsingDiagnostics } = ts
      .convertCompilerOptionsFromJson(config, "", "tsconfig.json");
    const program = ts.createIncrementalProgram({
      rootNames,
      options,
      host,
      configFileParsingDiagnostics,
    });

    const { diagnostics: emitDiagnostics } = program.emit();

    const diagnostics = [
      ...program.getConfigFileParsingDiagnostics(),
      ...program.getSyntacticDiagnostics(),
      ...program.getOptionsDiagnostics(),
      ...program.getGlobalDiagnostics(),
      ...program.getSemanticDiagnostics(),
      ...emitDiagnostics,
    ].filter(({ code }) =>
      !IGNORED_DIAGNOSTICS.includes(code) &&
      !IGNORED_COMPILE_DIAGNOSTICS.includes(code)
    );
    performanceProgram({ program });

    // TODO(@kitsonk) when legacy stats are removed, convert to just tuples
    let stats = performanceEnd().map(({ key, value }) => [key, value]);
    core.jsonOpSync("op_respond", {
      diagnostics: fromTypeScriptDiagnostic(diagnostics),
      stats,
    });
    debug("<<< exec stop");
  }

  let hasStarted = false;

  /** Startup the runtime environment, setting various flags.
   * @param {{ debugFlag?: boolean; legacyFlag?: boolean; }} msg 
   */
  function startup({ debugFlag = false, legacyFlag = true }) {
    if (hasStarted) {
      throw new Error("The compiler runtime already started.");
    }
    hasStarted = true;
    core.ops();
    core.registerErrorClass("Error", Error);
    setLogDebug(!!debugFlag, "TS");
    legacy = legacyFlag;
  }

  globalThis.startup = startup;
  globalThis.exec = exec;
})(this);
