// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use rusty_v8 as v8;

use crate::bindings;
use crate::error::attach_handle_to_error;
use crate::error::generic_error;
use crate::error::AnyError;
use crate::error::ErrWithV8Handle;
use crate::error::JsError;
use crate::module_specifier::ModuleSpecifier;
use crate::modules::ModuleId;
use crate::modules::ModuleLoadId;
use crate::modules::ModuleLoader;
use crate::modules::ModuleMap;
use crate::modules::NoopModuleLoader;
use crate::ops::*;
use crate::Extension;
use crate::OpMiddlewareFn;
use crate::OpPayload;
use crate::OpResult;
use crate::OpState;
use crate::PromiseId;
use futures::channel::mpsc;
use futures::future::poll_fn;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use futures::task::AtomicWaker;
use futures::Future;
use std::any::Any;
use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;
use std::convert::TryFrom;
use std::ffi::c_void;
use std::mem::forget;
use std::option::Option;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Once;
use std::task::Context;
use std::task::Poll;

type PendingOpFuture = Pin<Box<dyn Future<Output = (PromiseId, OpResult)>>>;

pub enum Snapshot {
  Static(&'static [u8]),
  JustCreated(v8::StartupData),
  Boxed(Box<[u8]>),
}

pub type JsErrorCreateFn = dyn Fn(JsError) -> AnyError;

pub type GetErrorClassFn =
  &'static dyn for<'e> Fn(&'e AnyError) -> &'static str;

/// Objects that need to live as long as the isolate
#[derive(Default)]
struct IsolateAllocations {
  near_heap_limit_callback_data:
    Option<(Box<RefCell<dyn Any>>, v8::NearHeapLimitCallback)>,
}

/// A single execution context of JavaScript. Corresponds roughly to the "Web
/// Worker" concept in the DOM. A JsRuntime is a Future that can be used with
/// an event loop (Tokio, async_std).
////
/// The JsRuntime future completes when there is an error or when all
/// pending ops have completed.
///
/// Pending ops are created in JavaScript by calling Deno.core.opAsync(), and in Rust
/// by implementing an async function that takes a serde::Deserialize "control argument"
/// and an optional zero copy buffer, each async Op is tied to a Promise in JavaScript.
pub struct JsRuntime {
  // This is an Option<OwnedIsolate> instead of just OwnedIsolate to workaround
  // an safety issue with SnapshotCreator. See JsRuntime::drop.
  v8_isolate: Option<v8::OwnedIsolate>,
  snapshot_creator: Option<v8::SnapshotCreator>,
  has_snapshotted: bool,
  allocations: IsolateAllocations,
  extensions: Vec<Extension>,
}

struct DynImportModEvaluate {
  load_id: ModuleLoadId,
  module_id: ModuleId,
  promise: v8::Global<v8::Promise>,
  module: v8::Global<v8::Module>,
}

struct ModEvaluate {
  promise: v8::Global<v8::Promise>,
  sender: mpsc::Sender<Result<(), AnyError>>,
}

/// Internal state for JsRuntime which is stored in one of v8::Isolate's
/// embedder slots.
pub(crate) struct JsRuntimeState {
  pub global_context: Option<v8::Global<v8::Context>>,
  pub(crate) js_recv_cb: Option<v8::Global<v8::Function>>,
  pub(crate) js_macrotask_cb: Option<v8::Global<v8::Function>>,
  pub(crate) pending_promise_exceptions:
    HashMap<v8::Global<v8::Promise>, v8::Global<v8::Value>>,
  pending_dyn_mod_evaluate: VecDeque<DynImportModEvaluate>,
  pending_mod_evaluate: Option<ModEvaluate>,
  pub(crate) js_error_create_fn: Rc<JsErrorCreateFn>,
  pub(crate) pending_ops: FuturesUnordered<PendingOpFuture>,
  pub(crate) pending_unref_ops: FuturesUnordered<PendingOpFuture>,
  pub(crate) have_unpolled_ops: bool,
  pub(crate) op_state: Rc<RefCell<OpState>>,
  waker: AtomicWaker,
}

impl Drop for JsRuntime {
  fn drop(&mut self) {
    if let Some(creator) = self.snapshot_creator.take() {
      // TODO(ry): in rusty_v8, `SnapShotCreator::get_owned_isolate()` returns
      // a `struct OwnedIsolate` which is not actually owned, hence the need
      // here to leak the `OwnedIsolate` in order to avoid a double free and
      // the segfault that it causes.
      let v8_isolate = self.v8_isolate.take().unwrap();
      forget(v8_isolate);

      // TODO(ry) V8 has a strange assert which prevents a SnapshotCreator from
      // being deallocated if it hasn't created a snapshot yet.
      // https://github.com/v8/v8/blob/73212783fbd534fac76cc4b66aac899c13f71fc8/src/api.cc#L603
      // If that assert is removed, this if guard could be removed.
      // WARNING: There may be false positive LSAN errors here.
      if self.has_snapshotted {
        drop(creator);
      }
    }
  }
}

fn v8_init(v8_platform: Option<v8::UniquePtr<v8::Platform>>) {
  // Include 10MB ICU data file.
  #[repr(C, align(16))]
  struct IcuData([u8; 10413584]);
  static ICU_DATA: IcuData = IcuData(*include_bytes!("icudtl.dat"));
  v8::icu::set_common_data(&ICU_DATA.0).unwrap();

  let v8_platform = v8_platform
    .unwrap_or_else(v8::new_default_platform)
    .unwrap();
  v8::V8::initialize_platform(v8_platform);
  v8::V8::initialize();

  let flags = concat!(
    // TODO(ry) This makes WASM compile synchronously. Eventually we should
    // remove this to make it work asynchronously too. But that requires getting
    // PumpMessageLoop and RunMicrotasks setup correctly.
    // See https://github.com/denoland/deno/issues/2544
    " --experimental-wasm-threads",
    " --no-wasm-async-compilation",
    " --harmony-top-level-await",
    " --harmony-import-assertions",
    " --no-validate-asm",
  );
  v8::V8::set_flags_from_string(flags);
}

#[derive(Default)]
pub struct RuntimeOptions {
  /// Allows a callback to be set whenever a V8 exception is made. This allows
  /// the caller to wrap the JsError into an error. By default this callback
  /// is set to `JsError::create()`.
  pub js_error_create_fn: Option<Rc<JsErrorCreateFn>>,

  /// Allows to map error type to a string "class" used to represent
  /// error in JavaScript.
  pub get_error_class_fn: Option<GetErrorClassFn>,

  /// Implementation of `ModuleLoader` which will be
  /// called when V8 requests to load ES modules.
  ///
  /// If not provided runtime will error if code being
  /// executed tries to load modules.
  pub module_loader: Option<Rc<dyn ModuleLoader>>,

  /// JsRuntime extensions, not to be confused with ES modules
  /// these are sets of ops and other JS code to be initialized.
  pub extensions: Vec<Extension>,

  /// V8 snapshot that should be loaded on startup.
  ///
  /// Currently can't be used with `will_snapshot`.
  pub startup_snapshot: Option<Snapshot>,

  /// Prepare runtime to take snapshot of loaded code.
  ///
  /// Currently can't be used with `startup_snapshot`.
  pub will_snapshot: bool,

  /// Isolate creation parameters.
  pub create_params: Option<v8::CreateParams>,

  /// V8 platform instance to use. Used when Deno initializes V8
  /// (which it only does once), otherwise it's silenty dropped.
  pub v8_platform: Option<v8::UniquePtr<v8::Platform>>,
}

impl JsRuntime {
  /// Only constructor, configuration is done through `options`.
  pub fn new(mut options: RuntimeOptions) -> Self {
    let v8_platform = options.v8_platform.take();

    static DENO_INIT: Once = Once::new();
    DENO_INIT.call_once(move || v8_init(v8_platform));

    let has_startup_snapshot = options.startup_snapshot.is_some();

    let global_context;
    let (mut isolate, maybe_snapshot_creator) = if options.will_snapshot {
      // TODO(ry) Support loading snapshots before snapshotting.
      assert!(options.startup_snapshot.is_none());
      let mut creator =
        v8::SnapshotCreator::new(Some(&bindings::EXTERNAL_REFERENCES));
      let isolate = unsafe { creator.get_owned_isolate() };
      let mut isolate = JsRuntime::setup_isolate(isolate);
      {
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = bindings::initialize_context(scope);
        global_context = v8::Global::new(scope, context);
        creator.set_default_context(context);
      }
      (isolate, Some(creator))
    } else {
      let mut params = options
        .create_params
        .take()
        .unwrap_or_else(v8::Isolate::create_params)
        .external_references(&**bindings::EXTERNAL_REFERENCES);
      let snapshot_loaded = if let Some(snapshot) = options.startup_snapshot {
        params = match snapshot {
          Snapshot::Static(data) => params.snapshot_blob(data),
          Snapshot::JustCreated(data) => params.snapshot_blob(data),
          Snapshot::Boxed(data) => params.snapshot_blob(data),
        };
        true
      } else {
        false
      };

      let isolate = v8::Isolate::new(params);
      let mut isolate = JsRuntime::setup_isolate(isolate);
      {
        let scope = &mut v8::HandleScope::new(&mut isolate);
        let context = if snapshot_loaded {
          v8::Context::new(scope)
        } else {
          // If no snapshot is provided, we initialize the context with empty
          // main source code and source maps.
          bindings::initialize_context(scope)
        };
        global_context = v8::Global::new(scope, context);
      }
      (isolate, None)
    };

    let loader = options
      .module_loader
      .unwrap_or_else(|| Rc::new(NoopModuleLoader));

    let js_error_create_fn = options
      .js_error_create_fn
      .unwrap_or_else(|| Rc::new(JsError::create));
    let mut op_state = OpState::new();

    if let Some(get_error_class_fn) = options.get_error_class_fn {
      op_state.get_error_class_fn = get_error_class_fn;
    }

    let op_state = Rc::new(RefCell::new(op_state));

    isolate.set_slot(Rc::new(RefCell::new(JsRuntimeState {
      global_context: Some(global_context),
      pending_promise_exceptions: HashMap::new(),
      pending_dyn_mod_evaluate: VecDeque::new(),
      pending_mod_evaluate: None,
      js_recv_cb: None,
      js_macrotask_cb: None,
      js_error_create_fn,
      pending_ops: FuturesUnordered::new(),
      pending_unref_ops: FuturesUnordered::new(),
      op_state: op_state.clone(),
      have_unpolled_ops: false,
      waker: AtomicWaker::new(),
    })));

    let module_map = ModuleMap::new(loader, op_state);
    isolate.set_slot(Rc::new(RefCell::new(module_map)));

    // Add builtins extension
    options
      .extensions
      .insert(0, crate::ops_builtin::init_builtins());

    let mut js_runtime = Self {
      v8_isolate: Some(isolate),
      snapshot_creator: maybe_snapshot_creator,
      has_snapshotted: false,
      allocations: IsolateAllocations::default(),
      extensions: options.extensions,
    };

    // TODO(@AaronO): diff extensions inited in snapshot and those provided
    // for now we assume that snapshot and extensions always match
    if !has_startup_snapshot {
      js_runtime.init_extension_js().unwrap();
    }
    // Init extension ops
    js_runtime.init_extension_ops().unwrap();
    js_runtime.sync_ops_cache();
    // Init async ops callback
    js_runtime.init_recv_cb();

    js_runtime
  }

  pub fn global_context(&mut self) -> v8::Global<v8::Context> {
    let state = Self::state(self.v8_isolate());
    let state = state.borrow();
    state.global_context.clone().unwrap()
  }

  pub fn v8_isolate(&mut self) -> &mut v8::OwnedIsolate {
    self.v8_isolate.as_mut().unwrap()
  }

  pub fn handle_scope(&mut self) -> v8::HandleScope {
    let context = self.global_context();
    v8::HandleScope::with_context(self.v8_isolate(), context)
  }

  fn setup_isolate(mut isolate: v8::OwnedIsolate) -> v8::OwnedIsolate {
    isolate.set_capture_stack_trace_for_uncaught_exceptions(true, 10);
    isolate.set_promise_reject_callback(bindings::promise_reject_callback);
    isolate.set_host_initialize_import_meta_object_callback(
      bindings::host_initialize_import_meta_object_callback,
    );
    isolate.set_host_import_module_dynamically_callback(
      bindings::host_import_module_dynamically_callback,
    );
    isolate
  }

  pub(crate) fn state(isolate: &v8::Isolate) -> Rc<RefCell<JsRuntimeState>> {
    let s = isolate.get_slot::<Rc<RefCell<JsRuntimeState>>>().unwrap();
    s.clone()
  }

  pub(crate) fn module_map(isolate: &v8::Isolate) -> Rc<RefCell<ModuleMap>> {
    let module_map = isolate.get_slot::<Rc<RefCell<ModuleMap>>>().unwrap();
    module_map.clone()
  }

  /// Initializes JS of provided Extensions
  fn init_extension_js(&mut self) -> Result<(), AnyError> {
    // Take extensions to avoid double-borrow
    let mut extensions: Vec<Extension> = std::mem::take(&mut self.extensions);
    for m in extensions.iter_mut() {
      let js_files = m.init_js();
      for (filename, source) in js_files {
        // TODO(@AaronO): use JsRuntime::execute_static() here to move src off heap
        self.execute(filename, source)?;
      }
    }
    // Restore extensions
    self.extensions = extensions;

    Ok(())
  }

  /// Initializes ops of provided Extensions
  fn init_extension_ops(&mut self) -> Result<(), AnyError> {
    let op_state = self.op_state();
    // Take extensions to avoid double-borrow
    let mut extensions: Vec<Extension> = std::mem::take(&mut self.extensions);

    // Middleware
    let middleware: Vec<Box<OpMiddlewareFn>> = extensions
      .iter_mut()
      .filter_map(|e| e.init_middleware())
      .collect();
    // macroware wraps an opfn in all the middleware
    let macroware =
      move |name, opfn| middleware.iter().fold(opfn, |opfn, m| m(name, opfn));

    // Register ops
    for e in extensions.iter_mut() {
      e.init_state(&mut op_state.borrow_mut())?;
      // Register each op after middlewaring it
      let ops = e.init_ops().unwrap_or_default();
      for (name, opfn) in ops {
        self.register_op(name, macroware(name, opfn));
      }
    }
    // Sync ops cache
    self.sync_ops_cache();
    // Restore extensions
    self.extensions = extensions;

    Ok(())
  }

  /// Grabs a reference to core.js' handleAsyncMsgFromRust
  fn init_recv_cb(&mut self) {
    let scope = &mut self.handle_scope();

    // Get Deno.core.handleAsyncMsgFromRust
    let code =
      v8::String::new(scope, "Deno.core.handleAsyncMsgFromRust").unwrap();
    let script = v8::Script::compile(scope, code, None).unwrap();
    let v8_value = script.run(scope).unwrap();

    // Put global handle in state.js_recv_cb
    let state_rc = JsRuntime::state(scope);
    let mut state = state_rc.borrow_mut();
    let cb = v8::Local::<v8::Function>::try_from(v8_value).unwrap();
    state.js_recv_cb.replace(v8::Global::new(scope, cb));
  }

  /// Ensures core.js has the latest op-name to op-id mappings
  pub fn sync_ops_cache(&mut self) {
    self.execute("<anon>", "Deno.core.syncOpsCache()").unwrap()
  }

  /// Returns the runtime's op state, which can be used to maintain ops
  /// and access resources between op calls.
  pub fn op_state(&mut self) -> Rc<RefCell<OpState>> {
    let state_rc = Self::state(self.v8_isolate());
    let state = state_rc.borrow();
    state.op_state.clone()
  }

  /// Executes traditional JavaScript code (traditional = not ES modules)
  ///
  /// The execution takes place on the current global context, so it is possible
  /// to maintain local JS state and invoke this method multiple times.
  ///
  /// `AnyError` can be downcast to a type that exposes additional information
  /// about the V8 exception. By default this type is `JsError`, however it may
  /// be a different type if `RuntimeOptions::js_error_create_fn` has been set.
  pub fn execute(
    &mut self,
    js_filename: &str,
    js_source: &str,
  ) -> Result<(), AnyError> {
    let scope = &mut self.handle_scope();

    let source = v8::String::new(scope, js_source).unwrap();
    let name = v8::String::new(scope, js_filename).unwrap();
    let origin = bindings::script_origin(scope, name);

    let tc_scope = &mut v8::TryCatch::new(scope);

    let script = match v8::Script::compile(tc_scope, source, Some(&origin)) {
      Some(script) => script,
      None => {
        let exception = tc_scope.exception().unwrap();
        return exception_to_err_result(tc_scope, exception, false);
      }
    };

    match script.run(tc_scope) {
      Some(_) => Ok(()),
      None => {
        assert!(tc_scope.has_caught());
        let exception = tc_scope.exception().unwrap();
        exception_to_err_result(tc_scope, exception, false)
      }
    }
  }

  /// Takes a snapshot. The isolate should have been created with will_snapshot
  /// set to true.
  ///
  /// `AnyError` can be downcast to a type that exposes additional information
  /// about the V8 exception. By default this type is `JsError`, however it may
  /// be a different type if `RuntimeOptions::js_error_create_fn` has been set.
  pub fn snapshot(&mut self) -> v8::StartupData {
    assert!(self.snapshot_creator.is_some());
    let state = Self::state(self.v8_isolate());

    // Note: create_blob() method must not be called from within a HandleScope.
    // TODO(piscisaureus): The rusty_v8 type system should enforce this.
    state.borrow_mut().global_context.take();

    // Overwrite existing ModuleMap to drop v8::Global handles
    self
      .v8_isolate()
      .set_slot(Rc::new(RefCell::new(ModuleMap::new(
        Rc::new(NoopModuleLoader),
        state.borrow().op_state.clone(),
      ))));
    // Drop other v8::Global handles before snapshotting
    std::mem::take(&mut state.borrow_mut().js_recv_cb);

    let snapshot_creator = self.snapshot_creator.as_mut().unwrap();
    let snapshot = snapshot_creator
      .create_blob(v8::FunctionCodeHandling::Keep)
      .unwrap();
    self.has_snapshotted = true;

    snapshot
  }

  /// Registers an op that can be called from JavaScript.
  ///
  /// The _op_ mechanism allows to expose Rust functions to the JS runtime,
  /// which can be called using the provided `name`.
  ///
  /// This function provides byte-level bindings. To pass data via JSON, the
  /// following functions can be passed as an argument for `op_fn`:
  /// * [op_sync()](fn.op_sync.html)
  /// * [op_async()](fn.op_async.html)
  pub fn register_op<F>(&mut self, name: &str, op_fn: F) -> OpId
  where
    F: Fn(Rc<RefCell<OpState>>, OpPayload) -> Op + 'static,
  {
    Self::state(self.v8_isolate())
      .borrow_mut()
      .op_state
      .borrow_mut()
      .op_table
      .register_op(name, op_fn)
  }

  /// Registers a callback on the isolate when the memory limits are approached.
  /// Use this to prevent V8 from crashing the process when reaching the limit.
  ///
  /// Calls the closure with the current heap limit and the initial heap limit.
  /// The return value of the closure is set as the new limit.
  pub fn add_near_heap_limit_callback<C>(&mut self, cb: C)
  where
    C: FnMut(usize, usize) -> usize + 'static,
  {
    let boxed_cb = Box::new(RefCell::new(cb));
    let data = boxed_cb.as_ptr() as *mut c_void;

    let prev = self
      .allocations
      .near_heap_limit_callback_data
      .replace((boxed_cb, near_heap_limit_callback::<C>));
    if let Some((_, prev_cb)) = prev {
      self
        .v8_isolate()
        .remove_near_heap_limit_callback(prev_cb, 0);
    }

    self
      .v8_isolate()
      .add_near_heap_limit_callback(near_heap_limit_callback::<C>, data);
  }

  pub fn remove_near_heap_limit_callback(&mut self, heap_limit: usize) {
    if let Some((_, cb)) = self.allocations.near_heap_limit_callback_data.take()
    {
      self
        .v8_isolate()
        .remove_near_heap_limit_callback(cb, heap_limit);
    }
  }

  /// Runs event loop to completion
  ///
  /// This future resolves when:
  ///  - there are no more pending dynamic imports
  ///  - there are no more pending ops
  pub async fn run_event_loop(&mut self) -> Result<(), AnyError> {
    poll_fn(|cx| self.poll_event_loop(cx)).await
  }

  /// Runs a single tick of event loop
  pub fn poll_event_loop(
    &mut self,
    cx: &mut Context,
  ) -> Poll<Result<(), AnyError>> {
    let state_rc = Self::state(self.v8_isolate());
    let module_map_rc = Self::module_map(self.v8_isolate());
    {
      let state = state_rc.borrow();
      state.waker.register(cx.waker());
    }

    // Ops
    {
      let async_responses = self.poll_pending_ops(cx);
      self.async_op_response(async_responses)?;
      self.drain_macrotasks()?;
      self.check_promise_exceptions()?;
    }

    // Dynamic module loading - ie. modules loaded using "import()"
    {
      let poll_imports = self.prepare_dyn_imports(cx)?;
      assert!(poll_imports.is_ready());

      let poll_imports = self.poll_dyn_imports(cx)?;
      assert!(poll_imports.is_ready());

      self.evaluate_dyn_imports();

      self.check_promise_exceptions()?;
    }

    // Top level module
    self.evaluate_pending_module();

    let state = state_rc.borrow();
    let module_map = module_map_rc.borrow();

    let has_pending_ops = !state.pending_ops.is_empty();

    let has_pending_dyn_imports = module_map.has_pending_dynamic_imports();
    let has_pending_dyn_module_evaluation =
      !state.pending_dyn_mod_evaluate.is_empty();
    let has_pending_module_evaluation = state.pending_mod_evaluate.is_some();

    if !has_pending_ops
      && !has_pending_dyn_imports
      && !has_pending_dyn_module_evaluation
      && !has_pending_module_evaluation
    {
      return Poll::Ready(Ok(()));
    }

    // Check if more async ops have been dispatched
    // during this turn of event loop.
    if state.have_unpolled_ops {
      state.waker.wake();
    }

    if has_pending_module_evaluation {
      if has_pending_ops
        || has_pending_dyn_imports
        || has_pending_dyn_module_evaluation
      {
        // pass, will be polled again
      } else {
        let msg = "Module evaluation is still pending but there are no pending ops or dynamic imports. This situation is often caused by unresolved promise.";
        return Poll::Ready(Err(generic_error(msg)));
      }
    }

    if has_pending_dyn_module_evaluation {
      if has_pending_ops || has_pending_dyn_imports {
        // pass, will be polled again
      } else {
        let mut msg = "Dynamically imported module evaluation is still pending but there are no pending ops. This situation is often caused by unresolved promise.
Pending dynamic modules:\n".to_string();
        for pending_evaluate in &state.pending_dyn_mod_evaluate {
          let module_info = module_map
            .get_info_by_id(&pending_evaluate.module_id)
            .unwrap();
          msg.push_str(&format!("- {}", module_info.name.as_str()));
        }
        return Poll::Ready(Err(generic_error(msg)));
      }
    }

    Poll::Pending
  }
}

extern "C" fn near_heap_limit_callback<F>(
  data: *mut c_void,
  current_heap_limit: usize,
  initial_heap_limit: usize,
) -> usize
where
  F: FnMut(usize, usize) -> usize,
{
  let callback = unsafe { &mut *(data as *mut F) };
  callback(current_heap_limit, initial_heap_limit)
}

impl JsRuntimeState {
  /// Called by `bindings::host_import_module_dynamically_callback`
  /// after initiating new dynamic import load.
  pub fn notify_new_dynamic_import(&mut self) {
    // Notify event loop to poll again soon.
    self.waker.wake();
  }
}

pub(crate) fn exception_to_err_result<'s, T>(
  scope: &mut v8::HandleScope<'s>,
  exception: v8::Local<v8::Value>,
  in_promise: bool,
) -> Result<T, AnyError> {
  let is_terminating_exception = scope.is_execution_terminating();
  let mut exception = exception;

  if is_terminating_exception {
    // TerminateExecution was called. Cancel exception termination so that the
    // exception can be created..
    scope.cancel_terminate_execution();

    // Maybe make a new exception object.
    if exception.is_null_or_undefined() {
      let message = v8::String::new(scope, "execution terminated").unwrap();
      exception = v8::Exception::error(scope, message);
    }
  }

  let mut js_error = JsError::from_v8_exception(scope, exception);
  if in_promise {
    js_error.message = format!(
      "Uncaught (in promise) {}",
      js_error.message.trim_start_matches("Uncaught ")
    );
  }

  let state_rc = JsRuntime::state(scope);
  let state = state_rc.borrow();
  let js_error = (state.js_error_create_fn)(js_error);

  if is_terminating_exception {
    // Re-enable exception termination.
    scope.terminate_execution();
  }

  Err(js_error)
}

// Related to module loading
impl JsRuntime {
  pub(crate) fn instantiate_module(
    &mut self,
    id: ModuleId,
  ) -> Result<(), AnyError> {
    let module_map_rc = Self::module_map(self.v8_isolate());
    let scope = &mut self.handle_scope();
    let tc_scope = &mut v8::TryCatch::new(scope);

    let module = module_map_rc
      .borrow()
      .get_handle(id)
      .map(|handle| v8::Local::new(tc_scope, handle))
      .expect("ModuleInfo not found");

    if module.get_status() == v8::ModuleStatus::Errored {
      let exception = module.get_exception();
      let err = exception_to_err_result(tc_scope, exception, false)
        .map_err(|err| attach_handle_to_error(tc_scope, err, exception));
      return err;
    }

    // IMPORTANT: No borrows to `ModuleMap` can be held at this point because
    // `module_resolve_callback` will be calling into `ModuleMap` from within
    // the isolate.
    let instantiate_result =
      module.instantiate_module(tc_scope, bindings::module_resolve_callback);

    if instantiate_result.is_none() {
      let exception = tc_scope.exception().unwrap();
      let err = exception_to_err_result(tc_scope, exception, false)
        .map_err(|err| attach_handle_to_error(tc_scope, err, exception));
      return err;
    }

    Ok(())
  }

