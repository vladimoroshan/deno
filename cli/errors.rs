// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

//! There are many types of errors in Deno:
//! - ErrBox: a generic boxed object. This is the super type of all
//!   errors handled in Rust.
//! - JSError: a container for the error message and stack trace for exceptions
//!   thrown in JavaScript code. We use this to pretty-print stack traces.
//! - Diagnostic: these are errors that originate in TypeScript's compiler.
//!   They're similar to JSError, in that they have line numbers.
//!   But Diagnostics are compile-time type errors, whereas JSErrors are runtime
//!   exceptions.

use crate::import_map::ImportMapError;
use crate::swc_util::SwcDiagnosticBuffer;
use deno_core::ErrBox;
use deno_core::ModuleResolutionError;
use rustyline::error::ReadlineError;
use std::env;
use std::error::Error;
use std::io;

fn get_dlopen_error_class(error: &dlopen::Error) -> &'static str {
  use dlopen::Error::*;
  match error {
    NullCharacter(_) => "InvalidData",
    OpeningLibraryError(ref e) => get_io_error_class(e),
    SymbolGettingError(ref e) => get_io_error_class(e),
    AddrNotMatchingDll(ref e) => get_io_error_class(e),
    NullSymbol => "NotFound",
  }
}

fn get_env_var_error_class(error: &env::VarError) -> &'static str {
  use env::VarError::*;
  match error {
    NotPresent => "NotFound",
    NotUnicode(..) => "InvalidData",
  }
}

fn get_import_map_error_class(_: &ImportMapError) -> &'static str {
  "URIError"
}

fn get_io_error_class(error: &io::Error) -> &'static str {
  use io::ErrorKind::*;
  match error.kind() {
    NotFound => "NotFound",
    PermissionDenied => "PermissionDenied",
    ConnectionRefused => "ConnectionRefused",
    ConnectionReset => "ConnectionReset",
    ConnectionAborted => "ConnectionAborted",
    NotConnected => "NotConnected",
    AddrInUse => "AddrInUse",
    AddrNotAvailable => "AddrNotAvailable",
    BrokenPipe => "BrokenPipe",
    AlreadyExists => "AlreadyExists",
    InvalidInput => "TypeError",
    InvalidData => "InvalidData",
    TimedOut => "TimedOut",
    Interrupted => "Interrupted",
    WriteZero => "WriteZero",
    UnexpectedEof => "UnexpectedEof",
    Other => "Error",
    WouldBlock => unreachable!(),
    // Non-exhaustive enum - might add new variants
    // in the future
    _ => unreachable!(),
  }
}

fn get_module_resolution_error_class(
  _: &ModuleResolutionError,
) -> &'static str {
  "URIError"
}

fn get_notify_error_class(error: &notify::Error) -> &'static str {
  use notify::ErrorKind::*;
  match error.kind {
    Generic(_) => "Error",
    Io(ref e) => get_io_error_class(e),
    PathNotFound => "NotFound",
    WatchNotFound => "NotFound",
    InvalidConfig(_) => "InvalidData",
  }
}

fn get_readline_error_class(error: &ReadlineError) -> &'static str {
  use ReadlineError::*;
  match error {
    Io(err) => get_io_error_class(err),
    Eof => "UnexpectedEof",
    Interrupted => "Interrupted",
    #[cfg(unix)]
    Errno(err) => get_nix_error_class(err),
    _ => unimplemented!(),
  }
}

fn get_regex_error_class(error: &regex::Error) -> &'static str {
  use regex::Error::*;
  match error {
    Syntax(_) => "SyntaxError",
    CompiledTooBig(_) => "RangeError",
    _ => "Error",
  }
}

fn get_request_error_class(error: &reqwest::Error) -> &'static str {
  error
    .source()
    .and_then(|inner_err| {
      (inner_err
        .downcast_ref::<io::Error>()
        .map(get_io_error_class))
      .or_else(|| {
        inner_err
          .downcast_ref::<serde_json::error::Error>()
          .map(get_serde_json_error_class)
      })
      .or_else(|| {
        inner_err
          .downcast_ref::<url::ParseError>()
          .map(get_url_parse_error_class)
      })
    })
    .unwrap_or("Http")
}

