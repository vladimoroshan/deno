// Copyright 2018-2020 the Deno authors. All rights reserved. MIT license.
use super::ParamDef;
use serde::Serialize;
use swc_common::SourceMap;
use swc_ecma_ast;
use swc_ecma_ast::TsArrayType;
use swc_ecma_ast::TsConditionalType;
use swc_ecma_ast::TsFnOrConstructorType;
use swc_ecma_ast::TsIndexedAccessType;
use swc_ecma_ast::TsKeywordType;
use swc_ecma_ast::TsLit;
use swc_ecma_ast::TsLitType;
use swc_ecma_ast::TsOptionalType;
use swc_ecma_ast::TsParenthesizedType;
use swc_ecma_ast::TsRestType;
use swc_ecma_ast::TsThisType;
use swc_ecma_ast::TsTupleType;
use swc_ecma_ast::TsType;
use swc_ecma_ast::TsTypeAnn;
use swc_ecma_ast::TsTypeLit;
use swc_ecma_ast::TsTypeOperator;
use swc_ecma_ast::TsTypeQuery;
use swc_ecma_ast::TsTypeRef;
use swc_ecma_ast::TsUnionOrIntersectionType;

// pub enum TsType {
//  *      TsKeywordType(TsKeywordType),
//  *      TsThisType(TsThisType),
//  *      TsFnOrConstructorType(TsFnOrConstructorType),
//  *      TsTypeRef(TsTypeRef),
//  *      TsTypeQuery(TsTypeQuery),
//  *      TsTypeLit(TsTypeLit),
//  *      TsArrayType(TsArrayType),
//  *      TsTupleType(TsTupleType),
//  *      TsOptionalType(TsOptionalType),
//  *      TsRestType(TsRestType),
//  *      TsUnionOrIntersectionType(TsUnionOrIntersectionType),
//  *      TsConditionalType(TsConditionalType),
//     TsInferType(TsInferType),
//  *      TsParenthesizedType(TsParenthesizedType),
//  *      TsTypeOperator(TsTypeOperator),
//  *      TsIndexedAccessType(TsIndexedAccessType),
//     TsMappedType(TsMappedType),
//  *      TsLitType(TsLitType),
//     TsTypePredicate(TsTypePredicate),
//     TsImportType(TsImportType),
// }

