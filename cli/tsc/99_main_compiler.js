// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

// deno-lint-ignore-file no-undef

// This module is the entry point for "compiler" isolate, ie. the one
// that is created when Deno needs to type check TypeScript, and in some
// instances convert TypeScript to JavaScript.

// Removes the `__proto__` for security reasons.  This intentionally makes
// Deno non compliant with ECMA-262 Annex B.2.2.1
delete Object.prototype.__proto__;

((window) => {
  const core = window.Deno.core;

  let logDebug = false;
  let logSource = "JS";

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

  /** @param {ts.DiagnosticRelatedInformation} diagnostic */
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

  /** @param {ts.Diagnostic[]} diagnostics */
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

  // Using incremental compile APIs requires that all
  // paths must be either relative or absolute. Since
  // analysis in Rust operates on fully resolved URLs,
  // it makes sense to use the same scheme here.
  const ASSETS = "asset:///";
  const CACHE = "cache:///";

  /** Diagnostics that are intentionally ignored when compiling TypeScript in
   * Deno, as they provide misleading or incorrect information. */
  const IGNORED_DIAGNOSTICS = [
    // TS1208: All files must be modules when the '--isolatedModules' flag is
    // provided.  We can ignore because we guarantuee that all files are
    // modules.
    1208,
    // TS1375: 'await' expressions are only allowed at the top level of a file
    // when that file is a module, but this file has no imports or exports.
    // Consider adding an empty 'export {}' to make this file a module.
    1375,
    // TS1103: 'for-await-of' statement is only allowed within an async function
    // or async generator.
    1103,
    // TS2306: File 'file:///Users/rld/src/deno/cli/tests/subdir/amd_like.js' is
    // not a module.
    2306,
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

  const SNAPSHOT_COMPILE_OPTIONS = {
    esModuleInterop: true,
    jsx: ts.JsxEmit.React,
    module: ts.ModuleKind.ESNext,
    noEmit: true,
    strict: true,
    target: ts.ScriptTarget.ESNext,
  };

  /** An object literal of the incremental compiler host, which provides the
   * specific "bindings" to the Deno environment that tsc needs to work.
   *
   * @type {ts.CompilerHost} */
  const host = {
    fileExists(fileName) {
      debug(`host.fileExists("${fileName}")`);
      return false;
    },
    readFile(specifier) {
      debug(`host.readFile("${specifier}")`);
      return core.jsonOpSync("op_load", { specifier }).data;
    },
    getSourceFile(
      specifier,
      languageVersion,
      _onError,
      _shouldCreateNewSourceFile,
    ) {
      debug(
        `host.getSourceFile("${specifier}", ${
          ts.ScriptTarget[languageVersion]
        })`,
      );
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
    },
    getDefaultLibFileName() {
      return `${ASSETS}/lib.esnext.d.ts`;
    },
    getDefaultLibLocation() {
      return ASSETS;
    },
    writeFile(fileName, data, _writeByteOrderMark, _onError, sourceFiles) {
      debug(`host.writeFile("${fileName}")`);
      let maybeSpecifiers;
      if (sourceFiles) {
        maybeSpecifiers = sourceFiles.map((sf) => sf.moduleName);
      }
      return core.jsonOpSync(
        "op_emit",
        { maybeSpecifiers, fileName, data },
      );
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
      /** @type {Array<[string, ts.Extension]>} */
      const resolved = core.jsonOpSync("op_resolve", {
        specifiers,
        base,
      });
      const r = resolved.map(([resolvedFileName, extension]) => ({
        resolvedFileName,
        extension,
        isExternalLibraryImport: false,
      }));
      return r;
    },
    createHash(data) {
      return core.jsonOpSync("op_create_hash", { data }).hash;
    },
  };

  /** @type {Array<[string, number]>} */
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
      stats.push(["Files", program.getSourceFiles().length]);
      stats.push(["Nodes", program.getNodeCount()]);
      stats.push(["Identifiers", program.getIdentifierCount()]);
      stats.push(["Symbols", program.getSymbolCount()]);
      stats.push(["Types", program.getTypeCount()]);
      stats.push(["Instantiations", program.getInstantiationCount()]);
    } else if (fileCount != null) {
      stats.push(["Files", fileCount]);
    }
    const programTime = ts.performance.getDuration("Program");
    const bindTime = ts.performance.getDuration("Bind");
    const checkTime = ts.performance.getDuration("Check");
    const emitTime = ts.performance.getDuration("Emit");
    stats.push(["Parse time", programTime]);
    stats.push(["Bind time", bindTime]);
    stats.push(["Check time", checkTime]);
    stats.push(["Emit time", emitTime]);
    stats.push(
      ["Total TS time", programTime + bindTime + checkTime + emitTime],
    );
  }

  function performanceEnd() {
    const duration = new Date() - statsStart;
    stats.push(["Compile time", duration]);
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
    // The `allowNonTsExtensions` is a "hidden" compiler option used in VSCode
    // which is not allowed to be passed in JSON, we need it to allow special
    // URLs which Deno supports. So we need to either ignore the diagnostic, or
    // inject it ourselves.
    Object.assign(options, { allowNonTsExtensions: true });
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
    ].filter(({ code }) => !IGNORED_DIAGNOSTICS.includes(code));
    performanceProgram({ program });

    core.jsonOpSync("op_respond", {
      diagnostics: fromTypeScriptDiagnostic(diagnostics),
      stats: performanceEnd(),
    });
    debug("<<< exec stop");
  }

  let hasStarted = false;

  /** Startup the runtime environment, setting various flags.
   * @param {{ debugFlag?: boolean; legacyFlag?: boolean; }} msg
   */
  function startup({ debugFlag = false }) {
    if (hasStarted) {
      throw new Error("The compiler runtime already started.");
    }
    hasStarted = true;
    core.ops();
    core.registerErrorClass("Error", Error);
    setLogDebug(!!debugFlag, "TS");
  }

  // Setup the compiler runtime during the build process.
  core.ops();
  core.registerErrorClass("Error", Error);

  // A build time only op that provides some setup information that is used to
  // ensure the snapshot is setup properly.
  /** @type {{ buildSpecifier: string; libs: string[] }} */
  const { buildSpecifier, libs } = core.jsonOpSync("op_build_info", {});
  for (const lib of libs) {
    const specifier = `lib.${lib}.d.ts`;
    // we are using internal APIs here to "inject" our custom libraries into
    // tsc, so things like `"lib": [ "deno.ns" ]` are supported.
    if (!ts.libs.includes(lib)) {
      ts.libs.push(lib);
      ts.libMap.set(lib, `lib.${lib}.d.ts`);
    }
    // we are caching in memory common type libraries that will be re-used by
    // tsc on when the snapshot is restored
    assert(
      host.getSourceFile(`${ASSETS}${specifier}`, ts.ScriptTarget.ESNext),
    );
  }
  // this helps ensure as much as possible is in memory that is re-usable
  // before the snapshotting is done, which helps unsure fast "startup" for
  // subsequent uses of tsc in Deno.
  const TS_SNAPSHOT_PROGRAM = ts.createProgram({
    rootNames: [buildSpecifier],
    options: SNAPSHOT_COMPILE_OPTIONS,
    host,
  });
  ts.getPreEmitDiagnostics(TS_SNAPSHOT_PROGRAM);

  // exposes the two functions that are called by `tsc::exec()` when type
  // checking TypeScript.
  globalThis.startup = startup;
  globalThis.exec = exec;
})(this);