  fn dynamic_import_module_evaluate(
    &mut self,
    load_id: ModuleLoadId,
    id: ModuleId,
  ) -> Result<(), AnyError> {
    let state_rc = Self::state(self.v8_isolate());
    let module_map_rc = Self::module_map(self.v8_isolate());

    let module_handle = module_map_rc
      .borrow()
      .get_handle(id)
      .expect("ModuleInfo not found");

    let status = {
      let scope = &mut self.handle_scope();
      let module = module_handle.get(scope);
      module.get_status()
    };

    match status {
      v8::ModuleStatus::Instantiated | v8::ModuleStatus::Evaluated => {}
      _ => return Ok(()),
    }

    // IMPORTANT: Top-level-await is enabled, which means that return value
    // of module evaluation is a promise.
    //
    // This promise is internal, and not the same one that gets returned to
    // the user. We add an empty `.catch()` handler so that it does not result
    // in an exception if it rejects. That will instead happen for the other
    // promise if not handled by the user.
    //
    // For more details see:
    // https://github.com/denoland/deno/issues/4908
    // https://v8.dev/features/top-level-await#module-execution-order
    let scope = &mut self.handle_scope();
    let module = v8::Local::new(scope, &module_handle);
    let maybe_value = module.evaluate(scope);

    // Update status after evaluating.
    let status = module.get_status();

    if let Some(value) = maybe_value {
      assert!(
        status == v8::ModuleStatus::Evaluated
          || status == v8::ModuleStatus::Errored
      );
      let promise = v8::Local::<v8::Promise>::try_from(value)
        .expect("Expected to get promise as module evaluation result");
      let empty_fn = |_scope: &mut v8::HandleScope,
                      _args: v8::FunctionCallbackArguments,
                      _rv: v8::ReturnValue| {};
      let empty_fn = v8::FunctionTemplate::new(scope, empty_fn);
      let empty_fn = empty_fn.get_function(scope).unwrap();
      promise.catch(scope, empty_fn);
      let mut state = state_rc.borrow_mut();
      let promise_global = v8::Global::new(scope, promise);
      let module_global = v8::Global::new(scope, module);

      let dyn_import_mod_evaluate = DynImportModEvaluate {
        load_id,
        module_id: id,
        promise: promise_global,
        module: module_global,
      };

      state
        .pending_dyn_mod_evaluate
        .push_back(dyn_import_mod_evaluate);
    } else {
      assert!(status == v8::ModuleStatus::Errored);
    }

    Ok(())
  }

