// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.

// TODO: This currently only applies to uncaught exceptions. It would be nice to
// also have source maps for situations like this:
//   const err = new Error("Boo!");
//   console.log(err.stack);
// It would require calling into Rust from Error.prototype.prepareStackTrace.

use crate::ErrBox;
use rusty_v8 as v8;
use std::convert::TryFrom;
use std::convert::TryInto;
use std::error::Error;
use std::fmt;

/// A `JSError` represents an exception coming from V8, with stack frames and
/// line numbers. The deno_cli crate defines another `JSError` type, which wraps
/// the one defined here, that adds source map support and colorful formatting.
#[derive(Debug, PartialEq, Clone)]
pub struct JSError {
  pub message: String,
  pub source_line: Option<String>,
  pub script_resource_name: Option<String>,
  pub line_number: Option<i64>,
  pub start_column: Option<i64>, // 0-based
  pub end_column: Option<i64>,   // 0-based
  pub frames: Vec<JSStackFrame>,
  pub formatted_frames: Vec<String>,
}

#[derive(Debug, PartialEq, Clone)]
pub struct JSStackFrame {
  pub type_name: Option<String>,
  pub function_name: Option<String>,
  pub method_name: Option<String>,
  pub file_name: Option<String>,
  pub line_number: Option<i64>,
  pub column_number: Option<i64>,
  pub eval_origin: Option<String>,
  pub is_top_level: Option<bool>,
  pub is_eval: bool,
  pub is_native: bool,
  pub is_constructor: bool,
  pub is_async: bool,
  pub is_promise_all: bool,
  pub promise_index: Option<i64>,
}

fn get_property<'a>(
  scope: &mut impl v8::ToLocal<'a>,
  context: v8::Local<v8::Context>,
  object: v8::Local<v8::Object>,
  key: &str,
) -> Option<v8::Local<'a, v8::Value>> {
  let key = v8::String::new(scope, key).unwrap();
  object.get(scope, context, key.into())
}

impl JSError {
  pub(crate) fn create(js_error: Self) -> ErrBox {
    ErrBox::from(js_error)
  }