fn get_serde_json_error_class(
  error: &serde_json::error::Error,
) -> &'static str {
  use serde_json::error::*;
  match error.classify() {
    Category::Io => error
      .source()
      .and_then(|e| e.downcast_ref::<io::Error>())
      .map(get_io_error_class)
      .unwrap(),
    Category::Syntax => "SyntaxError",
    Category::Data => "InvalidData",
    Category::Eof => "UnexpectedEof",
  }
}

fn get_swc_diagnostic_class(_: &SwcDiagnosticBuffer) -> &'static str {
  "SyntaxError"
}

fn get_url_parse_error_class(_error: &url::ParseError) -> &'static str {
  "URIError"
}

#[cfg(unix)]
fn get_nix_error_class(error: &nix::Error) -> &'static str {
  use nix::errno::Errno::*;
  match error {
    nix::Error::Sys(EPERM) => "PermissionDenied",
    nix::Error::Sys(EINVAL) => "TypeError",
    nix::Error::Sys(ENOENT) => "NotFound",
    nix::Error::Sys(ENOTTY) => "BadResource",
    nix::Error::Sys(UnknownErrno) => unreachable!(),
    nix::Error::Sys(_) => unreachable!(),
    nix::Error::InvalidPath => "TypeError",
    nix::Error::InvalidUtf8 => "InvalidData",
    nix::Error::UnsupportedOperation => unreachable!(),
  }
}

pub fn get_error_class(e: &ErrBox) -> &'static str {
  use ErrBox::*;
  match e {
    Simple { class, .. } => Some(*class),
    _ => None,
  }
  .or_else(|| {
    e.downcast_ref::<dlopen::Error>()
      .map(get_dlopen_error_class)
  })
  .or_else(|| {
    e.downcast_ref::<env::VarError>()
      .map(get_env_var_error_class)
  })
  .or_else(|| {
    e.downcast_ref::<ImportMapError>()
      .map(get_import_map_error_class)
  })
  .or_else(|| e.downcast_ref::<io::Error>().map(get_io_error_class))
  .or_else(|| {
    e.downcast_ref::<ModuleResolutionError>()
      .map(get_module_resolution_error_class)
  })
  .or_else(|| {
    e.downcast_ref::<notify::Error>()
      .map(get_notify_error_class)
  })
  .or_else(|| {
    e.downcast_ref::<ReadlineError>()
      .map(get_readline_error_class)
  })
  .or_else(|| {
    e.downcast_ref::<reqwest::Error>()
      .map(get_request_error_class)
  })
  .or_else(|| e.downcast_ref::<regex::Error>().map(get_regex_error_class))
  .or_else(|| {
    e.downcast_ref::<serde_json::error::Error>()
      .map(get_serde_json_error_class)
  })
  .or_else(|| {
    e.downcast_ref::<SwcDiagnosticBuffer>()
      .map(get_swc_diagnostic_class)
  })
  .or_else(|| {
    e.downcast_ref::<url::ParseError>()
      .map(get_url_parse_error_class)
  })
  .or_else(|| {
    #[cfg(unix)]
    let maybe_get_nix_error_class =
      || e.downcast_ref::<nix::Error>().map(get_nix_error_class);
    #[cfg(not(unix))]
    let maybe_get_nix_error_class = || Option::<&'static str>::None;
    (maybe_get_nix_error_class)()
  })
  .unwrap_or_else(|| {
    panic!("ErrBox '{}' contains boxed error of unknown type", e);
  })
}

pub fn rust_err_to_json(error: &ErrBox) -> Box<[u8]> {
  let error_value =
    json!({ "kind": get_error_class(error), "message": error.to_string()});
  serde_json::to_vec(&error_value).unwrap().into_boxed_slice()
}