  // TODO(bartlomieju): make it return `ModuleEvaluationFuture`?
  /// Evaluates an already instantiated ES module.
  ///
  /// Returns a receiver handle that resolves when module promise resolves.
  /// Implementors must manually call `run_event_loop()` to drive module
  /// evaluation future.
  ///
  /// `AnyError` can be downcast to a type that exposes additional information
  /// about the V8 exception. By default this type is `JsError`, however it may
  /// be a different type if `RuntimeOptions::js_error_create_fn` has been set.
  ///
  /// This function panics if module has not been instantiated.
  pub fn mod_evaluate(
    &mut self,
    id: ModuleId,
  ) -> mpsc::Receiver<Result<(), AnyError>> {
    let state_rc = Self::state(self.v8_isolate());
    let module_map_rc = Self::module_map(self.v8_isolate());
    let scope = &mut self.handle_scope();

    let module = module_map_rc
      .borrow()
      .get_handle(id)
      .map(|handle| v8::Local::new(scope, handle))
      .expect("ModuleInfo not found");
    let mut status = module.get_status();
    assert_eq!(status, v8::ModuleStatus::Instantiated);

    let (sender, receiver) = mpsc::channel(1);

    // IMPORTANT: Top-level-await is enabled, which means that return value
    // of module evaluation is a promise.
    //
    // Because that promise is created internally by V8, when error occurs during
    // module evaluation the promise is rejected, and since the promise has no rejection
    // handler it will result in call to `bindings::promise_reject_callback` adding
    // the promise to pending promise rejection table - meaning JsRuntime will return
    // error on next poll().
    //
    // This situation is not desirable as we want to manually return error at the
    // end of this function to handle it further. It means we need to manually
    // remove this promise from pending promise rejection table.
    //
    // For more details see:
    // https://github.com/denoland/deno/issues/4908
    // https://v8.dev/features/top-level-await#module-execution-order
    let maybe_value = module.evaluate(scope);

    // Update status after evaluating.
    status = module.get_status();

    if let Some(value) = maybe_value {
      assert!(
        status == v8::ModuleStatus::Evaluated
          || status == v8::ModuleStatus::Errored
      );
      let promise = v8::Local::<v8::Promise>::try_from(value)
        .expect("Expected to get promise as module evaluation result");
      let promise_global = v8::Global::new(scope, promise);
      let mut state = state_rc.borrow_mut();
      state.pending_promise_exceptions.remove(&promise_global);
      let promise_global = v8::Global::new(scope, promise);
      assert!(
        state.pending_mod_evaluate.is_none(),
        "There is already pending top level module evaluation"
      );

      state.pending_mod_evaluate = Some(ModEvaluate {
        promise: promise_global,
        sender,
      });
      scope.perform_microtask_checkpoint();
    } else {
      assert!(status == v8::ModuleStatus::Errored);
    }

    receiver
  }