  pub fn from_v8_exception(
    scope: &mut impl v8::InIsolate,
    exception: v8::Local<v8::Value>,
  ) -> Self {
    // Create a new HandleScope because we're creating a lot of new local
    // handles below.
    let mut hs = v8::HandleScope::new(scope);
    let scope = hs.enter();
    let context = { scope.get_current_context().unwrap() };

    let msg = v8::Exception::create_message(scope, exception);

    let exception: Option<v8::Local<v8::Object>> =
      exception.clone().try_into().ok();
    let _ = exception.map(|e| get_property(scope, context, e, "stack"));

    let maybe_call_sites = exception
      .and_then(|e| get_property(scope, context, e, "__callSiteEvals"));
    let maybe_call_sites: Option<v8::Local<v8::Array>> =
      maybe_call_sites.and_then(|a| a.try_into().ok());

    let (frames, formatted_frames) = if let Some(call_sites) = maybe_call_sites
    {
      let mut frames: Vec<JSStackFrame> = vec![];
      let mut formatted_frames: Vec<String> = vec![];

      let formatted_frames_v8 =
        get_property(scope, context, exception.unwrap(), "__formattedFrames");
      let formatted_frames_v8: v8::Local<v8::Array> = formatted_frames_v8
        .and_then(|a| a.try_into().ok())
        .expect("__formattedFrames should be defined if __callSiteEvals is.");

      for i in 0..call_sites.length() {
        let call_site: v8::Local<v8::Object> = call_sites
          .get_index(scope, context, i)
          .unwrap()
          .try_into()
          .unwrap();
        let type_name: Option<v8::Local<v8::String>> =
          get_property(scope, context, call_site, "typeName")
            .unwrap()
            .try_into()
            .ok();
        let type_name = type_name.map(|s| s.to_rust_string_lossy(scope));
        let function_name: Option<v8::Local<v8::String>> =
          get_property(scope, context, call_site, "functionName")
            .unwrap()
            .try_into()
            .ok();
        let function_name =
          function_name.map(|s| s.to_rust_string_lossy(scope));
        let method_name: Option<v8::Local<v8::String>> =
          get_property(scope, context, call_site, "methodName")
            .unwrap()
            .try_into()
            .ok();
        let method_name = method_name.map(|s| s.to_rust_string_lossy(scope));
        let file_name: Option<v8::Local<v8::String>> =
          get_property(scope, context, call_site, "fileName")
            .unwrap()
            .try_into()
            .ok();
        let file_name = file_name.map(|s| s.to_rust_string_lossy(scope));
        let line_number: Option<v8::Local<v8::Integer>> =
          get_property(scope, context, call_site, "lineNumber")
            .unwrap()
            .try_into()
            .ok();
        let line_number = line_number.map(|n| n.value());
        let column_number: Option<v8::Local<v8::Integer>> =
          get_property(scope, context, call_site, "columnNumber")
            .unwrap()
            .try_into()
            .ok();
        let column_number = column_number.map(|n| n.value());
        let eval_origin: Option<v8::Local<v8::String>> =
          get_property(scope, context, call_site, "evalOrigin")
            .unwrap()
            .try_into()
            .ok();
        let eval_origin = eval_origin.map(|s| s.to_rust_string_lossy(scope));
        let is_top_level: Option<v8::Local<v8::Boolean>> =
          get_property(scope, context, call_site, "isTopLevel")
            .unwrap()
            .try_into()
            .ok();
        let is_top_level = is_top_level.map(|b| b.is_true());
        let is_eval: v8::Local<v8::Boolean> =
          get_property(scope, context, call_site, "isEval")
            .unwrap()
            .try_into()
            .unwrap();
        let is_eval = is_eval.is_true();
        let is_native: v8::Local<v8::Boolean> =
          get_property(scope, context, call_site, "isNative")
            .unwrap()
            .try_into()
            .unwrap();
        let is_native = is_native.is_true();
        let is_constructor: v8::Local<v8::Boolean> =
          get_property(scope, context, call_site, "isConstructor")
            .unwrap()
            .try_into()
            .unwrap();
        let is_constructor = is_constructor.is_true();
        let is_async: v8::Local<v8::Boolean> =
          get_property(scope, context, call_site, "isAsync")
            .unwrap()
            .try_into()
            .unwrap();
        let is_async = is_async.is_true();
        let is_promise_all: v8::Local<v8::Boolean> =
          get_property(scope, context, call_site, "isPromiseAll")
            .unwrap()
            .try_into()
            .unwrap();
        let is_promise_all = is_promise_all.is_true();
        let promise_index: Option<v8::Local<v8::Integer>> =
          get_property(scope, context, call_site, "columnNumber")
            .unwrap()
            .try_into()
            .ok();
        let promise_index = promise_index.map(|n| n.value());
        frames.push(JSStackFrame {
          type_name,
          function_name,
          method_name,
          file_name,
          line_number,
          column_number,
          eval_origin,
          is_top_level,
          is_eval,
          is_native,
          is_constructor,
          is_async,
          is_promise_all,
          promise_index,
        });
        let formatted_frame: v8::Local<v8::String> = formatted_frames_v8
          .get_index(scope, context, i)
          .unwrap()
          .try_into()
          .unwrap();
        let formatted_frame = formatted_frame.to_rust_string_lossy(scope);
        formatted_frames.push(formatted_frame)
      }
      (frames, formatted_frames)
    } else {
      (vec![], vec![])
    };

    Self {
      message: msg.get(scope).to_rust_string_lossy(scope),
      script_resource_name: msg
        .get_script_resource_name(scope)
        .and_then(|v| v8::Local::<v8::String>::try_from(v).ok())
        .map(|v| v.to_rust_string_lossy(scope)),
      source_line: msg
        .get_source_line(scope, context)
        .map(|v| v.to_rust_string_lossy(scope)),
      line_number: msg.get_line_number(context).and_then(|v| v.try_into().ok()),
      start_column: msg.get_start_column().try_into().ok(),
      end_column: msg.get_end_column().try_into().ok(),
      frames,
      formatted_frames,
    }
  }
}

impl Error for JSError {}

fn format_source_loc(
  file_name: &str,
  line_number: i64,
  column_number: i64,
) -> String {
  // TODO match this style with how typescript displays errors.
  let line_number = line_number;
  let column_number = column_number;
  format!("{}:{}:{}", file_name, line_number, column_number)
}

impl fmt::Display for JSError {
  fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
    if self.script_resource_name.is_some() {
      let script_resource_name = self.script_resource_name.as_ref().unwrap();
      if self.line_number.is_some() && self.start_column.is_some() {
        assert!(self.line_number.is_some());
        assert!(self.start_column.is_some());
        let source_loc = format_source_loc(
          script_resource_name,
          self.line_number.unwrap(),
          self.start_column.unwrap(),
        );
        write!(f, "{}", source_loc)?;
      }
      if self.source_line.is_some() {
        write!(f, "\n{}\n", self.source_line.as_ref().unwrap())?;
        let mut s = String::new();
        for i in 0..self.end_column.unwrap() {
          if i >= self.start_column.unwrap() {
            s.push('^');
          } else {
            s.push(' ');
          }
        }
        writeln!(f, "{}", s)?;
      }
    }

    write!(f, "{}", self.message)?;

    for formatted_frame in &self.formatted_frames {
      // TODO: Strip ANSI color from formatted_frame.
      write!(f, "\n    at {}", formatted_frame)?;
    }
    Ok(())
  }
}
