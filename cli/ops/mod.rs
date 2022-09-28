// Copyright 2018-2022 the Deno authors. All rights reserved. MIT license.

use crate::proc_state::ProcState;
use deno_core::anyhow::bail;
use deno_core::error::AnyError;
use deno_core::op;
use deno_core::Extension;
use deno_core::OpState;

pub mod bench;
pub mod testing;

pub fn cli_exts(ps: ProcState) -> Vec<Extension> {
  vec![init_proc_state(ps)]
}

fn init_proc_state(ps: ProcState) -> Extension {
  Extension::builder()
    .ops(vec![op_npm_process_state::decl()])
    .state(move |state| {
      state.put(ps.clone());
      Ok(())
    })
    .build()
}

#[op]
fn op_npm_process_state(state: &mut OpState) -> Result<String, AnyError> {
  let proc_state = state.borrow_mut::<ProcState>();
  if !proc_state.options.unstable() {
    bail!(
      "Unstable use of npm process state. The --unstable flag must be provided."
    )
  }
  Ok(proc_state.npm_resolver.get_npm_process_state())
}