  fn dynamic_import_reject(&mut self, id: ModuleLoadId, err: AnyError) {
    let module_map_rc = Self::module_map(self.v8_isolate());
    let scope = &mut self.handle_scope();

    let resolver_handle = module_map_rc
      .borrow_mut()
      .dynamic_import_map
      .remove(&id)
      .expect("Invalid dynamic import id");
    let resolver = resolver_handle.get(scope);

    let exception = err
      .downcast_ref::<ErrWithV8Handle>()
      .map(|err| err.get_handle(scope))
      .unwrap_or_else(|| {
        let message = err.to_string();
        let message = v8::String::new(scope, &message).unwrap();
        v8::Exception::type_error(scope, message)
      });

    // IMPORTANT: No borrows to `ModuleMap` can be held at this point because
    // rejecting the promise might initiate another `import()` which will
    // in turn call `bindings::host_import_module_dynamically_callback` which
    // will reach into `ModuleMap` from within the isolate.
    resolver.reject(scope, exception).unwrap();
    scope.perform_microtask_checkpoint();
  }

  fn dynamic_import_resolve(&mut self, id: ModuleLoadId, mod_id: ModuleId) {
    let module_map_rc = Self::module_map(self.v8_isolate());
    let scope = &mut self.handle_scope();

    let resolver_handle = module_map_rc
      .borrow_mut()
      .dynamic_import_map
      .remove(&id)
      .expect("Invalid dynamic import id");
    let resolver = resolver_handle.get(scope);

    let module = {
      module_map_rc
        .borrow()
        .get_handle(mod_id)
        .map(|handle| v8::Local::new(scope, handle))
        .expect("Dyn import module info not found")
    };
    // Resolution success
    assert_eq!(module.get_status(), v8::ModuleStatus::Evaluated);

    // IMPORTANT: No borrows to `ModuleMap` can be held at this point because
    // resolving the promise might initiate another `import()` which will
    // in turn call `bindings::host_import_module_dynamically_callback` which
    // will reach into `ModuleMap` from within the isolate.
    let module_namespace = module.get_module_namespace();
    resolver.resolve(scope, module_namespace).unwrap();
    scope.perform_microtask_checkpoint();
  }

