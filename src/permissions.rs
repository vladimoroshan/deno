// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
use atty;

use crate::flags::DenoFlags;

use ansi_term::Style;
use crate::errors::permission_denied;
use crate::errors::DenoResult;
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg_attr(feature = "cargo-clippy", allow(stutter))]
#[derive(Debug, Default)]
pub struct DenoPermissions {
  pub allow_read: AtomicBool,
  pub allow_write: AtomicBool,
  pub allow_net: AtomicBool,
  pub allow_env: AtomicBool,
  pub allow_run: AtomicBool,
}

impl DenoPermissions {
  pub fn new(flags: &DenoFlags) -> Self {
    Self {
      allow_read: AtomicBool::new(flags.allow_read),
      allow_write: AtomicBool::new(flags.allow_write),
      allow_env: AtomicBool::new(flags.allow_env),
      allow_net: AtomicBool::new(flags.allow_net),
      allow_run: AtomicBool::new(flags.allow_run),
    }
  }

  pub fn check_run(&self) -> DenoResult<()> {
    if self.allow_run.load(Ordering::SeqCst) {
      return Ok(());
    };
    // TODO get location (where access occurred)
    let r = permission_prompt("access to run a subprocess");
    if r.is_ok() {
      self.allow_run.store(true, Ordering::SeqCst);
    }
    r
  }

  pub fn check_read(&self, filename: &str) -> DenoResult<()> {
    if self.allow_read.load(Ordering::SeqCst) {
      return Ok(());
    };
    // TODO get location (where access occurred)
    let r = permission_prompt(&format!(
      "Deno requests read access to \"{}\".",
      filename
    ));;
    if r.is_ok() {
      self.allow_read.store(true, Ordering::SeqCst);
    }
    r
  }

  pub fn check_write(&self, filename: &str) -> DenoResult<()> {
    if self.allow_write.load(Ordering::SeqCst) {
      return Ok(());
    };
    // TODO get location (where access occurred)
    let r = permission_prompt(&format!("write access to \"{}\"", filename));;
    if r.is_ok() {
      self.allow_write.store(true, Ordering::SeqCst);
    }
    r
  }

  pub fn check_net(&self, domain_name: &str) -> DenoResult<()> {
    if self.allow_net.load(Ordering::SeqCst) {
      return Ok(());
    };
    // TODO get location (where access occurred)
    let r =
      permission_prompt(&format!("network access to \"{}\"", domain_name));
    if r.is_ok() {
      self.allow_net.store(true, Ordering::SeqCst);
    }
    r
  }

  pub fn check_env(&self) -> DenoResult<()> {
    if self.allow_env.load(Ordering::SeqCst) {
      return Ok(());
    };
    // TODO get location (where access occurred)
    let r = permission_prompt(&"access to environment variables");
    if r.is_ok() {
      self.allow_env.store(true, Ordering::SeqCst);
    }
    r
  }
}

fn permission_prompt(message: &str) -> DenoResult<()> {
  if !atty::is(atty::Stream::Stdin) || !atty::is(atty::Stream::Stderr) {
    return Err(permission_denied());
  };
  let msg = format!("⚠️  Deno requests {}. Grant? [yN] ", message);
  // print to stderr so that if deno is > to a file this is still displayed.
  eprint!("{}", Style::new().bold().paint(msg));
  let mut input = String::new();
  let stdin = io::stdin();
  let _nread = stdin.read_line(&mut input)?;
  let ch = input.chars().next().unwrap();
  let is_yes = ch == 'y' || ch == 'Y';
  if is_yes {
    Ok(())
  } else {
    Err(permission_denied())
  }
}
