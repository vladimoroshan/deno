// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

use crate::colors;
use crate::ops::io::get_stdio;
use crate::permissions::Permissions;
use crate::program_state::ProgramState;
use crate::tokio_util::create_basic_runtime;
use crate::web_worker::WebWorker;
use crate::web_worker::WebWorkerHandle;
use crate::web_worker::WorkerEvent;
use deno_core::error::generic_error;
use deno_core::error::AnyError;
use deno_core::error::JsError;
use deno_core::futures::channel::mpsc;
use deno_core::futures::future::FutureExt;
use deno_core::serde_json;
use deno_core::serde_json::json;
use deno_core::serde_json::Value;
use deno_core::BufVec;
use deno_core::ModuleSpecifier;
use deno_core::OpState;
use deno_core::ZeroCopyBuf;
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::From;
use std::rc::Rc;
use std::sync::Arc;
use std::thread::JoinHandle;

#[derive(Deserialize)]
struct HostUnhandledErrorArgs {
  message: String,
}

pub fn init(
  rt: &mut deno_core::JsRuntime,
  sender: Option<mpsc::Sender<WorkerEvent>>,
) {
  {
    let op_state = rt.op_state();
    let mut state = op_state.borrow_mut();
    state.put::<WorkersTable>(WorkersTable::default());
    state.put::<WorkerId>(WorkerId::default());
  }
  super::reg_json_sync(rt, "op_create_worker", op_create_worker);
  super::reg_json_sync(
    rt,
    "op_host_terminate_worker",
    op_host_terminate_worker,
  );
  super::reg_json_sync(rt, "op_host_post_message", op_host_post_message);
  super::reg_json_async(rt, "op_host_get_message", op_host_get_message);
  super::reg_json_sync(
    rt,
    "op_host_unhandled_error",
    move |_state, args, _zero_copy| {
      if let Some(mut sender) = sender.clone() {
        let args: HostUnhandledErrorArgs = serde_json::from_value(args)?;
        sender
          .try_send(WorkerEvent::Error(generic_error(args.message)))
          .expect("Failed to propagate error event to parent worker");
        Ok(json!(true))
      } else {
        Err(generic_error("Cannot be called from main worker."))
      }
    },
  );
}

pub type WorkersTable = HashMap<u32, (JoinHandle<()>, WebWorkerHandle)>;
pub type WorkerId = u32;

fn create_web_worker(
  worker_id: u32,
  name: String,
  program_state: &Arc<ProgramState>,
  permissions: Permissions,
  specifier: ModuleSpecifier,
  has_deno_namespace: bool,
) -> Result<WebWorker, AnyError> {
  let mut worker = WebWorker::new(
    name.clone(),
    permissions,
    specifier,
    program_state.clone(),
    has_deno_namespace,
  );

  if has_deno_namespace {
    let state = worker.js_runtime.op_state();
    let mut state = state.borrow_mut();
    let (stdin, stdout, stderr) = get_stdio();
    if let Some(stream) = stdin {
      state.resource_table.add("stdin", Box::new(stream));
    }
    if let Some(stream) = stdout {
      state.resource_table.add("stdout", Box::new(stream));
    }
    if let Some(stream) = stderr {
      state.resource_table.add("stderr", Box::new(stream));
    }
  }

  // Instead of using name for log we use `worker-${id}` because
  // WebWorkers can have empty string as name.
  let script = format!(
    "bootstrap.workerRuntime(\"{}\", {}, \"worker-{}\")",
    name, worker.has_deno_namespace, worker_id
  );
  worker.execute(&script)?;

  Ok(worker)
}