  fn prepare_dyn_imports(
    &mut self,
    cx: &mut Context,
  ) -> Poll<Result<(), AnyError>> {
    let module_map_rc = Self::module_map(self.v8_isolate());

    if module_map_rc.borrow().preparing_dynamic_imports.is_empty() {
      return Poll::Ready(Ok(()));
    }

    loop {
      let poll_result = module_map_rc
        .borrow_mut()
        .preparing_dynamic_imports
        .poll_next_unpin(cx);

      if let Poll::Ready(Some(prepare_poll)) = poll_result {
        let dyn_import_id = prepare_poll.0;
        let prepare_result = prepare_poll.1;

        match prepare_result {
          Ok(load) => {
            module_map_rc
              .borrow_mut()
              .pending_dynamic_imports
              .push(load.into_future());
          }
          Err(err) => {
            self.dynamic_import_reject(dyn_import_id, err);
          }
        }
        // Continue polling for more prepared dynamic imports.
        continue;
      }

      // There are no active dynamic import loads, or none are ready.
      return Poll::Ready(Ok(()));
    }
  }

  fn poll_dyn_imports(
    &mut self,
    cx: &mut Context,
  ) -> Poll<Result<(), AnyError>> {
    let module_map_rc = Self::module_map(self.v8_isolate());

    if module_map_rc.borrow().pending_dynamic_imports.is_empty() {
      return Poll::Ready(Ok(()));
    }

    loop {
      let poll_result = module_map_rc
        .borrow_mut()
        .pending_dynamic_imports
        .poll_next_unpin(cx);

      if let Poll::Ready(Some(load_stream_poll)) = poll_result {
        let maybe_result = load_stream_poll.0;
        let mut load = load_stream_poll.1;
        let dyn_import_id = load.id;

        if let Some(load_stream_result) = maybe_result {
          match load_stream_result {
            Ok(info) => {
              // A module (not necessarily the one dynamically imported) has been
              // fetched. Create and register it, and if successful, poll for the
              // next recursive-load event related to this dynamic import.
              let register_result =
                module_map_rc.borrow_mut().register_during_load(
                  &mut self.handle_scope(),
                  info,
                  &mut load,
                );

              match register_result {
                Ok(()) => {
                  // Keep importing until it's fully drained
                  module_map_rc
                    .borrow_mut()
                    .pending_dynamic_imports
                    .push(load.into_future());
                }
                Err(err) => self.dynamic_import_reject(dyn_import_id, err),
              }
            }
            Err(err) => {
              // A non-javascript error occurred; this could be due to a an invalid
              // module specifier, or a problem with the source map, or a failure
              // to fetch the module source code.
              self.dynamic_import_reject(dyn_import_id, err)
            }
          }
        } else {
          // The top-level module from a dynamic import has been instantiated.
          // Load is done.
          let module_id = load.expect_finished();
          let result = self.instantiate_module(module_id);
          if let Err(err) = result {
            self.dynamic_import_reject(dyn_import_id, err);
          }
          self.dynamic_import_module_evaluate(dyn_import_id, module_id)?;
        }

        // Continue polling for more ready dynamic imports.
        continue;
      }

      // There are no active dynamic import loads, or none are ready.
      return Poll::Ready(Ok(()));
    }
  }

  /// "deno_core" runs V8 with "--harmony-top-level-await"
  /// flag on - it means that each module evaluation returns a promise
  /// from V8.
  ///
  /// This promise resolves after all dependent modules have also
  /// resolved. Each dependent module may perform calls to "import()" and APIs
  /// using async ops will add futures to the runtime's event loop.
  /// It means that the promise returned from module evaluation will
  /// resolve only after all futures in the event loop are done.
  ///
  /// Thus during turn of event loop we need to check if V8 has
  /// resolved or rejected the promise. If the promise is still pending
  /// then another turn of event loop must be performed.
  fn evaluate_pending_module(&mut self) {
    let state_rc = Self::state(self.v8_isolate());

    let maybe_module_evaluation =
      state_rc.borrow_mut().pending_mod_evaluate.take();

    if maybe_module_evaluation.is_none() {
      return;
    }

    let module_evaluation = maybe_module_evaluation.unwrap();
    let scope = &mut self.handle_scope();

    let promise = module_evaluation.promise.get(scope);
    let mut sender = module_evaluation.sender.clone();
    let promise_state = promise.state();

    match promise_state {
      v8::PromiseState::Pending => {
        // NOTE: `poll_event_loop` will decide if
        // runtime would be woken soon
        state_rc.borrow_mut().pending_mod_evaluate = Some(module_evaluation);
      }
      v8::PromiseState::Fulfilled => {
        scope.perform_microtask_checkpoint();
        // Receiver end might have been already dropped, ignore the result
        let _ = sender.try_send(Ok(()));
      }
      v8::PromiseState::Rejected => {
        let exception = promise.result(scope);
        scope.perform_microtask_checkpoint();
        let err1 = exception_to_err_result::<()>(scope, exception, false)
          .map_err(|err| attach_handle_to_error(scope, err, exception))
          .unwrap_err();
        // Receiver end might have been already dropped, ignore the result
        let _ = sender.try_send(Err(err1));
      }
    }
  }

