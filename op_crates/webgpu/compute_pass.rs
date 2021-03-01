// Copyright 2018-2021 the Deno authors. All rights reserved. MIT license.

use deno_core::error::bad_resource_id;
use deno_core::error::AnyError;
use deno_core::serde_json::json;
use deno_core::serde_json::Value;
use deno_core::ZeroCopyBuf;
use deno_core::{OpState, Resource};
use serde::Deserialize;
use std::borrow::Cow;
use std::cell::RefCell;

use super::error::WebGPUError;

pub(crate) struct WebGPUComputePass(
  pub(crate) RefCell<wgpu_core::command::ComputePass>,
);
impl Resource for WebGPUComputePass {
  fn name(&self) -> Cow<str> {
    "webGPUComputePass".into()
  }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassSetPipelineArgs {
  compute_pass_rid: u32,
  pipeline: u32,
}

pub fn op_webgpu_compute_pass_set_pipeline(
  state: &mut OpState,
  args: ComputePassSetPipelineArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pipeline_resource = state
    .resource_table
    .get::<super::pipeline::WebGPUComputePipeline>(args.pipeline)
    .ok_or_else(bad_resource_id)?;
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  wgpu_core::command::compute_ffi::wgpu_compute_pass_set_pipeline(
    &mut compute_pass_resource.0.borrow_mut(),
    compute_pipeline_resource.0,
  );

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassDispatchArgs {
  compute_pass_rid: u32,
  x: u32,
  y: u32,
  z: u32,
}

pub fn op_webgpu_compute_pass_dispatch(
  state: &mut OpState,
  args: ComputePassDispatchArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  wgpu_core::command::compute_ffi::wgpu_compute_pass_dispatch(
    &mut compute_pass_resource.0.borrow_mut(),
    args.x,
    args.y,
    args.z,
  );

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassDispatchIndirectArgs {
  compute_pass_rid: u32,
  indirect_buffer: u32,
  indirect_offset: u64,
}

pub fn op_webgpu_compute_pass_dispatch_indirect(
  state: &mut OpState,
  args: ComputePassDispatchIndirectArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let buffer_resource = state
    .resource_table
    .get::<super::buffer::WebGPUBuffer>(args.indirect_buffer)
    .ok_or_else(bad_resource_id)?;
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  wgpu_core::command::compute_ffi::wgpu_compute_pass_dispatch_indirect(
    &mut compute_pass_resource.0.borrow_mut(),
    buffer_resource.0,
    args.indirect_offset,
  );

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassBeginPipelineStatisticsQueryArgs {
  compute_pass_rid: u32,
  query_set: u32,
  query_index: u32,
}

pub fn op_webgpu_compute_pass_begin_pipeline_statistics_query(
  state: &mut OpState,
  args: ComputePassBeginPipelineStatisticsQueryArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;
  let query_set_resource = state
    .resource_table
    .get::<super::WebGPUQuerySet>(args.query_set)
    .ok_or_else(bad_resource_id)?;

  unsafe {
    wgpu_core::command::compute_ffi::wgpu_compute_pass_begin_pipeline_statistics_query(
      &mut compute_pass_resource.0.borrow_mut(),
      query_set_resource.0,
      args.query_index,
    );
  }

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassEndPipelineStatisticsQueryArgs {
  compute_pass_rid: u32,
}

pub fn op_webgpu_compute_pass_end_pipeline_statistics_query(
  state: &mut OpState,
  args: ComputePassEndPipelineStatisticsQueryArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  unsafe {
    wgpu_core::command::compute_ffi::wgpu_compute_pass_end_pipeline_statistics_query(
      &mut compute_pass_resource.0.borrow_mut(),
    );
  }

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassWriteTimestampArgs {
  compute_pass_rid: u32,
  query_set: u32,
  query_index: u32,
}

pub fn op_webgpu_compute_pass_write_timestamp(
  state: &mut OpState,
  args: ComputePassWriteTimestampArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;
  let query_set_resource = state
    .resource_table
    .get::<super::WebGPUQuerySet>(args.query_set)
    .ok_or_else(bad_resource_id)?;

  unsafe {
    wgpu_core::command::compute_ffi::wgpu_compute_pass_write_timestamp(
      &mut compute_pass_resource.0.borrow_mut(),
      query_set_resource.0,
      args.query_index,
    );
  }

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassEndPassArgs {
  command_encoder_rid: u32,
  compute_pass_rid: u32,
}

pub fn op_webgpu_compute_pass_end_pass(
  state: &mut OpState,
  args: ComputePassEndPassArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let command_encoder_resource = state
    .resource_table
    .get::<super::command_encoder::WebGPUCommandEncoder>(
      args.command_encoder_rid,
    )
    .ok_or_else(bad_resource_id)?;
  let command_encoder = command_encoder_resource.0;
  let compute_pass_resource = state
    .resource_table
    .take::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;
  let compute_pass = &compute_pass_resource.0.borrow();
  let instance = state.borrow::<super::Instance>();

  let maybe_err =
    gfx_select!(command_encoder => instance.command_encoder_run_compute_pass(
      command_encoder,
      compute_pass
    ))
    .err();

  Ok(json!({ "err": maybe_err.map(WebGPUError::from) }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassSetBindGroupArgs {
  compute_pass_rid: u32,
  index: u32,
  bind_group: u32,
  dynamic_offsets_data: Option<Vec<u32>>,
  dynamic_offsets_data_start: usize,
  dynamic_offsets_data_length: usize,
}

pub fn op_webgpu_compute_pass_set_bind_group(
  state: &mut OpState,
  args: ComputePassSetBindGroupArgs,
  zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let bind_group_resource = state
    .resource_table
    .get::<super::binding::WebGPUBindGroup>(args.bind_group)
    .ok_or_else(bad_resource_id)?;
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  unsafe {
    wgpu_core::command::compute_ffi::wgpu_compute_pass_set_bind_group(
      &mut compute_pass_resource.0.borrow_mut(),
      args.index,
      bind_group_resource.0,
      match args.dynamic_offsets_data {
        Some(data) => data.as_ptr(),
        None => {
          let (prefix, data, suffix) = zero_copy[0].align_to::<u32>();
          assert!(prefix.is_empty());
          assert!(suffix.is_empty());
          data[args.dynamic_offsets_data_start..].as_ptr()
        }
      },
      args.dynamic_offsets_data_length,
    );
  }

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassPushDebugGroupArgs {
  compute_pass_rid: u32,
  group_label: String,
}

pub fn op_webgpu_compute_pass_push_debug_group(
  state: &mut OpState,
  args: ComputePassPushDebugGroupArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  unsafe {
    let label = std::ffi::CString::new(args.group_label).unwrap();
    wgpu_core::command::compute_ffi::wgpu_compute_pass_push_debug_group(
      &mut compute_pass_resource.0.borrow_mut(),
      label.as_ptr(),
      0, // wgpu#975
    );
  }

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassPopDebugGroupArgs {
  compute_pass_rid: u32,
}

pub fn op_webgpu_compute_pass_pop_debug_group(
  state: &mut OpState,
  args: ComputePassPopDebugGroupArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  wgpu_core::command::compute_ffi::wgpu_compute_pass_pop_debug_group(
    &mut compute_pass_resource.0.borrow_mut(),
  );

  Ok(json!({}))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComputePassInsertDebugMarkerArgs {
  compute_pass_rid: u32,
  marker_label: String,
}

pub fn op_webgpu_compute_pass_insert_debug_marker(
  state: &mut OpState,
  args: ComputePassInsertDebugMarkerArgs,
  _zero_copy: &mut [ZeroCopyBuf],
) -> Result<Value, AnyError> {
  let compute_pass_resource = state
    .resource_table
    .get::<WebGPUComputePass>(args.compute_pass_rid)
    .ok_or_else(bad_resource_id)?;

  unsafe {
    let label = std::ffi::CString::new(args.marker_label).unwrap();
    wgpu_core::command::compute_ffi::wgpu_compute_pass_insert_debug_marker(
      &mut compute_pass_resource.0.borrow_mut(),
      label.as_ptr(),
      0, // wgpu#975
    );
  }

  Ok(json!({}))
}
