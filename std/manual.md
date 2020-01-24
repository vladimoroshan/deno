# Deno Manual

## Table of Contents

## Project Status / Disclaimer

**A word of caution: Deno is very much under development.**

We encourage brave early adopters, but expect bugs large and small. The API is
subject to change without notice.
[Bug reports](https://github.com/denoland/deno/issues) do help!

We are
[actively working towards 1.0](https://github.com/denoland/deno/issues/2473),
but there is no date guarantee.

## Introduction

Deno is a JavaScript/TypeScript runtime with secure defaults and a great
developer experience.

It's built on V8, Rust, and Tokio.

### Feature Highlights

- Secure by default. No file, network, or environment access (unless explicitly
  enabled).
- Supports TypeScript out of the box.
- Ships a single executable (`deno`).
- Has built in utilities like a dependency inspector (`deno info`) and a code
  formatter (`deno fmt`).
- Has
  [a set of reviewed (audited) standard modules](https://github.com/denoland/deno/tree/master/std)
  that are guaranteed to work with Deno.
- Scripts can be bundled into a single javascript file.

### Philosophy

Deno aims to be a productive and secure scripting environment for the modern
programmer.

Deno will always be distributed as a single executable. Given a URL to a Deno
program, it is runnable with nothing more than
[the 10 megabyte zipped executable](https://github.com/denoland/deno/releases).
Deno explicitly takes on the role of both runtime and package manager. It uses a
standard browser-compatible protocol for loading modules: URLs.

Among other things, Deno is a great replacement for utility scripts that may
have been historically written with bash or python.

### Goals

- Only ship a single executable (`deno`).
- Provide Secure Defaults
  - Unless specifically allowed, scripts can't access files, the environment, or
    the network.
- Browser compatible: The subset of Deno programs which are written completely
  in JavaScript and do not use the global `Deno` namespace (or feature test for
  it), ought to also be able to be run in a modern web browser without change.
- Provide built-in tooling like unit testing, code formatting, and linting to
  improve developer experience.
- Does not leak V8 concepts into user land.
- Be able to serve HTTP efficiently

### Comparison to Node.js

- Deno does not use `npm`
  - It uses modules referenced as URLs or file paths
- Deno does not use `package.json` in its module resolution algorithm.
- All async actions in Deno return a promise. Thus Deno provides different APIs
  than Node.
- Deno requires explicit permissions for file, network, and environment access.
- Deno always dies on uncaught errors.
- Uses "ES Modules" and does not support `require()`. Third party modules are
  imported via URLs:

  ```javascript
  import * as log from "https://deno.land/std/log/mod.ts";
  ```

### Other key behaviors

- Remote code is fetched and cached on first execution, and never updated until
  the code is run with the `--reload` flag. (So, this will still work on an
  airplane.)
- Modules/files loaded from remote URLs are intended to be immutable and
  cacheable.

## Built-in Deno Utilities / Commands

<!-- prettier-ignore-start -->
<!-- prettier incorrectly moves the coming soon links to new lines -->

- dependency inspector (`deno info`)
- code formatter (`deno fmt`)
- bundling (`deno bundle`)
- runtime type info (`deno types`)
- test runner (`deno test`)
- command-line debugger (`--debug`) [coming soon](https://github.com/denoland/deno/issues/1120)
- linter (`deno lint`) [coming soon](https://github.com/denoland/deno/issues/1880)

<!-- prettier-ignore-end -->

## Setup

Deno works on OSX, Linux, and Windows. Deno is a single binary executable. It
has no external dependencies.

### Download and Install

[deno_install](https://github.com/denoland/deno_install) provides convenience
scripts to download and install the binary.

Using Shell:

```shell
curl -fsSL https://deno.land/x/install/install.sh | sh
```

Using PowerShell:

```shell
iwr https://deno.land/x/install/install.ps1 -useb | iex
```

Using [Scoop](https://scoop.sh/) (windows):

```shell
scoop install deno
```

Using [Chocolatey](https://chocolatey.org/packages/deno) (windows):

```shell
choco install deno
```

Using [Homebrew](https://formulae.brew.sh/formula/deno) (mac):

```shell
brew install deno
```

Using [Cargo](https://crates.io/crates/deno):

```shell
cargo install deno
```

Deno binaries can also be installed manually, by downloading a tarball or zip
file at
[github.com/denoland/deno/releases](https://github.com/denoland/deno/releases).
These packages contain just a single executable file. You will have to set the
executable bit on Mac and Linux.

Once it's installed and in your `$PATH`, try it:

```shell
deno https://deno.land/std/examples/welcome.ts
```

### Build from Source

Follow the [build instruction for contributors](#development).

## API reference

### `deno types`

To get an exact reference of deno's runtime API, run the following in the
command line:

```shell
$ deno types
```

[This is what the output looks like.](https://github.com/denoland/deno/blob/master/cli/js/lib.deno_runtime.d.ts)

### Reference websites

[TypeScript Deno API](https://deno.land/typedoc/index.html).

If you are embedding deno in a Rust program, see
[Rust Deno API](https://docs.rs/deno).

The Deno crate is hosted on [crates.io](https://crates.io/crates/deno).

## Examples

### An implementation of the unix "cat" program

In this program each command-line argument is assumed to be a filename, the file
is opened, and printed to stdout.

```ts
for (let i = 0; i < Deno.args.length; i++) {
  let filename = Deno.args[i];
  let file = await Deno.open(filename);
  await Deno.copy(Deno.stdout, file);
  file.close();
}
```

The `copy()` function here actually makes no more than the necessary kernel ->
userspace -> kernel copies. That is, the same memory from which data is read
from the file, is written to stdout. This illustrates a general design goal for
I/O streams in Deno.

Try the program:

```shell
$ deno --allow-read https://deno.land/std/examples/cat.ts /etc/passwd
```

### TCP echo server

This is an example of a simple server which accepts connections on port 8080,
and returns to the client anything it sends.

```ts
const listener = Deno.listen({ port: 8080 });
console.log("listening on 0.0.0.0:8080");
for await (const conn of listener) {
  Deno.copy(conn, conn);
}
```

When this program is started, it throws PermissionDenied error.

```shell
$ deno https://deno.land/std/examples/echo_server.ts
error: Uncaught PermissionDenied: run again with the --allow-net flag
► $deno$/dispatch_json.ts:40:11
    at DenoError ($deno$/errors.ts:20:5)
    ...
```

For security reasons, Deno does not allow programs to access the network without
explicit permission. To allow accessing the network, use a command-line flag:

```shell
$ deno --allow-net https://deno.land/std/examples/echo_server.ts
```

To test it, try sending data to it with netcat:

```shell
$ nc localhost 8080
hello world
hello world
```

Like the `cat.ts` example, the `copy()` function here also does not make
unnecessary memory copies. It receives a packet from the kernel and sends back,
without further complexity.

### Inspecting and revoking permissions

Sometimes a program may want to revoke previously granted permissions. When a
program, at a later stage, needs those permissions, it will fail.

```ts
// lookup a permission
const status = await Deno.permissions.query({ name: "write" });
if (status.state !== "granted") {
  throw new Error("need write permission");
}

const log = await Deno.open("request.log", "a+");

// revoke some permissions
await Deno.permissions.revoke({ name: "read" });
await Deno.permissions.revoke({ name: "write" });

// use the log file
const encoder = new TextEncoder();
await log.write(encoder.encode("hello\n"));

// this will fail.
await Deno.remove("request.log");
```

### File server

This one serves a local directory in HTTP.

```bash
deno install file_server https://deno.land/std/http/file_server.ts --allow-net --allow-read
```

Run it:

```shell
$ file_server .
Downloading https://deno.land/std/http/file_server.ts...
[...]
HTTP server listening on http://0.0.0.0:4500/
```

And if you ever want to upgrade to the latest published version:

```shell
$ file_server --reload
```

### Reload specific modules

Sometimes we want to upgrade only some modules. You can control it by passing an
argument to a `--reload` flag.

To reload everything

`--reload`

To reload all standard modules

`--reload=https://deno.land/std`

To reload specific modules (in this example - colors and file system utils) use
a comma to separate URLs

`--reload=https://deno.land/std/fs/utils.ts,https://deno.land/std/fmt/colors.ts`

### Permissions whitelist

Deno also provides permissions whitelist.

This is an example to restrict file system access by whitelist.

```shell
$ deno --allow-read=/usr https://deno.land/std/examples/cat.ts /etc/passwd
error: Uncaught PermissionDenied: run again with the --allow-read flag
► $deno$/dispatch_json.ts:40:11
    at DenoError ($deno$/errors.ts:20:5)
    ...
```

You can grant read permission under `/etc` dir

```shell
$ deno --allow-read=/etc https://deno.land/std/examples/cat.ts /etc/passwd
```

`--allow-write` works same as `--allow-read`.

This is an example to restrict host.

```ts
const result = await fetch("https://deno.land/");
```

```shell
$ deno --allow-net=deno.land https://deno.land/std/examples/curl.ts https://deno.land/
```

### Run subprocess

[API Reference](https://deno.land/typedoc/index.html#run)

Example:

```ts
// create subprocess
const p = Deno.run({
  args: ["echo", "hello"]
});

// await its completion
await p.status();
```

Run it:

```shell
$ deno --allow-run ./subprocess_simple.ts
hello
```

Here a function is assigned to `window.onload`. This function is called after
the main script is loaded. This is the same as
[onload](https://developer.mozilla.org/en-US/docs/Web/API/GlobalEventHandlers/onload)
of the browsers, and it can be used as the main entrypoint.

By default when you use `Deno.run()` subprocess inherits `stdin`, `stdout` and
`stderr` of parent process. If you want to communicate with started subprocess
you can use `"piped"` option.

```ts
const fileNames = Deno.args;

const p = Deno.run({
  args: [
    "deno",
    "run",
    "--allow-read",
    "https://deno.land/std/examples/cat.ts",
    ...fileNames
  ],
  stdout: "piped",
  stderr: "piped"
});

const { code } = await p.status();

if (code === 0) {
  const rawOutput = await p.output();
  await Deno.stdout.write(rawOutput);
} else {
  const rawError = await p.stderrOutput();
  const errorString = new TextDecoder().decode(rawError);
  console.log(errorString);
}

Deno.exit(code);
```

When you run it:

```shell
$ deno run --allow-run ./subprocess.ts <somefile>
[file content]

$ deno run --allow-run ./subprocess.ts non_existent_file.md

Uncaught NotFound: No such file or directory (os error 2)
    at DenoError (deno/js/errors.ts:22:5)
    at maybeError (deno/js/errors.ts:41:12)
    at handleAsyncMsgFromRust (deno/js/dispatch.ts:27:17)
```

### Handle OS Signals

[API Reference](https://deno.land/typedoc/index.html#signal)

You can use `Deno.signal()` function for handling OS signals.

```
for await (const _ of Deno.signal(Deno.Signal.SIGINT)) {
  console.log("interrupted!");
}
```

`Deno.signal()` also works as a promise.

```
await Deno.signal(Deno.Singal.SIGINT);
console.log("interrupted!");
```

If you want to stop watching the signal, you can use `dispose()` method of the
signal object.

```
const sig = Deno.signal(Deno.Signal.SIGINT);
setTimeout(() => { sig.dispose(); }, 5000);

for await (const _ of sig) {
  console.log("interrupted");
}
```

The above for-await loop exits after 5 seconds when sig.dispose() is called.

### Linking to third party code

In the above examples, we saw that Deno could execute scripts from URLs. Like
browser JavaScript, Deno can import libraries directly from URLs. This example
uses a URL to import a test runner library:

```ts
import {
  assertEquals,
  runIfMain,
  test
} from "https://deno.land/std/testing/mod.ts";

test(function t1() {
  assertEquals("hello", "hello");
});

test(function t2() {
  assertEquals("world", "world");
});

runIfMain(import.meta);
```

Try running this:

```shell
$ deno run test.ts
running 2 tests
test t1 ... ok
test t2 ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

```

Note that we did not have to provide the `--allow-net` flag for this program,
and yet it accessed the network. The runtime has special access to download
imports and cache them to disk.

Deno caches remote imports in a special directory specified by the `$DENO_DIR`
environmental variable. It defaults to the system's cache directory if
`$DENO_DIR` is not specified. The next time you run the program, no downloads
will be made. If the program hasn't changed, it won't be recompiled either. The
default directory is:

- On Linux/Redox: `$XDG_CACHE_HOME/deno` or `$HOME/.cache/deno`
- On Windows: `%LOCALAPPDATA%/deno` (`%LOCALAPPDATA%` = `FOLDERID_LocalAppData`)
- On macOS: `$HOME/Library/Caches/deno`
- If something fails, it falls back to `$HOME/.deno`

**But what if `https://deno.land/` goes down?** Relying on external servers is
convenient for development but brittle in production. Production software should
always bundle its dependencies. In Deno this is done by checking the `$DENO_DIR`
into your source control system, and specifying that path as the `$DENO_DIR`
environmental variable at runtime.

**How can I trust a URL that may change** By using a lock file (using the
`--lock` command line flag) you can ensure you're running the code you expect to
be.

**How do you import to a specific version?** Simply specify the version in the
URL. For example, this URL fully specifies the code being run:
`https://unpkg.com/liltest@0.0.5/dist/liltest.js`. Combined with the
aforementioned technique of setting `$DENO_DIR` in production to stored code,
one can fully specify the exact code being run, and execute the code without
network access.

**It seems unwieldy to import URLs everywhere. What if one of the URLs links to
a subtly different version of a library? Isn't it error prone to maintain URLs
everywhere in a large project?** The solution is to import and re-export your
external libraries in a central `deps.ts` file (which serves the same purpose as
Node's `package.json` file). For example, let's say you were using the above
testing library across a large project. Rather than importing
`"https://deno.land/std/testing/mod.ts"` everywhere, you could create a
`deps.ts` file that exports the third-party code:

```ts
export {
  assertEquals,
  runTests,
  test
} from "https://deno.land/std/testing/mod.ts";
```

And throughout the same project, you can import from the `deps.ts` and avoid
having many references to the same URL:

```ts
import { assertEquals, runTests, test } from "./deps.ts";
```

This design circumvents a plethora of complexity spawned by package management
software, centralized code repositories, and superfluous file formats.

### Using external type definitions

Deno supports both JavaScript and TypeScript as first class languages at
runtime. This means it requires fully qualified module names, including the
extension (or a server providing the correct media type). In addition, Deno has
no "magical" module resolution.

The out of the box TypeScript compiler though relies on both extension-less
modules and the Node.js module resolution logic to apply types to JavaScript
modules.

In order to bridge this gap, Deno supports compiler hints that inform Deno the
location of `.d.ts` files and the JavaScript code they relate to. A compiler
hint looks like this:

```ts
// @deno-types="./foo.d.ts"
import * as foo from "./foo.js";
```

Where the hint affects the next `import` statement (or `export ... from`
statement) where the value of the `@deno-types` will be substituted at compile
time instead of the specified module. Like in the above example, the Deno
compiler will load `./foo.d.ts` instead of `./foo.js`. Deno will still load
`./foo.js` when it runs the program.

**Not all type definitions are supported.**

Deno will use the compiler hint to load the indicated `.d.ts` files, but some
`.d.ts` files contain unsupported features. Specifically, some `.d.ts` files
expect to be able to load or reference type definitions from other packages
using the module resolution logic. For example a type reference directive to
include `node`, expecting to resolve to some path like
`./node_modules/@types/node/index.d.ts`. Since this depends on non-relative
"magical" resolution, Deno cannot resolve this.

**Why not use the triple-slash type reference?**

The TypeScript compiler supports triple-slash directives, including a type
reference directive. If Deno used this, it would interfere with the behavior of
the TypeScript compiler.

### Testing if current file is the main program

To test if the current script has been executed as the main input to the program
check `import.meta.main`.

```ts
if (import.meta.main) {
  console.log("main");
}
```

## Command line interface

### Flags

Use `deno help` to see the help text.

```
A secure JavaScript and TypeScript runtime

Docs: https://deno.land/std/manual.md
Modules: https://deno.land/x/
Bugs: https://github.com/denoland/deno/issues

To run the REPL supply no arguments:

  deno

To evaluate code from the command line:

  deno eval "console.log(30933 + 404)"

To execute a script:

  deno https://deno.land/std/examples/welcome.ts

The default subcommand is 'run'. The above is equivalent to

  deno run https://deno.land/std/examples/welcome.ts

See 'deno help run' for run specific flags.

USAGE:
    deno [SUBCOMMAND]

OPTIONS:
    -h, --help                     Prints help information
    -L, --log-level <log-level>    Set log level [possible values: debug, info]
    -V, --version                  Prints version information

SUBCOMMANDS:
    bundle         Bundle module and dependencies into single file
    completions    Generate shell completions
    eval           Eval script
    fetch          Fetch the dependencies
    fmt            Format files
    help           Prints this message or the help of the given subcommand(s)
    info           Show info about cache or info related to source file
    install        Install script as executable
    repl           Read Eval Print Loop
    run            Run a program given a filename or url to the source code
    test           Run tests
    types          Print runtime TypeScript declarations

ENVIRONMENT VARIABLES:
    DENO_DIR       Set deno's base directory
    NO_COLOR       Set to disable color
    HTTP_PROXY     Proxy address for HTTP requests (module downloads, fetch)
    HTTPS_PROXY    Same but for HTTPS
```

### Environmental variables

There are several env vars that control how Deno behaves:

`DENO_DIR` defaults to `$HOME/.deno` but can be set to any path to control where
generated and cached source code is written and read to.

`NO_COLOR` will turn off color output if set. See https://no-color.org/. User
code can test if `NO_COLOR` was set without having `--allow-env` by using the
boolean constant `Deno.noColor`.

### Shell completion

You can generate completion script for your shell using the
`deno completions <shell>` command. The command outputs to stdout so you should
redirect it to an appropriate file.

The supported shells are:

- zsh
- bash
- fish
- powershell
- elvish

Example:

```shell
deno completions bash > /usr/local/etc/bash_completion.d/deno.bash
source /usr/local/etc/bash_completion.d/deno.bash
```

### V8 flags

V8 has many many internal command-line flags.

```shell
# list available v8 flags
$ deno --v8-flags=--help

#  example for applying multiple flags
$ deno --v8-flags=--expose-gc,--use-strict
```

Particularly useful ones:

```
--async-stack-trace
```

### Bundling

`deno bundle [URL]` will output a single JavaScript file, which includes all
dependencies of the specified input. For example:

```
> deno bundle https://deno.land/std/examples/colors.ts colors.bundle.js
Bundling "colors.bundle.js"
Emitting bundle to "colors.bundle.js"
9.2 kB emitted.
```

If you omit the out file, the bundle will be sent to `stdout`.

The bundle can just be run as any other module in Deno would:

```
deno colors.bundle.js
```

The output is a self contained ES Module, which any exports from the main module
supplied on the command line will be available. For example if the main module
looked something like this:

```ts
export { foo } from "./foo.js";

export const bar = "bar";
```

It could be imported like this:

```ts
import { foo, bar } from "./lib.bundle.js";
```

Bundles can also be loaded in the web browser. The bundle is a self-contained ES
module, and so the attribute of `type` must be set to `"module"`. For example:

```html
<script type="module" src="website.bundle.js"></script>
```

Or you could import it into another ES module to consume:

```html
<script type="module">
  import * as website from "website.bundle.js";
</script>
```

### Installing executable scripts

Deno provides ability to easily install and distribute executable code via
`deno install` command.

`deno install [EXE_NAME] [URL] [FLAGS...]` will install script available at
`URL` with name `EXE_NAME`.

This command is a thin wrapper that creates executable shell scripts which
invoke `deno` with specified permissions and CLI flags.

Example:

```shell
$ deno install file_server https://deno.land/std/http/file_server.ts --allow-net --allow-read
[1/1] Compiling https://deno.land/std/http/file_server.ts

✅ Successfully installed file_server.
/Users/deno/.deno/bin/file_server
```

By default scripts are installed at `$HOME/.deno/bin` and that directory must be
added to the path manually.

```shell
$ echo 'export PATH="$HOME/.deno/bin:$PATH"' >> ~/.bashrc
```

Installation directory can be changed using `-d/--dir` flag:

```shell
$ deno install --dir /usr/local/bin prettier https://deno.land/std/prettier/main.ts --allow-write --allow-read
```

When installing a script you can specify permissions that will be used to run
the script. They are placed after the script URL and can be mixed with any
additional CLI flags you want to pass to the script.

Example:

```shell
$ deno install format_check https://deno.land/std/prettier/main.ts --allow-write --allow-read --check --print-width 88 --tab-width 2
```

Above command creates an executable called `format_check` that runs `prettier`
with write and read permissions. When you run `format_check` deno will run
prettier in `check` mode and configured to use `88` column width with `2` column
tabs.

It is a good practice to use `import.meta.main` idiom for an entry point for
executable file. See
[Testing if current file is the main program](#testing-if-current-file-is-the-main-program)
section.

Example:

```ts
// https://example.com/awesome/cli.ts
async function myAwesomeCli(): Promise<void> {
  -- snip --
}

if (import.meta.main) {
  myAwesomeCli();
}
```

When you create executable script make sure to let users know by adding example
installation command to your repository:

```shell
# Install using deno install

$ deno install awesome_cli https://example.com/awesome/cli.ts
```

## Proxies

Deno supports proxies for module downloads and `fetch` API.

Proxy configuration is read from environmental variables: `HTTP_PROXY` and
`HTTPS_PROXY`.

In case of Windows if environmental variables are not found Deno falls back to
reading proxies from registry.

## Lock file

Deno can store and check module subresource integrity for modules using a small
JSON file. Use the `--lock=lock.json` to enable and specify lock file checking.
To update or create a lock use `--lock=lock.json --lock-write`.

## Import maps

Deno supports [import maps](https://github.com/WICG/import-maps).

One can use import map with `--importmap=<FILE>` CLI flag.

Current limitations:

- single import map
- no fallback URLs
- Deno does not support `std:` namespace
- Does supports only `file:`, `http:` and `https:` schemes

Example:

```js
// import_map.json

{
   "imports": {
      "http/": "https://deno.land/std/http/"
   }
}
```

```ts
// hello_server.ts

import { serve } from "http/server.ts";

const body = new TextEncoder().encode("Hello World\n");
for await (const req of serve(":8000")) {
  req.respond({ body });
}
```

```shell
$ deno run --importmap=import_map.json hello_server.ts
```

## WASM support

Deno can execute [wasm](https://webassembly.org/) binaries.

<!-- prettier-ignore-start -->
```js
const wasmCode = new Uint8Array([
  0, 97, 115, 109, 1, 0, 0, 0, 1, 133, 128, 128, 128, 0, 1, 96, 0, 1, 127,
  3, 130, 128, 128, 128, 0, 1, 0, 4, 132, 128, 128, 128, 0, 1, 112, 0, 0,
  5, 131, 128, 128, 128, 0, 1, 0, 1, 6, 129, 128, 128, 128, 0, 0, 7, 145,
  128, 128, 128, 0, 2, 6, 109, 101, 109, 111, 114, 121, 2, 0, 4, 109, 97,
  105, 110, 0, 0, 10, 138, 128, 128, 128, 0, 1, 132, 128, 128, 128, 0, 0,
  65, 42, 11
]);
const wasmModule = new WebAssembly.Module(wasmCode);
const wasmInstance = new WebAssembly.Instance(wasmModule);
console.log(wasmInstance.exports.main().toString());
```
<!-- prettier-ignore-end -->

WASM files can also be loaded using imports:

```ts
import { fib } from "./fib.wasm";
console.log(fib(20));
```

## Compiler API

Deno supports runtime access to the built in TypeScript compiler. There are
three methods in the `Deno` namespace that provide this access.

### `Deno.compile()`

This works similar to `deno fetch` in that it can fetch code, compile it, but
not run it. It takes up to three arguments, the `rootName`, optionally
`sources`, and optionally `options`. The `rootName` is the root module which
will be used to generate the resulting program. This is like module name you
would pass on the command line in `deno --reload run example.ts`. The `sources`
is a hash where the key is the fully qualified module name, and the value is the
text source of the module. If `sources` is passed, Deno will resolve all the
modules from within that hash and not attempt to resolve them outside of Deno.
If `sources` are not provided, Deno will resolve modules as if the root module
had been passed on the command line. Deno will also cache any of these
resources. The `options` argument is a set of options of type
`Deno.CompilerOptions`, which is a subset of the TypeScript compiler options
which can be supported by Deno.

The method resolves with a tuple where the first argument is any diagnostics
(syntax or type errors) related to the code, and a map of the code, where the
key would be the output filename and the value would be the content.

An example of providing sources:

```ts
const [diagnostics, emitMap] = await Deno.compile("/foo.ts", {
  "/foo.ts": `import * as bar from "./bar.ts";\nconsole.log(bar);\n`,
  "/bar.ts": `export const bar = "bar";\n`
});

assert(diagnostics == null); // ensuring no diagnostics are returned
console.log(emitMap);
```

We would expect map to contain 4 "files", named `/foo.js.map`, `/foo.js`,
`/bar.js.map`, and `/bar.js`.

When not supplying resources, you can use local or remote modules, just like you
could do on the command line. So you could do something like this:

```ts
const [diagnostics, emitMap] = await Deno.compile(
  "https://deno.land/std/examples/welcome.ts"
);
```

We should get back in the `emitMap` a simple `console.log()` statement.

### `Deno.bundle()`

This works a lot like `deno bundle` does on the command line. It is also like
`Deno.compile()`, except instead of returning a map of files, it returns a
single string, which is a self-contained JavaScript ES module which will include
all of the code that was provided or resolved as well as exports of all the
exports of the root module that was provided. It takes up to three arguments,
the `rootName`, optionally `sources`, and optionally `options`. The `rootName`
is the root module which will be used to generate the resulting program. This is
like module name you would pass on the command line in `deno bundle example.ts`.
The `sources` is a hash where the key is the fully qualified module name, and
the value is the text source of the module. If `sources` is passed, Deno will
resolve all the modules from within that hash and not attempt to resolve them
outside of Deno. If `sources` are not provided, Deno will resolve modules as if
the root module had been passed on the command line. Deno will also cache any of
these resources. The `options` argument is a set of options of type
`Deno.CompilerOptions`, which is a subset of the TypeScript compiler options
which can be supported by Deno.

An example of providing sources:

```ts
const [diagnostics, emit] = await Deno.bundle("/foo.ts", {
  "/foo.ts": `import * as bar from "./bar.ts";\nconsole.log(bar);\n`,
  "/bar.ts": `export const bar = "bar";\n`
});

assert(diagnostics == null); // ensuring no diagnostics are returned
console.log(emit);
```

We would expect `emit` to be the text for an ES module, which would contain the
output sources for both modules.

When not supplying resources, you can use local or remote modules, just like you
could do on the command line. So you could do something like this:

```ts
const [diagnostics, emit] = await Deno.bundle(
  "https://deno.land/std/http/server.ts"
);
```

We should get back in `emit` a self contained JavaScript ES module with all of
its dependencies resolved and exporting the same exports as the source module.

### `Deno.transpileOnly()`

This is based off of the TypeScript function `transpileModule()`. All this does
is "erase" any types from the modules and emit JavaScript. There is no type
checking and no resolution of dependencies. It accepts up to two arguments, the
first is a hash where the key is the module name and the value is the contents.
The only purpose of the module name is when putting information into a source
map, of what the source file name was. The second is optionally `options` which
is of type `Deno.CompilerOptions`. This is a subset of options which can be
supported by Deno. It resolves with a map where the key is the source module
name supplied, and the value is an object with a property of `source` which is
the output contents of the module, and optionally `map` which would be the
source map. By default, source maps are output, but can be turned off via the
`options` argument.

An example:

```ts
const result = await Deno.transpileOnly({
  "/foo.ts": `enum Foo { Foo, Bar, Baz };\n`
});

console.log(result["/foo.ts"].source);
console.log(result["/foo.ts"].map);
```

We would expect the `enum` would be rewritten to an IIFE which constructs the
enumerable, and the map to be defined.

## Program lifecycle

Deno supports browser compatible lifecycle events: `load` and `unload`. You can
use these event to provide setup and cleanup code in your program.

`load` event listener supports asynchronous functions and will await these
functions. `unload` event listener supports only synchronous code. Both events
are not cancellable.

Example:

```typescript
// main.ts
import "./imported.ts";

const handler = (e: Event): void => {
  console.log(`got ${e.type} event in event handler (main)`);
};

window.addEventListener("load", handler);

window.addEventListener("unload", handler);

window.onload = (e: Event): void => {
  console.log(`got ${e.type} event in onload function (main)`);
};

window.onunload = (e: Event): void => {
  console.log(`got ${e.type} event in onunload function (main)`);
};

// imported.ts
const handler = (e: Event): void => {
  console.log(`got ${e.type} event in event handler (imported)`);
};

window.addEventListener("load", handler);
window.addEventListener("unload", handler);

window.onload = (e: Event): void => {
  console.log(`got ${e.type} event in onload function (imported)`);
};

window.onunload = (e: Event): void => {
  console.log(`got ${e.type} event in onunload function (imported)`);
};

console.log("log from imported script");
```

Note that you can use both `window.addEventListener` and
`window.onload`/`window.onunload` to define handlers for events. There is a
major difference between them, let's run example:

```shell
$ deno main.ts
log from imported script
log from main script
got load event in onload function (main)
got load event in event handler (imported)
got load event in event handler (main)
got unload event in onunload function (main)
got unload event in event handler (imported)
got unload event in event handler (main)
```

All listeners added using `window.addEventListener` were run, but
`window.onload` and `window.onunload` defined in `main.ts` overridden handlers
defined in `imported.ts`.

## Internal details

### Deno and Linux analogy

|                       **Linux** | **Deno**                         |
| ------------------------------: | :------------------------------- |
|                       Processes | Web Workers                      |
|                        Syscalls | Ops                              |
|           File descriptors (fd) | [Resource ids (rid)](#resources) |
|                       Scheduler | Tokio                            |
| Userland: libc++ / glib / boost | https://deno.land/std/           |
|                 /proc/\$\$/stat | [Deno.metrics()](#metrics)       |
|                       man pages | deno types                       |

#### Resources

Resources (AKA `rid`) are Deno's version of file descriptors. They are integer
values used to refer to open files, sockets, and other concepts. For testing it
would be good to be able to query the system for how many open resources there
are.

```ts
const { resources, close } = Deno;
console.log(resources());
// { 0: "stdin", 1: "stdout", 2: "stderr" }
close(0);
console.log(resources());
// { 1: "stdout", 2: "stderr" }
```

#### Metrics

Metrics is Deno's internal counters for various statics.

```shell
> console.table(Deno.metrics())
┌──────────────────┬────────┐
│     (index)      │ Values │
├──────────────────┼────────┤
│  opsDispatched   │   9    │
│   opsCompleted   │   9    │
│ bytesSentControl │  504   │
│  bytesSentData   │   0    │
│  bytesReceived   │  856   │
└──────────────────┴────────┘
```

### Schematic diagram

<img src="https://deno.land/images/schematic_v0.2.png">

### Profiling

To start profiling,

```sh
# Make sure we're only building release.
# Build deno and V8's d8.
ninja -C target/release d8

# Start the program we want to benchmark with --prof
./target/release/deno tests/http_bench.ts --allow-net --v8-flags=--prof &

# Exercise it.
third_party/wrk/linux/wrk http://localhost:4500/
kill `pgrep deno`
```

V8 will write a file in the current directory that looks like this:
`isolate-0x7fad98242400-v8.log`. To examine this file:

```sh
D8_PATH=target/release/ ./third_party/v8/tools/linux-tick-processor
isolate-0x7fad98242400-v8.log > prof.log
# on macOS, use ./third_party/v8/tools/mac-tick-processor instead
```

`prof.log` will contain information about tick distribution of different calls.

To view the log with Web UI, generate JSON file of the log:

```sh
D8_PATH=target/release/ ./third_party/v8/tools/linux-tick-processor
isolate-0x7fad98242400-v8.log --preprocess > prof.json
```

Open `third_party/v8/tools/profview/index.html` in your browser, and select
`prof.json` to view the distribution graphically.

Useful V8 flags during profiling:

- --prof
- --log-internal-timer-events
- --log-timer-events
- --track-gc
- --log-source-code
- --track-gc-object-stats

Note that you might need to run Deno with `--current-thread` flag to capture
full V8 profiling output.

To learn more about `d8` and profiling, check out the following links:

- [https://v8.dev/docs/d8](https://v8.dev/docs/d8)
- [https://v8.dev/docs/profile](https://v8.dev/docs/profile)

### Debugging with LLDB

We can use LLDB to debug Deno.

```shell
$ lldb -- target/debug/deno run tests/worker.js
> run
> bt
> up
> up
> l
```

To debug Rust code, we can use `rust-lldb`. It should come with `rustc` and is a
wrapper around LLDB.

```shell
$ rust-lldb -- ./target/debug/deno run --allow-net tests/http_bench.ts
# On macOS, you might get warnings like
# `ImportError: cannot import name _remove_dead_weakref`
# In that case, use system python by setting PATH, e.g.
# PATH=/System/Library/Frameworks/Python.framework/Versions/2.7/bin:$PATH
(lldb) command script import "/Users/kevinqian/.rustup/toolchains/1.36.0-x86_64-apple-darwin/lib/rustlib/etc/lldb_rust_formatters.py"
(lldb) type summary add --no-value --python-function lldb_rust_formatters.print_val -x ".*" --category Rust
(lldb) type category enable Rust
(lldb) target create "../deno/target/debug/deno"
Current executable set to '../deno/target/debug/deno' (x86_64).
(lldb) settings set -- target.run-args  "tests/http_bench.ts" "--allow-net"
(lldb) b op_start
(lldb) r
```

### Deno Core

The core binding layer for Deno. It is released as a
[standalone crate](https://crates.io/crates/deno). Inside of core is V8 itself,
with a binding API called "libdeno". See the crate documentation for more
details.

### Continuous Benchmarks

See our benchmarks [over here](https://deno.land/benchmarks.html)

The benchmark chart supposes `//website/data.json` has the type
`BenchmarkData[]` where `BenchmarkData` is defined like the below:

```ts
interface ExecTimeData {
  mean: number;
  stddev: number;
  user: number;
  system: number;
  min: number;
  max: number;
}

interface BenchmarkData {
  created_at: string;
  sha1: string;
  benchmark: {
    [key: string]: ExecTimeData;
  };
  binarySizeData: {
    [key: string]: number;
  };
  threadCountData: {
    [key: string]: number;
  };
  syscallCountData: {
    [key: string]: number;
  };
}
```

### Logos

These Deno logos, like the Deno software, are distributed under the MIT license
(public domain and free for use)

- [A hand drawn one by @ry](https://deno.land/images/deno_logo.png)

- [An animated one by @hashrock](https://github.com/denolib/animated-deno-logo/)

- [A high resolution SVG one by @kevinkassimo](https://github.com/denolib/high-res-deno-logo)

- [A pixelated animation one by @tanakaworld](https://deno.land/images/deno_logo_4.gif)

## Contributing

- Read the [style guide](style_guide.md).
- Progress towards future releases is tracked
  [here](https://github.com/denoland/deno/milestones).
- Please don't make [the benchmarks](https://deno.land/benchmarks.html) worse.
- Ask for help in the [community chat room](https://gitter.im/denolife/Lobby).
- If you are going to work on an issue, mention so in the issue comments
  _before_ you start working on the issue.

### Development

#### Cloning the Repository

Clone on Linux or Mac:

```bash
git clone --recurse-submodules https://github.com/denoland/deno.git
```

Extra steps for Windows users:

1. [Enable "Developer Mode"](https://www.google.com/search?q=windows+enable+developer+mode)
   (otherwise symlinks would require administrator privileges).
2. Make sure you are using git version 2.19.2.windows.1 or newer.
3. Set `core.symlinks=true` before the checkout:
   ```bash
   git config --global core.symlinks true
   git clone --recurse-submodules https://github.com/denoland/deno.git
   ```

#### Prerequisites

Deno has most of its dependencies in a git submodule to ensure reproducible
builds. The following must be installed separately:

<!-- prettier-ignore-start -->
<!-- see https://github.com/prettier/prettier/issues/3679 -->

1. [Rust](https://www.rust-lang.org/en-US/install.html)
    - Ensure that your version is compatible with the one used in [CI](
      https://github.com/denoland/deno/blob/master/.github/workflows/ci.yml).
      This is updated frequently.
2. [Python 2](https://www.python.org/downloads)
    - Ensure that a suffix-less `python`/`python.exe` exists in your `PATH` and
      it refers to Python 2, [not 3](
      https://github.com/denoland/deno/issues/464#issuecomment-411795578).

Extra steps for Linux users:

- Install glib-2.0 development files.
    - Required by [rusty_v8](https://github.com/denoland/rusty_v8#build).
    - On Ubuntu, run `sudo apt install libglib2.0-dev`.

Extra steps for Mac users:

- Install [XCode](https://developer.apple.com/xcode/) :(

Extra steps for Windows users:

1. Get [VS Community 2017](https://www.visualstudio.com/downloads/) with
   "Desktop development with C++" toolkit and make sure to select the following
   required tools listed below along with all C++ tools.
    - Windows 10 SDK >= 10.0.17134
    - Visual C++ ATL for x86 and x64
    - Visual C++ MFC for x86 and x64
    - C++ profiling tools
2. Enable "Debugging Tools for Windows". Go to "Control Panel" → "Programs" →
   "Programs and Features" → Select "Windows Software Development Kit - Windows
   10" → "Change" → "Change" → Check "Debugging Tools For Windows" → "Change" ->
   "Finish".

<!-- prettier-ignore-end -->

#### Building

Build with Cargo:

```bash
# Build:
cargo build -vv

# Run:
./target/debug/deno tests/002_hello.ts
```

#### Testing and Tools

Test `deno`:

```bash
# Run the whole suite:
cargo test

# Only test cli/js/:
cargo test js_unit_tests
```

Test `std/`:

```bash
cd std
cargo run -- -A testing/runner.ts --exclude "**/testdata"
```

Lint the code:

```bash
./tools/lint.py
```

Format the code:

```bash
./tools/format.py
```

#### Other Useful Commands

```bash
# Call ninja manually.
ninja -C target/debug

# Build a release binary.
cargo build --release

# List executable targets.
gn --root=core/libdeno ls target/debug "//:*" --as=output --type=executable

# List build configuration.
gn --root=core/libdeno args target/debug/ --list

# Edit build configuration.
gn --root=core/libdeno args target/debug/

# Describe a target.
gn --root=core/libdeno desc target/debug/ :deno
gn help

# Update third_party modules
git submodule update

# Skip downloading binary build tools and point the build
# to the system provided ones (for packagers of deno ...).
export DENO_BUILD_ARGS="clang_base_path=/usr clang_use_chrome_plugins=false"
DENO_NO_BINARY_DOWNLOAD=1 DENO_GN_PATH=/usr/bin/gn cargo build
```

Environment variables: `DENO_BUILD_MODE`, `DENO_BUILD_PATH`, `DENO_BUILD_ARGS`,
`DENO_DIR`, `DENO_GN_PATH`, `DENO_NO_BINARY_DOWNLOAD`.

### Submitting a Pull Request

Before submitting, please make sure the following is done:

1. That there is a related issue and it is referenced in the PR text.
2. There are tests that cover the changes.
3. Ensure `cargo test` passes.
4. Format your code with `tools/format.py`
5. Make sure `./tools/lint.py` passes.

### Changes to `third_party`

[`deno_third_party`](https://github.com/denoland/deno_third_party) contains most
of the external code that Deno depends on, so that we know exactly what we are
executing at any given time. It is carefully maintained with a mixture of manual
labor and private scripts. It's likely you will need help from @ry or
@piscisaureus to make changes.

### Adding Ops (aka bindings)

We are very concerned about making mistakes when adding new APIs. When adding an
Op to Deno, the counterpart interfaces on other platforms should be researched.
Please list how this functionality is done in Go, Node, Rust, and Python.

As an example, see how `Deno.rename()` was proposed and added in
[PR #671](https://github.com/denoland/deno/pull/671).

### Documenting APIs

It is important to document public APIs and we want to do that inline with the
code. This helps ensure that code and documentation are tightly coupled
together.

#### Utilize JSDoc

All publicly exposed APIs and types, both via the `deno` module as well as the
global/`window` namespace should have JSDoc documentation. This documentation is
parsed and available to the TypeScript compiler, and therefore easy to provide
further downstream. JSDoc blocks come just prior to the statement they apply to
and are denoted by a leading `/**` before terminating with a `*/`. For example:

```ts
/** A simple JSDoc comment */
export const FOO = "foo";
```