  fn evaluate_dyn_imports(&mut self) {
    let state_rc = Self::state(self.v8_isolate());

    loop {
      let maybe_pending_dyn_evaluate =
        state_rc.borrow_mut().pending_dyn_mod_evaluate.pop_front();

      if maybe_pending_dyn_evaluate.is_none() {
        break;
      }

      let maybe_result = {
        let scope = &mut self.handle_scope();
        let pending_dyn_evaluate = maybe_pending_dyn_evaluate.unwrap();

        let module_id = pending_dyn_evaluate.module_id;
        let promise = pending_dyn_evaluate.promise.get(scope);
        let _module = pending_dyn_evaluate.module.get(scope);
        let promise_state = promise.state();

        match promise_state {
          v8::PromiseState::Pending => {
            state_rc
              .borrow_mut()
              .pending_dyn_mod_evaluate
              .push_back(pending_dyn_evaluate);
            None
          }
          v8::PromiseState::Fulfilled => {
            Some(Ok((pending_dyn_evaluate.load_id, module_id)))
          }
          v8::PromiseState::Rejected => {
            let exception = promise.result(scope);
            let err1 = exception_to_err_result::<()>(scope, exception, false)
              .map_err(|err| attach_handle_to_error(scope, err, exception))
              .unwrap_err();
            Some(Err((pending_dyn_evaluate.load_id, err1)))
          }
        }
      };

      if let Some(result) = maybe_result {
        match result {
          Ok((dyn_import_id, module_id)) => {
            self.dynamic_import_resolve(dyn_import_id, module_id);
          }
          Err((dyn_import_id, err1)) => {
            self.dynamic_import_reject(dyn_import_id, err1);
          }
        }
      } else {
        break;
      }
    }
  }

  /// Asynchronously load specified module and all of its dependencies
  ///
  /// User must call `JsRuntime::mod_evaluate` with returned `ModuleId`
  /// manually after load is finished.
  pub async fn load_module(
    &mut self,
    specifier: &ModuleSpecifier,
    code: Option<String>,
  ) -> Result<ModuleId, AnyError> {
    let module_map_rc = Self::module_map(self.v8_isolate());

    let load = module_map_rc.borrow().load_main(specifier.as_str(), code);

    let (_load_id, prepare_result) = load.prepare().await;

    let mut load = prepare_result?;

    while let Some(info_result) = load.next().await {
      let info = info_result?;
      let scope = &mut self.handle_scope();
      module_map_rc
        .borrow_mut()
        .register_during_load(scope, info, &mut load)?;
    }

    let root_id = load.expect_finished();
    self.instantiate_module(root_id).map(|_| root_id)
  }

  fn poll_pending_ops(
    &mut self,
    cx: &mut Context,
  ) -> Vec<(PromiseId, OpResult)> {
    let state_rc = Self::state(self.v8_isolate());
    let mut async_responses: Vec<(PromiseId, OpResult)> = Vec::new();

    let mut state = state_rc.borrow_mut();

    // Now handle actual ops.
    state.have_unpolled_ops = false;

    loop {
      let pending_r = state.pending_ops.poll_next_unpin(cx);
      match pending_r {
        Poll::Ready(None) => break,
        Poll::Pending => break,
        Poll::Ready(Some((promise_id, resp))) => {
          async_responses.push((promise_id, resp));
        }
      };
    }

    loop {
      let unref_r = state.pending_unref_ops.poll_next_unpin(cx);
      match unref_r {
        Poll::Ready(None) => break,
        Poll::Pending => break,
        Poll::Ready(Some((promise_id, resp))) => {
          async_responses.push((promise_id, resp));
        }
      };
    }

    async_responses
  }

  fn check_promise_exceptions(&mut self) -> Result<(), AnyError> {
    let state_rc = Self::state(self.v8_isolate());
    let mut state = state_rc.borrow_mut();

    if state.pending_promise_exceptions.is_empty() {
      return Ok(());
    }

    let key = {
      state
        .pending_promise_exceptions
        .keys()
        .next()
        .unwrap()
        .clone()
    };
    let handle = state.pending_promise_exceptions.remove(&key).unwrap();
    drop(state);

    let scope = &mut self.handle_scope();
    let exception = v8::Local::new(scope, handle);
    exception_to_err_result(scope, exception, true)
  }

  // Send finished responses to JS
  fn async_op_response(
    &mut self,
    async_responses: Vec<(PromiseId, OpResult)>,
  ) -> Result<(), AnyError> {
    let state_rc = Self::state(self.v8_isolate());

    let async_responses_size = async_responses.len();
    if async_responses_size == 0 {
      return Ok(());
    }

    let js_recv_cb_handle = state_rc.borrow().js_recv_cb.clone().unwrap();

    let scope = &mut self.handle_scope();

    // We return async responses to JS in unbounded batches (may change),
    // each batch is a flat vector of tuples:
    // `[promise_id1, op_result1, promise_id2, op_result2, ...]`
    // promise_id is a simple integer, op_result is an ops::OpResult
    // which contains a value OR an error, encoded as a tuple.
    // This batch is received in JS via the special `arguments` variable
    // and then each tuple is used to resolve or reject promises
    let mut args: Vec<v8::Local<v8::Value>> =
      Vec::with_capacity(2 * async_responses_size);
    for overflown_response in async_responses {
      let (promise_id, resp) = overflown_response;
      args.push(v8::Integer::new(scope, promise_id as i32).into());
      args.push(resp.to_v8(scope).unwrap());
    }

    let tc_scope = &mut v8::TryCatch::new(scope);
    let js_recv_cb = js_recv_cb_handle.get(tc_scope);
    let this = v8::undefined(tc_scope).into();
    js_recv_cb.call(tc_scope, this, args.as_slice());

    match tc_scope.exception() {
      None => Ok(()),
      Some(exception) => exception_to_err_result(tc_scope, exception, false),
    }
  }

  fn drain_macrotasks(&mut self) -> Result<(), AnyError> {
    let js_macrotask_cb_handle =
      match &Self::state(self.v8_isolate()).borrow().js_macrotask_cb {
        Some(handle) => handle.clone(),
        None => return Ok(()),
      };

    let scope = &mut self.handle_scope();
    let js_macrotask_cb = js_macrotask_cb_handle.get(scope);

    // Repeatedly invoke macrotask callback until it returns true (done),
    // such that ready microtasks would be automatically run before
    // next macrotask is processed.
    let tc_scope = &mut v8::TryCatch::new(scope);
    let this = v8::undefined(tc_scope).into();
    loop {
      let is_done = js_macrotask_cb.call(tc_scope, this, &[]);

      if let Some(exception) = tc_scope.exception() {
        return exception_to_err_result(tc_scope, exception, false);
      }

      let is_done = is_done.unwrap();
      if is_done.is_true() {
        break;
      }
    }

    Ok(())
  }
}

#[cfg(test)]
pub mod tests {
  use super::*;
  use crate::error::custom_error;
  use crate::modules::ModuleSourceFuture;
  use crate::op_sync;
  use crate::ZeroCopyBuf;
  use futures::future::lazy;
  use std::ops::FnOnce;
  use std::rc::Rc;
  use std::sync::atomic::{AtomicUsize, Ordering};
  use std::sync::Arc;

  pub fn run_in_task<F>(f: F)
  where
    F: FnOnce(&mut Context) + Send + 'static,
  {
    futures::executor::block_on(lazy(move |cx| f(cx)));
  }

  enum Mode {
    Async,
    AsyncZeroCopy(bool),
  }

  struct TestState {
    mode: Mode,
    dispatch_count: Arc<AtomicUsize>,
  }