impl Into<TsTypeDef> for &TsLitType {
  fn into(self) -> TsTypeDef {
    let (repr, lit) = match &self.lit {
      TsLit::Number(num) => (
        format!("{}", num.value),
        LiteralDef {
          kind: LiteralDefKind::Number,
          number: Some(num.value),
          string: None,
          boolean: None,
        },
      ),
      TsLit::Str(str_) => (
        str_.value.to_string(),
        LiteralDef {
          kind: LiteralDefKind::String,
          number: None,
          string: Some(str_.value.to_string()),
          boolean: None,
        },
      ),
      TsLit::Bool(bool_) => (
        bool_.value.to_string(),
        LiteralDef {
          kind: LiteralDefKind::Boolean,
          number: None,
          string: None,
          boolean: Some(bool_.value),
        },
      ),
    };

    TsTypeDef {
      repr,
      kind: Some(TsTypeDefKind::Literal),
      literal: Some(lit),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsArrayType {
  fn into(self) -> TsTypeDef {
    let ts_type_def: TsTypeDef = (&*self.elem_type).into();

    TsTypeDef {
      array: Some(Box::new(ts_type_def)),
      kind: Some(TsTypeDefKind::Array),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsTupleType {
  fn into(self) -> TsTypeDef {
    let mut type_defs = vec![];

    for type_box in &self.elem_types {
      let ts_type: &TsType = &(*type_box);
      let def: TsTypeDef = ts_type.into();
      type_defs.push(def)
    }

    TsTypeDef {
      tuple: Some(type_defs),
      kind: Some(TsTypeDefKind::Tuple),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsUnionOrIntersectionType {
  fn into(self) -> TsTypeDef {
    use swc_ecma_ast::TsUnionOrIntersectionType::*;

    match self {
      TsUnionType(union_type) => {
        let mut types_union = vec![];

        for type_box in &union_type.types {
          let ts_type: &TsType = &(*type_box);
          let def: TsTypeDef = ts_type.into();
          types_union.push(def);
        }

        TsTypeDef {
          union: Some(types_union),
          kind: Some(TsTypeDefKind::Union),
          ..Default::default()
        }
      }
      TsIntersectionType(intersection_type) => {
        let mut types_intersection = vec![];

        for type_box in &intersection_type.types {
          let ts_type: &TsType = &(*type_box);
          let def: TsTypeDef = ts_type.into();
          types_intersection.push(def);
        }

        TsTypeDef {
          intersection: Some(types_intersection),
          kind: Some(TsTypeDefKind::Intersection),
          ..Default::default()
        }
      }
    }
  }
}

impl Into<TsTypeDef> for &TsKeywordType {
  fn into(self) -> TsTypeDef {
    use swc_ecma_ast::TsKeywordTypeKind::*;

    let keyword_str = match self.kind {
      TsAnyKeyword => "any",
      TsUnknownKeyword => "unknown",
      TsNumberKeyword => "number",
      TsObjectKeyword => "object",
      TsBooleanKeyword => "boolean",
      TsBigIntKeyword => "bigint",
      TsStringKeyword => "string",
      TsSymbolKeyword => "symbol",
      TsVoidKeyword => "void",
      TsUndefinedKeyword => "undefined",
      TsNullKeyword => "null",
      TsNeverKeyword => "never",
    };

    TsTypeDef {
      repr: keyword_str.to_string(),
      kind: Some(TsTypeDefKind::Keyword),
      keyword: Some(keyword_str.to_string()),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsTypeOperator {
  fn into(self) -> TsTypeDef {
    let ts_type = (&*self.type_ann).into();
    let type_operator_def = TsTypeOperatorDef {
      operator: self.op.as_str().to_string(),
      ts_type,
    };

    TsTypeDef {
      type_operator: Some(Box::new(type_operator_def)),
      kind: Some(TsTypeDefKind::TypeOperator),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsParenthesizedType {
  fn into(self) -> TsTypeDef {
    let ts_type = (&*self.type_ann).into();

    TsTypeDef {
      parenthesized: Some(Box::new(ts_type)),
      kind: Some(TsTypeDefKind::Parenthesized),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsRestType {
  fn into(self) -> TsTypeDef {
    let ts_type = (&*self.type_ann).into();

    TsTypeDef {
      rest: Some(Box::new(ts_type)),
      kind: Some(TsTypeDefKind::Rest),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsOptionalType {
  fn into(self) -> TsTypeDef {
    let ts_type = (&*self.type_ann).into();

    TsTypeDef {
      optional: Some(Box::new(ts_type)),
      kind: Some(TsTypeDefKind::Optional),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsThisType {
  fn into(self) -> TsTypeDef {
    TsTypeDef {
      repr: "this".to_string(),
      this: Some(true),
      kind: Some(TsTypeDefKind::This),
      ..Default::default()
    }
  }
}

pub fn ts_entity_name_to_name(
  entity_name: &swc_ecma_ast::TsEntityName,
) -> String {
  use swc_ecma_ast::TsEntityName::*;

  match entity_name {
    Ident(ident) => ident.sym.to_string(),
    TsQualifiedName(ts_qualified_name) => {
      let left = ts_entity_name_to_name(&ts_qualified_name.left);
      let right = ts_qualified_name.right.sym.to_string();
      format!("{}.{}", left, right)
    }
  }
}

impl Into<TsTypeDef> for &TsTypeQuery {
  fn into(self) -> TsTypeDef {
    use swc_ecma_ast::TsTypeQueryExpr::*;

    let type_name = match &self.expr_name {
      TsEntityName(entity_name) => ts_entity_name_to_name(&*entity_name),
      Import(import_type) => import_type.arg.value.to_string(),
    };

    TsTypeDef {
      repr: type_name.to_string(),
      type_query: Some(type_name),
      kind: Some(TsTypeDefKind::TypeQuery),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsTypeRef {
  fn into(self) -> TsTypeDef {
    let type_name = ts_entity_name_to_name(&self.type_name);

    let type_params = if let Some(type_params_inst) = &self.type_params {
      let mut ts_type_defs = vec![];

      for type_box in &type_params_inst.params {
        let ts_type: &TsType = &(*type_box);
        let def: TsTypeDef = ts_type.into();
        ts_type_defs.push(def);
      }

      Some(ts_type_defs)
    } else {
      None
    };

    TsTypeDef {
      repr: type_name.to_string(),
      type_ref: Some(TsTypeRefDef {
        type_name,
        type_params,
      }),
      kind: Some(TsTypeDefKind::TypeRef),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsIndexedAccessType {
  fn into(self) -> TsTypeDef {
    let indexed_access_def = TsIndexedAccessDef {
      readonly: self.readonly,
      obj_type: Box::new((&*self.obj_type).into()),
      index_type: Box::new((&*self.index_type).into()),
    };

    TsTypeDef {
      indexed_access: Some(indexed_access_def),
      kind: Some(TsTypeDefKind::IndexedAccess),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsTypeLit {
  fn into(self) -> TsTypeDef {
    let mut methods = vec![];
    let mut properties = vec![];
    let mut call_signatures = vec![];

    for type_element in &self.members {
      use swc_ecma_ast::TsTypeElement::*;

      match &type_element {
        TsMethodSignature(ts_method_sig) => {
          let mut params = vec![];

          for param in &ts_method_sig.params {
            use swc_ecma_ast::TsFnParam::*;

            let param_def = match param {
              Ident(ident) => {
                let ts_type =
                  ident.type_ann.as_ref().map(|rt| (&*rt.type_ann).into());

                ParamDef {
                  name: ident.sym.to_string(),
                  ts_type,
                }
              }
              _ => ParamDef {
                name: "<TODO>".to_string(),
                ts_type: None,
              },
            };

            params.push(param_def);
          }

          let maybe_return_type = ts_method_sig
            .type_ann
            .as_ref()
            .map(|rt| (&*rt.type_ann).into());

          let method_def = LiteralMethodDef {
            name: "<TODO>".to_string(),
            params,
            return_type: maybe_return_type,
          };
          methods.push(method_def);
        }
        TsPropertySignature(ts_prop_sig) => {
          let name = match &*ts_prop_sig.key {
            swc_ecma_ast::Expr::Ident(ident) => ident.sym.to_string(),
            _ => "TODO".to_string(),
          };

          let mut params = vec![];

          for param in &ts_prop_sig.params {
            use swc_ecma_ast::TsFnParam::*;

            let param_def = match param {
              Ident(ident) => {
                let ts_type =
                  ident.type_ann.as_ref().map(|rt| (&*rt.type_ann).into());

                ParamDef {
                  name: ident.sym.to_string(),
                  ts_type,
                }
              }
              _ => ParamDef {
                name: "<TODO>".to_string(),
                ts_type: None,
              },
            };

            params.push(param_def);
          }

          let ts_type = ts_prop_sig
            .type_ann
            .as_ref()
            .map(|rt| (&*rt.type_ann).into());

          let prop_def = LiteralPropertyDef {
            name,
            params,
            ts_type,
            computed: ts_prop_sig.computed,
            optional: ts_prop_sig.optional,
          };
          properties.push(prop_def);
        }
        TsCallSignatureDecl(ts_call_sig) => {
          let mut params = vec![];
          for param in &ts_call_sig.params {
            use swc_ecma_ast::TsFnParam::*;

            let param_def = match param {
              Ident(ident) => {
                let ts_type =
                  ident.type_ann.as_ref().map(|rt| (&*rt.type_ann).into());

                ParamDef {
                  name: ident.sym.to_string(),
                  ts_type,
                }
              }
              _ => ParamDef {
                name: "<TODO>".to_string(),
                ts_type: None,
              },
            };

            params.push(param_def);
          }

          let ts_type = ts_call_sig
            .type_ann
            .as_ref()
            .map(|rt| (&*rt.type_ann).into());

          let call_sig_def = LiteralCallSignatureDef { params, ts_type };
          call_signatures.push(call_sig_def);
        }
        // TODO:
        TsConstructSignatureDecl(_) => {}
        TsIndexSignature(_) => {}
      }
    }

    let type_literal = TsTypeLiteralDef {
      methods,
      properties,
      call_signatures,
    };

    TsTypeDef {
      kind: Some(TsTypeDefKind::TypeLiteral),
      type_literal: Some(type_literal),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsConditionalType {
  fn into(self) -> TsTypeDef {
    let conditional_type_def = TsConditionalDef {
      check_type: Box::new((&*self.check_type).into()),
      extends_type: Box::new((&*self.extends_type).into()),
      true_type: Box::new((&*self.true_type).into()),
      false_type: Box::new((&*self.false_type).into()),
    };

    TsTypeDef {
      kind: Some(TsTypeDefKind::Conditional),
      conditional_type: Some(conditional_type_def),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsFnOrConstructorType {
  fn into(self) -> TsTypeDef {
    use swc_ecma_ast::TsFnOrConstructorType::*;

    let fn_def = match self {
      TsFnType(ts_fn_type) => {
        let mut params = vec![];

        for param in &ts_fn_type.params {
          use swc_ecma_ast::TsFnParam::*;

          let param_def = match param {
            Ident(ident) => {
              let ts_type: Option<TsTypeDef> =
                ident.type_ann.as_ref().map(|rt| {
                  let type_box = &*rt.type_ann;
                  (&*type_box).into()
                });

              ParamDef {
                name: ident.sym.to_string(),
                ts_type,
              }
            }
            _ => ParamDef {
              name: "<TODO>".to_string(),
              ts_type: None,
            },
          };

          params.push(param_def);
        }

        TsFnOrConstructorDef {
          constructor: false,
          ts_type: (&*ts_fn_type.type_ann.type_ann).into(),
          params,
        }
      }
      TsConstructorType(ctor_type) => {
        let mut params = vec![];

        for param in &ctor_type.params {
          use swc_ecma_ast::TsFnParam::*;

          let param_def = match param {
            Ident(ident) => {
              let ts_type: Option<TsTypeDef> =
                ident.type_ann.as_ref().map(|rt| {
                  let type_box = &*rt.type_ann;
                  (&*type_box).into()
                });

              ParamDef {
                name: ident.sym.to_string(),
                ts_type,
              }
            }
            _ => ParamDef {
              name: "<TODO>".to_string(),
              ts_type: None,
            },
          };

          params.push(param_def);
        }

        TsFnOrConstructorDef {
          constructor: true,
          ts_type: (&*ctor_type.type_ann.type_ann).into(),
          params: vec![],
        }
      }
    };

    TsTypeDef {
      kind: Some(TsTypeDefKind::FnOrConstructor),
      fn_or_constructor: Some(Box::new(fn_def)),
      ..Default::default()
    }
  }
}

impl Into<TsTypeDef> for &TsType {
  fn into(self) -> TsTypeDef {
    use swc_ecma_ast::TsType::*;

    match self {
      TsKeywordType(ref keyword_type) => keyword_type.into(),
      TsLitType(ref lit_type) => lit_type.into(),
      TsTypeRef(ref type_ref) => type_ref.into(),
      TsUnionOrIntersectionType(union_or_inter) => union_or_inter.into(),
      TsArrayType(array_type) => array_type.into(),
      TsTupleType(tuple_type) => tuple_type.into(),
      TsTypeOperator(type_op_type) => type_op_type.into(),
      TsParenthesizedType(paren_type) => paren_type.into(),
      TsRestType(rest_type) => rest_type.into(),
      TsOptionalType(optional_type) => optional_type.into(),
      TsTypeQuery(type_query) => type_query.into(),
      TsThisType(this_type) => this_type.into(),
      TsFnOrConstructorType(fn_or_con_type) => fn_or_con_type.into(),
      TsConditionalType(conditional_type) => conditional_type.into(),
      TsIndexedAccessType(indexed_access_type) => indexed_access_type.into(),
      TsTypeLit(type_literal) => type_literal.into(),
      _ => TsTypeDef {
        repr: "<UNIMPLEMENTED>".to_string(),
        ..Default::default()
      },
    }
  }
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsTypeRefDef {
  pub type_params: Option<Vec<TsTypeDef>>,
  pub type_name: String,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum LiteralDefKind {
  Number,
  String,
  Boolean,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiteralDef {
  pub kind: LiteralDefKind,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub number: Option<f64>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub string: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub boolean: Option<bool>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsTypeOperatorDef {
  pub operator: String,
  pub ts_type: TsTypeDef,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsFnOrConstructorDef {
  // TODO: type_params
  pub constructor: bool,
  pub ts_type: TsTypeDef,
  pub params: Vec<ParamDef>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsConditionalDef {
  pub check_type: Box<TsTypeDef>,
  pub extends_type: Box<TsTypeDef>,
  pub true_type: Box<TsTypeDef>,
  pub false_type: Box<TsTypeDef>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsIndexedAccessDef {
  pub readonly: bool,
  pub obj_type: Box<TsTypeDef>,
  pub index_type: Box<TsTypeDef>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiteralMethodDef {
  // TODO: type_params
  pub name: String,
  // pub location: Location,
  // pub js_doc: Option<String>,
  pub params: Vec<ParamDef>,
  pub return_type: Option<TsTypeDef>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiteralPropertyDef {
  // TODO: type_params
  pub name: String,
  // pub location: Location,
  // pub js_doc: Option<String>,
  pub params: Vec<ParamDef>,
  pub computed: bool,
  pub optional: bool,
  pub ts_type: Option<TsTypeDef>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct LiteralCallSignatureDef {
  // TODO: type_params
  // pub location: Location,
  // pub js_doc: Option<String>,
  pub params: Vec<ParamDef>,
  pub ts_type: Option<TsTypeDef>,
}

#[derive(Debug, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsTypeLiteralDef {
  pub methods: Vec<LiteralMethodDef>,
  pub properties: Vec<LiteralPropertyDef>,
  pub call_signatures: Vec<LiteralCallSignatureDef>,
}

#[derive(Debug, PartialEq, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub enum TsTypeDefKind {
  Keyword,
  Literal,
  TypeRef,
  Union,
  Intersection,
  Array,
  Tuple,
  TypeOperator,
  Parenthesized,
  Rest,
  Optional,
  TypeQuery,
  This,
  FnOrConstructor,
  Conditional,
  IndexedAccess,
  TypeLiteral,
}

#[derive(Debug, Default, Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TsTypeDef {
  pub repr: String,

  pub kind: Option<TsTypeDefKind>,

  // TODO: make this struct more conrete
  #[serde(skip_serializing_if = "Option::is_none")]
  pub keyword: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub literal: Option<LiteralDef>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub type_ref: Option<TsTypeRefDef>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub union: Option<Vec<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub intersection: Option<Vec<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub array: Option<Box<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub tuple: Option<Vec<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub type_operator: Option<Box<TsTypeOperatorDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub parenthesized: Option<Box<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub rest: Option<Box<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub optional: Option<Box<TsTypeDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub type_query: Option<String>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub this: Option<bool>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub fn_or_constructor: Option<Box<TsFnOrConstructorDef>>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub conditional_type: Option<TsConditionalDef>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub indexed_access: Option<TsIndexedAccessDef>,

  #[serde(skip_serializing_if = "Option::is_none")]
  pub type_literal: Option<TsTypeLiteralDef>,
}

pub fn ts_type_ann_to_def(
  source_map: &SourceMap,
  type_ann: &TsTypeAnn,
) -> TsTypeDef {
  use swc_ecma_ast::TsType::*;

  match &*type_ann.type_ann {
    TsKeywordType(keyword_type) => keyword_type.into(),
    TsLitType(lit_type) => lit_type.into(),
    TsTypeRef(type_ref) => type_ref.into(),
    TsUnionOrIntersectionType(union_or_inter) => union_or_inter.into(),
    TsArrayType(array_type) => array_type.into(),
    TsTupleType(tuple_type) => tuple_type.into(),
    TsTypeOperator(type_op_type) => type_op_type.into(),
    TsParenthesizedType(paren_type) => paren_type.into(),
    TsRestType(rest_type) => rest_type.into(),
    TsOptionalType(optional_type) => optional_type.into(),
    TsTypeQuery(type_query) => type_query.into(),
    TsThisType(this_type) => this_type.into(),
    TsFnOrConstructorType(fn_or_con_type) => fn_or_con_type.into(),
    TsConditionalType(conditional_type) => conditional_type.into(),
    TsIndexedAccessType(indexed_access_type) => indexed_access_type.into(),
    TsTypeLit(type_literal) => type_literal.into(),
    _ => {
      let repr = source_map
        .span_to_snippet(type_ann.span)
        .expect("Class prop type not found");
      let repr = repr.trim_start_matches(':').trim_start().to_string();

      TsTypeDef {
        repr,
        ..Default::default()
      }
    }
  }
}
