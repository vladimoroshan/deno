// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
#[cfg(feature = "no-snapshot-init")]
use deno::Script;

use deno::StartupData;
use deno_cli_snapshots::CLI_SNAPSHOT;
use deno_cli_snapshots::COMPILER_SNAPSHOT;

#[cfg(feature = "no-snapshot-init")]
pub fn deno_isolate_init() -> StartupData<'static> {
  debug!("Deno isolate init without snapshots.");
  #[cfg(not(feature = "check-only"))]
  let source =
    include_str!(concat!(env!("GN_OUT_DIR"), "/gen/cli/bundle/main.js"));
  #[cfg(feature = "check-only")]
  let source = "";

  StartupData::Script(Script {
    filename: "gen/cli/bundle/main.js",
    source,
  })
}

#[cfg(not(feature = "no-snapshot-init"))]
pub fn deno_isolate_init() -> StartupData<'static> {
  debug!("Deno isolate init with snapshots.");
  #[cfg(not(feature = "check-only"))]
  let data = CLI_SNAPSHOT;
  #[cfg(feature = "check-only")]
  let data = b"";

  StartupData::Snapshot(data)
}

#[cfg(feature = "no-snapshot-init")]
pub fn compiler_isolate_init() -> StartupData<'static> {
  debug!("Compiler isolate init without snapshots.");
  #[cfg(not(feature = "check-only"))]
  let source =
    include_str!(concat!(env!("GN_OUT_DIR"), "/gen/cli/bundle/compiler.js"));
  #[cfg(feature = "check-only")]
  let source = "";

  StartupData::Script(Script {
    filename: "gen/cli/bundle/compiler.js",
    source,
  })
}

#[cfg(not(feature = "no-snapshot-init"))]
pub fn compiler_isolate_init() -> StartupData<'static> {
  debug!("Deno isolate init with snapshots.");
  #[cfg(not(feature = "check-only"))]
  let data = COMPILER_SNAPSHOT;
  #[cfg(feature = "check-only")]
  let data = b"";

  StartupData::Snapshot(data)
}
