// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use crate::fmt_errors::JSError;
use crate::ops;
use crate::state::ThreadSafeState;
use deno_core;
use deno_core::Buf;
use deno_core::ErrBox;
use deno_core::ModuleSpecifier;
use deno_core::StartupData;
use futures::channel::mpsc;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::sink::SinkExt;
use futures::stream::StreamExt;
use futures::task::AtomicWaker;
use std::env;
use std::future::Future;
use std::ops::Deref;
use std::ops::DerefMut;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use tokio::sync::Mutex as AsyncMutex;
use url::Url;

/// Wraps mpsc channels so they can be referenced
/// from ops and used to facilitate parent-child communication
/// for workers.
#[derive(Clone)]
pub struct WorkerChannels {
  pub sender: mpsc::Sender<Buf>,
  pub receiver: Arc<AsyncMutex<mpsc::Receiver<Buf>>>,
}

impl WorkerChannels {
  /// Post message to worker as a host.
  pub async fn post_message(&self, buf: Buf) -> Result<(), ErrBox> {
    let mut sender = self.sender.clone();
    sender.send(buf).map_err(ErrBox::from).await
  }

  /// Get message from worker as a host.
  pub fn get_message(&self) -> Pin<Box<dyn Future<Output = Option<Buf>>>> {
    let receiver_mutex = self.receiver.clone();

    async move {
      let mut receiver = receiver_mutex.lock().await;
      receiver.next().await
    }
    .boxed_local()
  }
}

pub struct WorkerChannelsInternal(WorkerChannels);

