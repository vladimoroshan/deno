use crate::TSState;
use deno::CoreOp;
use deno::ErrBox;
use deno::ModuleSpecifier;
use deno::Op;
use deno::OpId;
use serde::Deserialize;
use serde_json::json;
use serde_json::Value;

#[derive(Debug)]
pub struct WrittenFile {
  pub url: String,
  pub module_name: String,
  pub source_code: String,
}

fn dispatch2(
  s: &mut TSState,
  op_id: OpId,
  control_buf: &[u8],
) -> Result<Value, ErrBox> {
  let v = serde_json::from_slice(control_buf)?;
  // Warning! The op_id values below are shared between this code and
  // compiler_main.js. Update with care!
  match op_id {
    49 => read_file(s, v),
    50 => exit(s, v),
    51 => write_file(s, v),
    52 => resolve_module_names(s, v),
    53 => set_emit_result(s, v),
    _ => unreachable!(),
  }
}

pub fn dispatch_op(s: &mut TSState, op_id: OpId, control_buf: &[u8]) -> CoreOp {
  let result = dispatch2(s, op_id, control_buf);
  let response = match result {
    Ok(v) => json!({ "ok": v }),
    Err(err) => json!({ "err": err.to_string() }),
  };
  let x = serde_json::to_string(&response).unwrap();
  let vec = x.into_bytes();
  Op::Sync(vec.into_boxed_slice())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReadFile {
  file_name: String,
  language_version: Option<i32>,
  should_create_new_source_file: bool,
}

fn read_file(_s: &mut TSState, v: Value) -> Result<Value, ErrBox> {
  let v: ReadFile = serde_json::from_value(v)?;
  let (module_name, source_code) = if v.file_name.starts_with("$asset$/") {
    let asset = v.file_name.replace("$asset$/", "");
    let source_code = crate::get_asset2(&asset)?.to_string();
    (asset, source_code)
  } else {
    assert!(!v.file_name.starts_with("$assets$"), "you meant $asset$");
    let module_specifier = ModuleSpecifier::resolve_url_or_path(&v.file_name)?;
    let path = module_specifier.as_url().to_file_path().unwrap();
    println!("cargo:rerun-if-changed={}", path.display());
    (
      module_specifier.as_str().to_string(),
      std::fs::read_to_string(&path)?,
    )
  };
  Ok(json!({
    "moduleName": module_name,
    "sourceCode": source_code,
  }))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WriteFile {
  file_name: String,
  data: String,
  module_name: String,
}

fn write_file(s: &mut TSState, v: Value) -> Result<Value, ErrBox> {
  let v: WriteFile = serde_json::from_value(v)?;
  let module_specifier = ModuleSpecifier::resolve_url_or_path(&v.file_name)?;
  if s.bundle {
    std::fs::write(&v.file_name, &v.data)?;
  }
  s.written_files.push(WrittenFile {
    url: module_specifier.as_str().to_string(),
    module_name: v.module_name,
    source_code: v.data,
  });
  Ok(json!(true))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResolveModuleNames {
  module_names: Vec<String>,
  containing_file: String,
}

fn resolve_module_names(_s: &mut TSState, v: Value) -> Result<Value, ErrBox> {
  let v: ResolveModuleNames = serde_json::from_value(v).unwrap();
  let mut resolved = Vec::<String>::new();
  let referrer = ModuleSpecifier::resolve_url_or_path(&v.containing_file)?;
  for specifier in v.module_names {
    if specifier.starts_with("$asset$/") {
      resolved.push(specifier.clone());
    } else {
      let ms = ModuleSpecifier::resolve_import(&specifier, referrer.as_str())?;
      resolved.push(ms.as_str().to_string());
    }
  }
  Ok(json!(resolved))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Exit {
  code: i32,
}

fn exit(s: &mut TSState, v: Value) -> Result<Value, ErrBox> {
  let v: Exit = serde_json::from_value(v)?;
  s.exit_code = v.code;
  std::process::exit(v.code)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmitResult {
  pub emit_skipped: bool,
  pub diagnostics: Vec<String>,
  pub emitted_files: Vec<String>,
}

fn set_emit_result(s: &mut TSState, v: Value) -> Result<Value, ErrBox> {
  let v: EmitResult = serde_json::from_value(v)?;
  s.emit_result = Some(v);
  Ok(json!(true))
}