  fn dispatch(rc_op_state: Rc<RefCell<OpState>>, payload: OpPayload) -> Op {
    let rc_op_state2 = rc_op_state.clone();
    let op_state_ = rc_op_state2.borrow();
    let test_state = op_state_.borrow::<TestState>();
    test_state.dispatch_count.fetch_add(1, Ordering::Relaxed);
    let (control, buf): (u8, Option<ZeroCopyBuf>) =
      payload.deserialize().unwrap();
    match test_state.mode {
      Mode::Async => {
        assert_eq!(control, 42);
        let resp = (0, serialize_op_result(Ok(43), rc_op_state));
        Op::Async(Box::pin(futures::future::ready(resp)))
      }
      Mode::AsyncZeroCopy(has_buffer) => {
        assert_eq!(buf.is_some(), has_buffer);
        if let Some(buf) = buf {
          assert_eq!(buf.len(), 1);
        }

        let resp = serialize_op_result(Ok(43), rc_op_state);
        Op::Async(Box::pin(futures::future::ready((0, resp))))
      }
    }
  }

  fn setup(mode: Mode) -> (JsRuntime, Arc<AtomicUsize>) {
    let dispatch_count = Arc::new(AtomicUsize::new(0));
    let mut runtime = JsRuntime::new(Default::default());
    let op_state = runtime.op_state();
    op_state.borrow_mut().put(TestState {
      mode,
      dispatch_count: dispatch_count.clone(),
    });

    runtime.register_op("op_test", dispatch);
    runtime.sync_ops_cache();

    runtime
      .execute(
        "setup.js",
        r#"
        function assert(cond) {
          if (!cond) {
            throw Error("assert");
          }
        }
        "#,
      )
      .unwrap();
    assert_eq!(dispatch_count.load(Ordering::Relaxed), 0);
    (runtime, dispatch_count)
  }

  #[test]
  fn test_dispatch() {
    let (mut runtime, dispatch_count) = setup(Mode::Async);
    runtime
      .execute(
        "filename.js",
        r#"
        let control = 42;
        Deno.core.opAsync("op_test", control);
        async function main() {
          Deno.core.opAsync("op_test", control);
        }
        main();
        "#,
      )
      .unwrap();
    assert_eq!(dispatch_count.load(Ordering::Relaxed), 2);
  }

  #[test]
  fn test_dispatch_no_zero_copy_buf() {
    let (mut runtime, dispatch_count) = setup(Mode::AsyncZeroCopy(false));
    runtime
      .execute(
        "filename.js",
        r#"
        Deno.core.opAsync("op_test");
        "#,
      )
      .unwrap();
    assert_eq!(dispatch_count.load(Ordering::Relaxed), 1);
  }

  #[test]
  fn test_dispatch_stack_zero_copy_bufs() {
    let (mut runtime, dispatch_count) = setup(Mode::AsyncZeroCopy(true));
    runtime
      .execute(
        "filename.js",
        r#"
        let zero_copy_a = new Uint8Array([0]);
        Deno.core.opAsync("op_test", null, zero_copy_a);
        "#,
      )
      .unwrap();
    assert_eq!(dispatch_count.load(Ordering::Relaxed), 1);
  }

  #[test]
  fn terminate_execution() {
    let (mut isolate, _dispatch_count) = setup(Mode::Async);
    // TODO(piscisaureus): in rusty_v8, the `thread_safe_handle()` method
    // should not require a mutable reference to `struct rusty_v8::Isolate`.
    let v8_isolate_handle = isolate.v8_isolate().thread_safe_handle();

    let terminator_thread = std::thread::spawn(move || {
      // allow deno to boot and run
      std::thread::sleep(std::time::Duration::from_millis(100));

      // terminate execution
      let ok = v8_isolate_handle.terminate_execution();
      assert!(ok);
    });

    // Rn an infinite loop, which should be terminated.
    match isolate.execute("infinite_loop.js", "for(;;) {}") {
      Ok(_) => panic!("execution should be terminated"),
      Err(e) => {
        assert_eq!(e.to_string(), "Uncaught Error: execution terminated")
      }
    };

    // Cancel the execution-terminating exception in order to allow script
    // execution again.
    let ok = isolate.v8_isolate().cancel_terminate_execution();
    assert!(ok);

    // Verify that the isolate usable again.
    isolate
      .execute("simple.js", "1 + 1")
      .expect("execution should be possible again");

    terminator_thread.join().unwrap();
  }

  #[test]
  fn dangling_shared_isolate() {
    let v8_isolate_handle = {
      // isolate is dropped at the end of this block
      let (mut runtime, _dispatch_count) = setup(Mode::Async);
      // TODO(piscisaureus): in rusty_v8, the `thread_safe_handle()` method
      // should not require a mutable reference to `struct rusty_v8::Isolate`.
      runtime.v8_isolate().thread_safe_handle()
    };

    // this should not SEGFAULT
    v8_isolate_handle.terminate_execution();
  }