impl Deref for WorkerChannelsInternal {
  type Target = WorkerChannels;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for WorkerChannelsInternal {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[derive(Clone)]
pub struct WorkerChannelsExternal(WorkerChannels);

impl Deref for WorkerChannelsExternal {
  type Target = WorkerChannels;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for WorkerChannelsExternal {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

fn create_channels() -> (WorkerChannelsInternal, WorkerChannelsExternal) {
  let (in_tx, in_rx) = mpsc::channel::<Buf>(1);
  let (out_tx, out_rx) = mpsc::channel::<Buf>(1);
  let internal_channels = WorkerChannelsInternal(WorkerChannels {
    sender: out_tx,
    receiver: Arc::new(AsyncMutex::new(in_rx)),
  });
  let external_channels = WorkerChannelsExternal(WorkerChannels {
    sender: in_tx,
    receiver: Arc::new(AsyncMutex::new(out_rx)),
  });
  (internal_channels, external_channels)
}

/// Worker is a CLI wrapper for `deno_core::Isolate`.
///
/// It provides infrastructure to communicate with a worker and
/// consequently between workers.
///
/// This struct is meant to be used as a base struct for concrete
/// type of worker that registers set of ops.
///
/// Currently there are three types of workers:
///  - `MainWorker`
///  - `CompilerWorker`
///  - `WebWorker`
pub struct Worker {
  pub name: String,
  pub isolate: Box<deno_core::EsIsolate>,
  pub state: ThreadSafeState,
  external_channels: WorkerChannelsExternal,
}

impl Worker {
  pub fn new(
    name: String,
    startup_data: StartupData,
    state: ThreadSafeState,
  ) -> Self {
    let mut isolate =
      deno_core::EsIsolate::new(Box::new(state.clone()), startup_data, false);

    let global_state_ = state.global_state.clone();
    isolate.set_js_error_create(move |v8_exception| {
      JSError::from_v8_exception(v8_exception, &global_state_.ts_compiler)
    });

    let (internal_channels, external_channels) = create_channels();
    {
      let mut c = state.worker_channels_internal.lock().unwrap();
      *c = Some(internal_channels);
    }

    Self {
      name,
      isolate,
      state,
      external_channels,
    }
  }

  /// Same as execute2() but the filename defaults to "$CWD/__anonymous__".
  pub fn execute(&mut self, js_source: &str) -> Result<(), ErrBox> {
    let path = env::current_dir().unwrap().join("__anonymous__");
    let url = Url::from_file_path(path).unwrap();
    self.execute2(url.as_str(), js_source)
  }

  /// Executes the provided JavaScript source code. The js_filename argument is
  /// provided only for debugging purposes.
  pub fn execute2(
    &mut self,
    js_filename: &str,
    js_source: &str,
  ) -> Result<(), ErrBox> {
    self.isolate.execute(js_filename, js_source)
  }

  /// Executes the provided JavaScript module.
  pub async fn execute_mod_async(
    &mut self,
    module_specifier: &ModuleSpecifier,
    maybe_code: Option<String>,
    is_prefetch: bool,
  ) -> Result<(), ErrBox> {
    let specifier = module_specifier.to_string();
    let id = self.isolate.load_module(&specifier, maybe_code).await?;
    self.state.global_state.progress.done();
    if !is_prefetch {
      return self.isolate.mod_evaluate(id);
    }
    Ok(())
  }

  /// Returns a way to communicate with the Worker from other threads.
  pub fn thread_safe_handle(&self) -> WorkerChannelsExternal {
    self.external_channels.clone()
  }
}

impl Future for Worker {
  type Output = Result<(), ErrBox>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
    let inner = self.get_mut();
    let waker = AtomicWaker::new();
    waker.register(cx.waker());
    inner.isolate.poll_unpin(cx)
  }
}

/// This worker is created and used by Deno executable.
///
/// It provides ops available in the `Deno` namespace.
///
/// All WebWorkers created during program execution are decendants of
/// this worker.
pub struct MainWorker(Worker);

impl MainWorker {
  pub fn new(
    name: String,
    startup_data: StartupData,
    state: ThreadSafeState,
  ) -> Self {
    let state_ = state.clone();
    let mut worker = Worker::new(name, startup_data, state_);
    {
      let op_registry = worker.isolate.op_registry.clone();
      let isolate = &mut worker.isolate;
      ops::runtime::init(isolate, &state);
      ops::runtime_compiler::init(isolate, &state);
      ops::errors::init(isolate, &state);
      ops::fetch::init(isolate, &state);
      ops::files::init(isolate, &state);
      ops::fs::init(isolate, &state);
      ops::io::init(isolate, &state);
      ops::plugins::init(isolate, &state, op_registry);
      ops::net::init(isolate, &state);
      ops::tls::init(isolate, &state);
      ops::os::init(isolate, &state);
      ops::permissions::init(isolate, &state);
      ops::process::init(isolate, &state);
      ops::random::init(isolate, &state);
      ops::repl::init(isolate, &state);
      ops::resources::init(isolate, &state);
      ops::signal::init(isolate, &state);
      ops::timers::init(isolate, &state);
      ops::worker_host::init(isolate, &state);
      ops::web_worker::init(isolate, &state);
    }
    Self(worker)
  }
}

impl Deref for MainWorker {
  type Target = Worker;
  fn deref(&self) -> &Self::Target {
    &self.0
  }
}

impl DerefMut for MainWorker {
  fn deref_mut(&mut self) -> &mut Self::Target {
    &mut self.0
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::flags;
  use crate::global_state::ThreadSafeGlobalState;
  use crate::progress::Progress;
  use crate::startup_data;
  use crate::state::ThreadSafeState;
  use crate::tokio_util;
  use futures::executor::block_on;
  use std::sync::atomic::Ordering;

  pub fn run_in_task<F>(f: F)
  where
    F: FnOnce() + Send + 'static,
  {
    let fut = futures::future::lazy(move |_cx| f());
    tokio_util::run_basic(fut)
  }

  #[test]
  fn execute_mod_esm_imports_a() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .parent()
      .unwrap()
      .join("cli/tests/esm_imports_a.js");
    let module_specifier =
      ModuleSpecifier::resolve_url_or_path(&p.to_string_lossy()).unwrap();
    let global_state =
      ThreadSafeGlobalState::new(flags::DenoFlags::default(), Progress::new())
        .unwrap();
    let state =
      ThreadSafeState::new(global_state, None, module_specifier.clone())
        .unwrap();
    let state_ = state.clone();
    tokio_util::run_basic(async move {
      let mut worker =
        MainWorker::new("TEST".to_string(), StartupData::None, state);
      let result = worker
        .execute_mod_async(&module_specifier, None, false)
        .await;
      if let Err(err) = result {
        eprintln!("execute_mod err {:?}", err);
      }
      if let Err(e) = (&mut *worker).await {
        panic!("Future got unexpected error: {:?}", e);
      }
    });

    let metrics = &state_.metrics;
    assert_eq!(metrics.resolve_count.load(Ordering::SeqCst), 2);
    // Check that we didn't start the compiler.
    assert_eq!(metrics.compiler_starts.load(Ordering::SeqCst), 0);
  }

  #[test]
  fn execute_mod_circular() {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .parent()
      .unwrap()
      .join("tests/circular1.ts");
    let module_specifier =
      ModuleSpecifier::resolve_url_or_path(&p.to_string_lossy()).unwrap();
    let global_state =
      ThreadSafeGlobalState::new(flags::DenoFlags::default(), Progress::new())
        .unwrap();
    let state =
      ThreadSafeState::new(global_state, None, module_specifier.clone())
        .unwrap();
    let state_ = state.clone();
    tokio_util::run_basic(async move {
      let mut worker =
        MainWorker::new("TEST".to_string(), StartupData::None, state);
      let result = worker
        .execute_mod_async(&module_specifier, None, false)
        .await;
      if let Err(err) = result {
        eprintln!("execute_mod err {:?}", err);
      }
      if let Err(e) = (&mut *worker).await {
        panic!("Future got unexpected error: {:?}", e);
      }
    });

    let metrics = &state_.metrics;
    // TODO  assert_eq!(metrics.resolve_count.load(Ordering::SeqCst), 2);
    // Check that we didn't start the compiler.
    assert_eq!(metrics.compiler_starts.load(Ordering::SeqCst), 0);
  }

  #[tokio::test]
  async fn execute_006_url_imports() {
    let http_server_guard = crate::test_util::http_server();
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .parent()
      .unwrap()
      .join("cli/tests/006_url_imports.ts");
    let module_specifier =
      ModuleSpecifier::resolve_url_or_path(&p.to_string_lossy()).unwrap();
    let flags = flags::DenoFlags {
      subcommand: flags::DenoSubcommand::Run {
        script: module_specifier.to_string(),
      },
      reload: true,
      ..flags::DenoFlags::default()
    };
    let global_state =
      ThreadSafeGlobalState::new(flags, Progress::new()).unwrap();
    let state = ThreadSafeState::new(
      global_state.clone(),
      None,
      module_specifier.clone(),
    )
    .unwrap();
    let mut worker = MainWorker::new(
      "TEST".to_string(),
      startup_data::deno_isolate_init(),
      state.clone(),
    );
    worker.execute("bootstrapMainRuntime()").unwrap();
    let result = worker
      .execute_mod_async(&module_specifier, None, false)
      .await;
    if let Err(err) = result {
      eprintln!("execute_mod err {:?}", err);
    }
    if let Err(e) = (&mut *worker).await {
      panic!("Future got unexpected error: {:?}", e);
    }
    assert_eq!(state.metrics.resolve_count.load(Ordering::SeqCst), 3);
    // Check that we've only invoked the compiler once.
    assert_eq!(
      global_state.metrics.compiler_starts.load(Ordering::SeqCst),
      1
    );
    drop(http_server_guard);
  }

  fn create_test_worker() -> MainWorker {
    let state = ThreadSafeState::mock("./hello.js");
    let mut worker = MainWorker::new(
      "TEST".to_string(),
      startup_data::deno_isolate_init(),
      state,
    );
    worker.execute("bootstrapMainRuntime()").unwrap();
    worker
  }

  #[test]
  fn execute_mod_resolve_error() {
    run_in_task(|| {
      // "foo" is not a valid module specifier so this should return an error.
      let mut worker = create_test_worker();
      let module_specifier =
        ModuleSpecifier::resolve_url_or_path("does-not-exist").unwrap();
      let result =
        block_on(worker.execute_mod_async(&module_specifier, None, false));
      assert!(result.is_err());
    })
  }

  #[test]
  fn execute_mod_002_hello() {
    run_in_task(|| {
      // This assumes cwd is project root (an assumption made throughout the
      // tests).
      let mut worker = create_test_worker();
      let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("cli/tests/002_hello.ts");
      let module_specifier =
        ModuleSpecifier::resolve_url_or_path(&p.to_string_lossy()).unwrap();
      let result =
        block_on(worker.execute_mod_async(&module_specifier, None, false));
      assert!(result.is_ok());
    })
  }
}
