// 実行時の値
//
// 値の独立性：代入するとコピーされたように振る舞う。
// 中身は Rc で共有し、変更するときだけ Rc::make_mut で複製する（Copy-on-Write）。

use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;

use crate::ast::TypeExpr;

#[derive(Debug, PartialEq)]
pub struct TypeDef {
    pub name: String,
    pub fields: Vec<String>,
    pub types: Vec<Option<TypeExpr>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructVal {
    pub def: Rc<TypeDef>,
    pub fields: Vec<Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 値を返さない関数の結果。利用者が直接触ることはない
    None,
    Int(i64),
    Bool(bool),
    Str(Rc<str>),
    Array(Rc<Vec<Value>>),
    Struct(Rc<StructVal>),
}

/// 見た目の1文字（書記素クラスタ）単位に分ける
pub fn graphemes(s: &str) -> Vec<&str> {
    s.graphemes(true).collect()
}

impl Value {
    /// エラーメッセージ用の種類の名前
    pub fn kind_name(&self) -> String {
        match self {
            Value::None => "なし".into(),
            Value::Int(_) => "数".into(),
            Value::Bool(_) => "true/false".into(),
            Value::Str(_) => "文字".into(),
            Value::Array(_) => "配列".into(),
            Value::Struct(s) => s.def.name.clone(),
        }
    }

    /// 同じ種類の値か（配列の中身までは見ない）
    pub fn same_kind(&self, other: &Value) -> bool {
        match (self, other) {
            (Value::Struct(a), Value::Struct(b)) => a.def.name == b.def.name,
            _ => std::mem::discriminant(self) == std::mem::discriminant(other),
        }
    }

    /// print や "{x}" で表示するときの文字列
    pub fn show(&self) -> String {
        match self {
            Value::Str(s) => s.to_string(),
            other => other.show_inner(),
        }
    }

    fn show_inner(&self) -> String {
        match self {
            Value::None => "なし".into(),
            Value::Int(n) => n.to_string(),
            Value::Bool(b) => b.to_string(),
            Value::Str(s) => format!("\"{s}\""),
            Value::Array(xs) => {
                let inner: Vec<String> = xs.iter().map(|x| x.show_inner()).collect();
                format!("[{}]", inner.join(", "))
            }
            Value::Struct(s) => {
                let inner: Vec<String> = s
                    .def
                    .fields
                    .iter()
                    .zip(&s.fields)
                    .map(|(n, v)| format!("{n}: {}", v.show_inner()))
                    .collect();
                format!("{}({})", s.def.name, inner.join(", "))
            }
        }
    }
}

pub fn type_matches(v: &Value, ty: &TypeExpr) -> bool {
    match (ty, v) {
        (TypeExpr::Named(n), Value::Int(_)) => n == "int",
        (TypeExpr::Named(n), Value::Str(_)) => n == "string",
        (TypeExpr::Named(n), Value::Bool(_)) => n == "bool",
        (TypeExpr::Named(n), Value::Struct(s)) => &s.def.name == n,
        (TypeExpr::Array(t), Value::Array(xs)) => xs.iter().all(|x| type_matches(x, t)),
        _ => false,
    }
}
