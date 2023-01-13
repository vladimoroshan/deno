// Copyright 2018-2023 the Deno authors. All rights reserved. MIT license.

use deno_runtime::ops::tty::ConsoleSize;

/// Gets the console size.
pub fn console_size() -> Option<ConsoleSize> {
  let stderr = &deno_runtime::ops::io::STDERR_HANDLE;
  deno_runtime::ops::tty::console_size(stderr).ok()
}
