// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use crate::error::AnyError;
use crate::serialize_op_result;
use crate::Op;
use crate::OpFn;
use crate::OpPayload;
use crate::OpState;
use crate::ZeroCopyBuf;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;

/// Creates an op that passes data synchronously using JSON.
///
/// The provided function `op_fn` has the following parameters:
/// * `&mut OpState`: the op state, can be used to read/write resources in the runtime from an op.
/// * `V`: the deserializable value that is passed to the Rust function.
/// * `&mut [ZeroCopyBuf]`: raw bytes passed along, usually not needed if the JSON value is used.
///
/// `op_fn` returns a serializable value, which is directly returned to JavaScript.
///
/// When registering an op like this...
/// ```ignore
/// let mut runtime = JsRuntime::new(...);
/// runtime.register_op("hello", deno_core::json_op_sync(Self::hello_op));
/// ```
///
/// ...it can be invoked from JS using the provided name, for example:
/// ```js
/// Deno.core.ops();
/// let result = Deno.core.jsonOpSync("function_name", args);
/// ```
///
/// The `Deno.core.ops()` statement is needed once before any op calls, for initialization.
/// A more complete example is available in the examples directory.
pub fn json_op_sync<F, V, R>(op_fn: F) -> Box<OpFn>
where
  F: Fn(&mut OpState, V, Option<ZeroCopyBuf>) -> Result<R, AnyError> + 'static,
  V: DeserializeOwned,
  R: Serialize + 'static,
{
  Box::new(move |state, payload, buf| -> Op {
    let result = payload
      .deserialize()
      .and_then(|args| op_fn(&mut state.borrow_mut(), args, buf));
    Op::Sync(serialize_op_result(result, state))
  })
}

/// Creates an op that passes data asynchronously using JSON.
///
/// The provided function `op_fn` has the following parameters:
/// * `Rc<RefCell<OpState>`: the op state, can be used to read/write resources in the runtime from an op.
/// * `V`: the deserializable value that is passed to the Rust function.
/// * `BufVec`: raw bytes passed along, usually not needed if the JSON value is used.
///
/// `op_fn` returns a future, whose output is a serializable value. This value will be asynchronously
/// returned to JavaScript.
///
/// When registering an op like this...
/// ```ignore
/// let mut runtime = JsRuntime::new(...);
/// runtime.register_op("hello", deno_core::json_op_async(Self::hello_op));
/// ```
///
/// ...it can be invoked from JS using the provided name, for example:
/// ```js
/// Deno.core.ops();
/// let future = Deno.core.jsonOpAsync("function_name", args);
/// ```
///
/// The `Deno.core.ops()` statement is needed once before any op calls, for initialization.
/// A more complete example is available in the examples directory.
pub fn json_op_async<F, V, R, RV>(op_fn: F) -> Box<OpFn>
where
  F: Fn(Rc<RefCell<OpState>>, V, Option<ZeroCopyBuf>) -> R + 'static,
  V: DeserializeOwned,
  R: Future<Output = Result<RV, AnyError>> + 'static,
  RV: Serialize + 'static,
{
  let try_dispatch_op = move |state: Rc<RefCell<OpState>>,
                              p: OpPayload,
                              buf: Option<ZeroCopyBuf>|
        -> Result<Op, AnyError> {
    // Parse args
    let args = p.deserialize()?;

    use crate::futures::FutureExt;
    let fut = op_fn(state.clone(), args, buf)
      .map(move |result| serialize_op_result(result, state));
    Ok(Op::Async(Box::pin(fut)))
  };

  Box::new(
    move |state: Rc<RefCell<OpState>>,
          p: OpPayload,
          b: Option<ZeroCopyBuf>|
          -> Op {
      match try_dispatch_op(state.clone(), p, b) {
        Ok(op) => op,
        Err(err) => {
          Op::Sync(serialize_op_result(Err::<(), AnyError>(err), state))
        }
      }
    },
  )
}
