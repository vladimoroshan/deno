// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use deno_core::ErrBox;
use futures::Future;
use serde_json::Value;

mod compiler_worker;
mod js;
mod json;
mod ts;
mod wasm;

pub use js::JsCompiler;
pub use json::JsonCompiler;
pub use ts::runtime_compile_async;
pub use ts::runtime_transpile_async;
pub use ts::TsCompiler;
pub use wasm::WasmCompiler;

pub type CompilationResultFuture =
  dyn Future<Output = Result<Value, ErrBox>> + Send;

#[derive(Debug, Clone)]
pub struct CompiledModule {
  pub code: String,
  pub name: String,
}

pub type CompiledModuleFuture =
  dyn Future<Output = Result<CompiledModule, ErrBox>> + Send;
