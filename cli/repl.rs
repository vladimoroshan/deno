// Copyright 2018-2019 the Deno authors. All rights reserved. MIT license.
use crate::deno_dir::DenoDir;
use deno::ErrBox;
use rustyline;
use std::path::PathBuf;

#[cfg(not(windows))]
use rustyline::Editor;

// Work around the issue that on Windows, `struct Editor` does not implement the
// `Send` trait, because it embeds a windows HANDLE which is a type alias for
// *mut c_void. This value isn't actually a pointer and there's nothing that
// can be mutated through it, so hack around it. TODO: a prettier solution.
#[cfg(windows)]
use std::ops::{Deref, DerefMut};

#[cfg(windows)]
struct Editor<T: rustyline::Helper> {
  inner: rustyline::Editor<T>,
}

#[cfg(windows)]
unsafe impl<T: rustyline::Helper> Send for Editor<T> {}

#[cfg(windows)]
impl<T: rustyline::Helper> Editor<T> {
  pub fn new() -> Editor<T> {
    Editor {
      inner: rustyline::Editor::<T>::new(),
    }
  }
}

#[cfg(windows)]
impl<T: rustyline::Helper> Deref for Editor<T> {
  type Target = rustyline::Editor<T>;

  fn deref(&self) -> &rustyline::Editor<T> {
    &self.inner
  }
}

#[cfg(windows)]
impl<T: rustyline::Helper> DerefMut for Editor<T> {
  fn deref_mut(&mut self) -> &mut rustyline::Editor<T> {
    &mut self.inner
  }
}

pub struct Repl {
  editor: Editor<()>,
  history_file: PathBuf,
}

impl Repl {
  pub fn new(history_file: PathBuf) -> Self {
    let mut repl = Self {
      editor: Editor::<()>::new(),
      history_file,
    };

    repl.load_history();
    repl
  }

  fn load_history(&mut self) {
    debug!("Loading REPL history: {:?}", self.history_file);
    self
      .editor
      .load_history(&self.history_file.to_str().unwrap())
      .map_err(|e| {
        debug!("Unable to load history file: {:?} {}", self.history_file, e)
      })
      // ignore this error (e.g. it occurs on first load)
      .unwrap_or(())
  }

  fn save_history(&mut self) -> Result<(), ErrBox> {
    if !self.history_dir_exists() {
      eprintln!(
        "Unable to save REPL history: {:?} directory does not exist",
        self.history_file
      );
      return Ok(());
    }

    self
      .editor
      .save_history(&self.history_file.to_str().unwrap())
      .map(|_| debug!("Saved REPL history to: {:?}", self.history_file))
      .map_err(|e| {
        eprintln!("Unable to save REPL history: {:?} {}", self.history_file, e);
        ErrBox::from(e)
      })
  }

  fn history_dir_exists(&self) -> bool {
    self
      .history_file
      .parent()
      .map(|ref p| p.exists())
      .unwrap_or(false)
  }

  pub fn readline(&mut self, prompt: &str) -> Result<String, ErrBox> {
    self
      .editor
      .readline(&prompt)
      .map(|line| {
        self.editor.add_history_entry(line.clone());
        line
      })
      .map_err(ErrBox::from)
    // Forward error to TS side for processing
  }
}

impl Drop for Repl {
  fn drop(&mut self) {
    self.save_history().unwrap();
  }
}

pub fn history_path(dir: &DenoDir, history_file: &str) -> PathBuf {
  let mut p: PathBuf = dir.root.clone();
  p.push(history_file);
  p
}