// TODO(bartlomieju): check if order of actions is aligned to Worker spec
fn run_worker_thread(
  worker_id: u32,
  name: String,
  program_state: &Arc<ProgramState>,
  permissions: Permissions,
  specifier: ModuleSpecifier,
  has_deno_namespace: bool,
  maybe_source_code: Option<String>,
) -> Result<(JoinHandle<()>, WebWorkerHandle), AnyError> {
  let program_state = program_state.clone();
  let (handle_sender, handle_receiver) =
    std::sync::mpsc::sync_channel::<Result<WebWorkerHandle, AnyError>>(1);

  let builder =
    std::thread::Builder::new().name(format!("deno-worker-{}", worker_id));
  let join_handle = builder.spawn(move || {
    // Any error inside this block is terminal:
    // - JS worker is useless - meaning it throws an exception and can't do anything else,
    //  all action done upon it should be noops
    // - newly spawned thread exits
    let result = create_web_worker(
      worker_id,
      name,
      &program_state,
      permissions,
      specifier.clone(),
      has_deno_namespace,
    );

    if let Err(err) = result {
      handle_sender.send(Err(err)).unwrap();
      return;
    }

    let mut worker = result.unwrap();
    let name = worker.name.to_string();
    // Send thread safe handle to newly created worker to host thread
    handle_sender.send(Ok(worker.thread_safe_handle())).unwrap();
    drop(handle_sender);

    // At this point the only method of communication with host
    // is using `worker.internal_channels`.
    //
    // Host can already push messages and interact with worker.
    //
    // Next steps:
    // - create tokio runtime
    // - load provided module or code
    // - start driving worker's event loop

    let mut rt = create_basic_runtime();

    // TODO: run with using select with terminate

    // Execute provided source code immediately
    let result = if let Some(source_code) = maybe_source_code {
      worker.execute(&source_code)
    } else {
      // TODO(bartlomieju): add "type": "classic", ie. ability to load
      // script instead of module
      let load_future = worker.execute_module(&specifier).boxed_local();

      rt.block_on(load_future)
    };

    let mut sender = worker.internal_channels.sender.clone();

    // If sender is closed it means that worker has already been closed from
    // within using "globalThis.close()"
    if sender.is_closed() {
      return;
    }

    if let Err(e) = result {
      eprintln!(
        "{}: Uncaught (in worker \"{}\") {}",
        colors::red_bold("error"),
        name,
        e.to_string().trim_start_matches("Uncaught "),
      );
      sender
        .try_send(WorkerEvent::TerminalError(e))
        .expect("Failed to post message to host");

      // Failure to execute script is a terminal error, bye, bye.
      return;
    }

    // TODO(bartlomieju): this thread should return result of event loop
    // that means that we should store JoinHandle to thread to ensure
    // that it actually terminates.
    rt.block_on(worker.run_event_loop())
      .expect("Panic in event loop");
    debug!("Worker thread shuts down {}", &name);
  })?;

  let worker_handle = handle_receiver.recv().unwrap()?;
  Ok((join_handle, worker_handle))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkerArgs {
  name: Option<String>,
  specifier: String,
  has_source_code: bool,
  source_code: String,
  use_deno_namespace: bool,
}

/// Create worker as the host
fn op_create_worker(
  state: &mut OpState,
  args: Value,
  _data: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let args: CreateWorkerArgs = serde_json::from_value(args)?;

  let specifier = args.specifier.clone();
  let maybe_source_code = if args.has_source_code {
    Some(args.source_code.clone())
  } else {
    None
  };
  let args_name = args.name;
  let use_deno_namespace = args.use_deno_namespace;
  if use_deno_namespace {
    super::check_unstable(state, "Worker.deno");
  }
  let permissions = state.borrow::<Permissions>().clone();
  let worker_id = state.take::<WorkerId>();
  state.put::<WorkerId>(worker_id + 1);

  let module_specifier = ModuleSpecifier::resolve_url(&specifier)?;
  let worker_name = args_name.unwrap_or_else(|| "".to_string());
  let cli_state = super::program_state(state);

  let (join_handle, worker_handle) = run_worker_thread(
    worker_id,
    worker_name,
    &cli_state,
    permissions,
    module_specifier,
    use_deno_namespace,
    maybe_source_code,
  )?;
  // At this point all interactions with worker happen using thread
  // safe handler returned from previous function call
  state
    .borrow_mut::<WorkersTable>()
    .insert(worker_id, (join_handle, worker_handle));

  Ok(json!({ "id": worker_id }))
}

#[derive(Deserialize)]
struct WorkerArgs {
  id: i32,
}

fn op_host_terminate_worker(
  state: &mut OpState,
  args: Value,
  _data: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let args: WorkerArgs = serde_json::from_value(args)?;
  let id = args.id as u32;
  let (join_handle, worker_handle) = state
    .borrow_mut::<WorkersTable>()
    .remove(&id)
    .expect("No worker handle found");
  worker_handle.terminate();
  join_handle.join().expect("Panic in worker thread");
  Ok(json!({}))
}

fn serialize_worker_event(event: WorkerEvent) -> Value {
  match event {
    WorkerEvent::Message(buf) => json!({ "type": "msg", "data": buf }),
    WorkerEvent::TerminalError(error) => match error.downcast::<JsError>() {
      Ok(js_error) => json!({
        "type": "terminalError",
        "error": {
          "message": js_error.message,
          "fileName": js_error.script_resource_name,
          "lineNumber": js_error.line_number,
          "columnNumber": js_error.start_column,
        }
      }),
      Err(error) => json!({
        "type": "terminalError",
        "error": {
          "message": error.to_string(),
        }
      }),
    },
    WorkerEvent::Error(error) => match error.downcast::<JsError>() {
      Ok(js_error) => json!({
        "type": "error",
        "error": {
          "message": js_error.message,
          "fileName": js_error.script_resource_name,
          "lineNumber": js_error.line_number,
          "columnNumber": js_error.start_column,
        }
      }),
      Err(error) => json!({
        "type": "error",
        "error": {
          "message": error.to_string(),
        }
      }),
    },
  }
}

/// Get message from guest worker as host
async fn op_host_get_message(
  state: Rc<RefCell<OpState>>,
  args: Value,
  _zero_copy: BufVec,
) -> Result<Value, AnyError> {
  let args: WorkerArgs = serde_json::from_value(args)?;
  let id = args.id as u32;

  let worker_handle = {
    let s = state.borrow();
    let workers_table = s.borrow::<WorkersTable>();
    let maybe_handle = workers_table.get(&id);
    if let Some(handle) = maybe_handle {
      handle.1.clone()
    } else {
      // If handle was not found it means worker has already shutdown
      return Ok(json!({ "type": "close" }));
    }
  };

  let response = match worker_handle.get_event().await? {
    Some(event) => {
      // Terminal error means that worker should be removed from worker table.
      if let WorkerEvent::TerminalError(_) = &event {
        let mut s = state.borrow_mut();
        if let Some((join_handle, mut worker_handle)) =
          s.borrow_mut::<WorkersTable>().remove(&id)
        {
          worker_handle.sender.close_channel();
          join_handle.join().expect("Worker thread panicked");
        };
      }
      serialize_worker_event(event)
    }
    None => {
      // Worker shuts down
      let mut s = state.borrow_mut();
      let workers = s.borrow_mut::<WorkersTable>();
      // Try to remove worker from workers table - NOTE: `Worker.terminate()` might have been called
      // already meaning that we won't find worker in table - in that case ignore.
      if let Some((join_handle, mut worker_handle)) = workers.remove(&id) {
        worker_handle.sender.close_channel();
        join_handle.join().expect("Worker thread panicked");
      }
      json!({ "type": "close" })
    }
  };
  Ok(response)
}

/// Post message to guest worker as host
fn op_host_post_message(
  state: &mut OpState,
  args: Value,
  data: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  assert_eq!(data.len(), 1, "Invalid number of arguments");
  let args: WorkerArgs = serde_json::from_value(args)?;
  let id = args.id as u32;
  let msg = Vec::from(&*data[0]).into_boxed_slice();

  debug!("post message to worker {}", id);
  let workers = state.borrow::<WorkersTable>();
  let worker_handle = workers[&id].1.clone();
  worker_handle.post_message(msg)?;
  Ok(json!({}))
}