  #[test]
  fn test_pre_dispatch() {
    run_in_task(|mut cx| {
      let (mut runtime, _dispatch_count) = setup(Mode::Async);
      runtime
        .execute(
          "bad_op_id.js",
          r#"
          let thrown;
          try {
            Deno.core.opSync(100);
          } catch (e) {
            thrown = e;
          }
          assert(String(thrown) === "TypeError: Unknown op id: 100");
         "#,
        )
        .unwrap();
      if let Poll::Ready(Err(_)) = runtime.poll_event_loop(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn syntax_error() {
    let mut runtime = JsRuntime::new(Default::default());
    let src = "hocuspocus(";
    let r = runtime.execute("i.js", src);
    let e = r.unwrap_err();
    let js_error = e.downcast::<JsError>().unwrap();
    assert_eq!(js_error.end_column, Some(11));
  }

  #[test]
  fn test_encode_decode() {
    run_in_task(|mut cx| {
      let (mut runtime, _dispatch_count) = setup(Mode::Async);
      runtime
        .execute(
          "encode_decode_test.js",
          include_str!("encode_decode_test.js"),
        )
        .unwrap();
      if let Poll::Ready(Err(_)) = runtime.poll_event_loop(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn test_serialize_deserialize() {
    run_in_task(|mut cx| {
      let (mut runtime, _dispatch_count) = setup(Mode::Async);
      runtime
        .execute(
          "serialize_deserialize_test.js",
          include_str!("serialize_deserialize_test.js"),
        )
        .unwrap();
      if let Poll::Ready(Err(_)) = runtime.poll_event_loop(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn test_error_builder() {
    fn op_err(
      _: &mut OpState,
      _: (),
      _: Option<ZeroCopyBuf>,
    ) -> Result<(), AnyError> {
      Err(custom_error("DOMExceptionOperationError", "abc"))
    }

    pub fn get_error_class_name(_: &AnyError) -> &'static str {
      "DOMExceptionOperationError"
    }

    run_in_task(|mut cx| {
      let mut runtime = JsRuntime::new(RuntimeOptions {
        get_error_class_fn: Some(&get_error_class_name),
        ..Default::default()
      });
      runtime.register_op("op_err", op_sync(op_err));
      runtime.sync_ops_cache();
      runtime
        .execute(
          "error_builder_test.js",
          include_str!("error_builder_test.js"),
        )
        .unwrap();
      if let Poll::Ready(Err(_)) = runtime.poll_event_loop(&mut cx) {
        unreachable!();
      }
    });
  }

  #[test]
  fn will_snapshot() {
    let snapshot = {
      let mut runtime = JsRuntime::new(RuntimeOptions {
        will_snapshot: true,
        ..Default::default()
      });
      runtime.execute("a.js", "a = 1 + 2").unwrap();
      runtime.snapshot()
    };

    let snapshot = Snapshot::JustCreated(snapshot);
    let mut runtime2 = JsRuntime::new(RuntimeOptions {
      startup_snapshot: Some(snapshot),
      ..Default::default()
    });
    runtime2
      .execute("check.js", "if (a != 3) throw Error('x')")
      .unwrap();
  }

  #[test]
  fn test_from_boxed_snapshot() {
    let snapshot = {
      let mut runtime = JsRuntime::new(RuntimeOptions {
        will_snapshot: true,
        ..Default::default()
      });
      runtime.execute("a.js", "a = 1 + 2").unwrap();
      let snap: &[u8] = &*runtime.snapshot();
      Vec::from(snap).into_boxed_slice()
    };

    let snapshot = Snapshot::Boxed(snapshot);
    let mut runtime2 = JsRuntime::new(RuntimeOptions {
      startup_snapshot: Some(snapshot),
      ..Default::default()
    });
    runtime2
      .execute("check.js", "if (a != 3) throw Error('x')")
      .unwrap();
  }

  #[test]
  fn test_heap_limits() {
    let create_params = v8::Isolate::create_params().heap_limits(0, 20 * 1024);
    let mut runtime = JsRuntime::new(RuntimeOptions {
      create_params: Some(create_params),
      ..Default::default()
    });
    let cb_handle = runtime.v8_isolate().thread_safe_handle();

    let callback_invoke_count = Rc::new(AtomicUsize::default());
    let inner_invoke_count = Rc::clone(&callback_invoke_count);

    runtime.add_near_heap_limit_callback(
      move |current_limit, _initial_limit| {
        inner_invoke_count.fetch_add(1, Ordering::SeqCst);
        cb_handle.terminate_execution();
        current_limit * 2
      },
    );
    let err = runtime
      .execute(
        "script name",
        r#"let s = ""; while(true) { s += "Hello"; }"#,
      )
      .expect_err("script should fail");
    assert_eq!(
      "Uncaught Error: execution terminated",
      err.downcast::<JsError>().unwrap().message
    );
    assert!(callback_invoke_count.load(Ordering::SeqCst) > 0)
  }

  #[test]
  fn test_heap_limit_cb_remove() {
    let mut runtime = JsRuntime::new(Default::default());

    runtime.add_near_heap_limit_callback(|current_limit, _initial_limit| {
      current_limit * 2
    });
    runtime.remove_near_heap_limit_callback(20 * 1024);
    assert!(runtime.allocations.near_heap_limit_callback_data.is_none());
  }

  #[test]
  fn test_heap_limit_cb_multiple() {
    let create_params = v8::Isolate::create_params().heap_limits(0, 20 * 1024);
    let mut runtime = JsRuntime::new(RuntimeOptions {
      create_params: Some(create_params),
      ..Default::default()
    });
    let cb_handle = runtime.v8_isolate().thread_safe_handle();

    let callback_invoke_count_first = Rc::new(AtomicUsize::default());
    let inner_invoke_count_first = Rc::clone(&callback_invoke_count_first);
    runtime.add_near_heap_limit_callback(
      move |current_limit, _initial_limit| {
        inner_invoke_count_first.fetch_add(1, Ordering::SeqCst);
        current_limit * 2
      },
    );

    let callback_invoke_count_second = Rc::new(AtomicUsize::default());
    let inner_invoke_count_second = Rc::clone(&callback_invoke_count_second);
    runtime.add_near_heap_limit_callback(
      move |current_limit, _initial_limit| {
        inner_invoke_count_second.fetch_add(1, Ordering::SeqCst);
        cb_handle.terminate_execution();
        current_limit * 2
      },
    );

    let err = runtime
      .execute(
        "script name",
        r#"let s = ""; while(true) { s += "Hello"; }"#,
      )
      .expect_err("script should fail");
    assert_eq!(
      "Uncaught Error: execution terminated",
      err.downcast::<JsError>().unwrap().message
    );
    assert_eq!(0, callback_invoke_count_first.load(Ordering::SeqCst));
    assert!(callback_invoke_count_second.load(Ordering::SeqCst) > 0);
  }

  #[test]
  fn es_snapshot() {
    #[derive(Default)]
    struct ModsLoader;

    impl ModuleLoader for ModsLoader {
      fn resolve(
        &self,
        _op_state: Rc<RefCell<OpState>>,
        specifier: &str,
        referrer: &str,
        _is_main: bool,
      ) -> Result<ModuleSpecifier, AnyError> {
        assert_eq!(specifier, "file:///main.js");
        assert_eq!(referrer, ".");
        let s = crate::resolve_import(specifier, referrer).unwrap();
        Ok(s)
      }

      fn load(
        &self,
        _op_state: Rc<RefCell<OpState>>,
        _module_specifier: &ModuleSpecifier,
        _maybe_referrer: Option<ModuleSpecifier>,
        _is_dyn_import: bool,
      ) -> Pin<Box<ModuleSourceFuture>> {
        unreachable!()
      }
    }

    let loader = std::rc::Rc::new(ModsLoader::default());
    let mut runtime = JsRuntime::new(RuntimeOptions {
      module_loader: Some(loader),
      will_snapshot: true,
      ..Default::default()
    });

    let specifier = crate::resolve_url("file:///main.js").unwrap();
    let source_code = "Deno.core.print('hello\\n')".to_string();

    let module_id = futures::executor::block_on(
      runtime.load_module(&specifier, Some(source_code)),
    )
    .unwrap();

    runtime.mod_evaluate(module_id);
    futures::executor::block_on(runtime.run_event_loop()).unwrap();

    let _snapshot = runtime.snapshot();
  }

  #[test]
  fn test_error_without_stack() {
    let mut runtime = JsRuntime::new(RuntimeOptions::default());
    // SyntaxError
    let result = runtime.execute(
      "error_without_stack.js",
      r#"
function main() {
  console.log("asdf);
}

main();
"#,
    );
    let expected_error = r#"Uncaught SyntaxError: Invalid or unexpected token
    at error_without_stack.js:3:14"#;
    assert_eq!(result.unwrap_err().to_string(), expected_error);
  }

  #[test]
  fn test_error_stack() {
    let mut runtime = JsRuntime::new(RuntimeOptions::default());
    let result = runtime.execute(
      "error_stack.js",
      r#"
function assert(cond) {
  if (!cond) {
    throw Error("assert");
  }
}

function main() {
  assert(false);
}

main();
        "#,
    );
    let expected_error = r#"Error: assert
    at assert (error_stack.js:4:11)
    at main (error_stack.js:9:3)
    at error_stack.js:12:1"#;
    assert_eq!(result.unwrap_err().to_string(), expected_error);
  }

  #[test]
  fn test_error_async_stack() {
    run_in_task(|cx| {
      let mut runtime = JsRuntime::new(RuntimeOptions::default());
      runtime
        .execute(
          "error_async_stack.js",
          r#"
(async () => {
  const p = (async () => {
    await Promise.resolve().then(() => {
      throw new Error("async");
    });
  })();

  try {
    await p;
  } catch (error) {
    console.log(error.stack);
    throw error;
  }
})();"#,
        )
        .unwrap();
      let expected_error = r#"Error: async
    at error_async_stack.js:5:13
    at async error_async_stack.js:4:5
    at async error_async_stack.js:10:5"#;

      match runtime.poll_event_loop(cx) {
        Poll::Ready(Err(e)) => {
          assert_eq!(e.to_string(), expected_error);
        }
        _ => panic!(),
      };
    })
  }

  #[test]
  fn test_core_js_stack_frame() {
    let mut runtime = JsRuntime::new(RuntimeOptions::default());
    // Call non-existent op so we get error from `core.js`
    let error = runtime
      .execute(
        "core_js_stack_frame.js",
        "Deno.core.opSync('non_existent');",
      )
      .unwrap_err();
    let error_string = error.to_string();
    // Test that the script specifier is a URL: `deno:<repo-relative path>`.
    assert!(error_string.contains("deno:core/core.js"));
  }

  #[test]
  fn test_v8_platform() {
    let options = RuntimeOptions {
      v8_platform: Some(v8::new_default_platform()),
      ..Default::default()
    };
    let mut runtime = JsRuntime::new(options);
    runtime.execute("<none>", "").unwrap();
  }
}
