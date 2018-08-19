# deno

| **Linux** | **Windows** |
|:---------------:|:-----------:|
| [![Travis](https://travis-ci.com/denoland/deno.svg?branch=master)](https://travis-ci.com/denoland/deno) | [![Appveyor](https://ci.appveyor.com/api/projects/status/yel7wtcqwoy0to8x?branch=master&svg=true)](https://ci.appveyor.com/project/deno/deno) |



## A secure TypeScript runtime built on V8

* Supports TypeScript 3.0.1 out of the box. Uses V8 6.9.297. That is, it's
  very modern JavaScript.

* No `package.json`. No npm. Not explicitly compatible with Node.

* Imports reference source code URLs only.
	```
  import { test } from "https://unpkg.com/deno_testing@0.0.5/testing.ts"
  import { log } from "./util.ts"
	```
  Remote code is fetched and cached on first execution, and never updated until
  the code is run with the `--reload` flag. (So, this will still work on an
  airplane. See `~/.deno/src` for details on the cache.)

* File system and network access can be controlled in order to run sandboxed
  code. Defaults to read-only file system access and no network access.
	Access between V8 (unprivileged) and Rust (privileged) is only done via
  serialized messages defined in this
  [flatbuffer](https://github.com/denoland/deno/blob/master/src/msg.fbs). This makes it
  easy to audit.
	To enable write access explicitly use `--allow-write` and `--allow-net` for
  network access.

* Single executable:
	```
  > ls -lh out/release/deno
  -rwxr-xr-x  1 rld  staff    48M Aug  2 13:24 out/release/deno
  > otool -L out/release/deno
  out/release/deno:
    /usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1252.50.4)
    /usr/lib/libresolv.9.dylib (compatibility version 1.0.0, current version 1.0.0)
    /System/Library/Frameworks/Security.framework/Versions/A/Security (compatibility version 1.0.0, current version 58286.51.6)
    /usr/lib/libc++.1.dylib (compatibility version 1.0.0, current version 400.9.0)
  >
	```

* Always dies on uncaught errors.

* Supports top-level `await`.

* Aims to be browser compatible.


## Status

Under development.

The prototype golang implementation is
[here](https://github.com/denoland/deno/tree/golang). We are in the process of
rewriting in C++/Rust to avoid future GC contention between Go and V8.

Progress towards first release is tracked
[here](https://github.com/denoland/deno/issues?utf8=%E2%9C%93&q=is%3Aissue+milestone%3A%22v0.1+%28first+binary+release%29%22+).

Roadmap is [here](https://github.com/denoland/deno/blob/master/Roadmap.md).
Also see [this presentation](http://tinyclouds.org/jsconf2018.pdf).

[Chat room](https://gitter.im/denolife/Lobby).


## Build instructions

To ensure reproducable builds, Deno has most of its dependencies in a git
submodule. However, you need to install separately:

1. [Rust](https://www.rust-lang.org/en-US/install.html)
2. [Node](http://nodejs.org/)
3. Python 2. [Not 3](https://github.com/denoland/deno/issues/464#issuecomment-411795578).
4. [ccache](https://developer.mozilla.org/en-US/docs/Mozilla/Developer_guide/Build_Instructions/ccache) (Optional but helpful for speeding up rebuilds of V8.)
.

To build:

    # Fetch deps.
    git clone --recurse-submodules https://github.com/denoland/deno.git
    cd deno
    ./tools/setup.py

    # Build.
    ./tools/build.py

    # Run
    ./out/debug/deno tests/002_hello.ts

Other useful commands:

    # Call ninja manually.
    ./third_party/depot_tools/ninja -C out/debug
    # Build a release binary.
    DENO_BUILD_MODE=release ./tools/build.py :deno
    # List executable targets.
    ./third_party/depot_tools/gn ls out/debug //:* --as=output --type=executable
    # List build configuation.
    ./third_party/depot_tools/gn args out/debug/ --list
    # Edit build configuration.
    ./third_party/depot_tools/gn args out/debug/
    # Describe a target.
    ./third_party/depot_tools/gn desc out/debug/ :deno
    ./third_party/depot_tools/gn help

Env vars: `DENO_BUILD_MODE`, `DENO_BUILD_PATH`, `DENO_BUILD_ARGS`.
