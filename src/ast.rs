// 構文木（AST）の定義

use std::collections::HashMap;

/// ソースコード上の位置。col と len は文字単位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub line: usize,
    pub col: usize,
    pub len: usize,
}

impl Span {
    pub fn new(line: usize, col: usize, len: usize) -> Self {
        Span { line, col, len }
    }

    /// self の始まりから other の終わりまで（同じ行のときだけ広げる）
    pub fn to(self, other: Span) -> Span {
        if self.line == other.line && other.col + other.len >= self.col {
            Span::new(self.line, self.col, other.col + other.len - self.col)
        } else {
            self
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    Named(String),
    Array(Box<TypeExpr>),
}

impl TypeExpr {
    pub fn show(&self) -> String {
        match self {
            TypeExpr::Named(n) => n.clone(),
            TypeExpr::Array(t) => format!("[]{}", t.show()),
        }
    }

    pub(crate) fn element(&self) -> Option<&Self> {
        match self {
            Self::Array(ty) => Some(ty),
            Self::Named(_) => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct Program {
    pub funcs: Vec<FuncDecl>,
    pub types: Vec<TypeDecl>,
    pub body: Vec<Stmt>,
}

#[derive(Debug)]
pub struct FuncDecl {
    pub name: String,
    pub span: Span,
    pub params: Vec<Param>,
    pub ret: Option<TypeExpr>,
    pub body: Vec<Stmt>,
}

/// 関数の引数、または型の項目
#[derive(Debug, Clone)]
pub struct Param {
    pub name: String,
    pub span: Span,
    pub ty: Option<TypeExpr>,
}

#[derive(Debug)]
pub struct TypeDecl {
    pub name: String,
    pub span: Span,
    pub fields: Vec<Param>,
}

pub(crate) type TypeDecls<'a> = HashMap<&'a str, &'a TypeDecl>;
pub(crate) type VariableTypes<'a> = HashMap<&'a str, &'a TypeExpr>;

#[derive(Debug)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignOp {
    Set,
    Add,
    Sub,
}

#[derive(Debug)]
pub enum Access {
    Field(String, Span),
    Index(Expr),
}

/// 代入先。例: x / user.age / xs[1] / board[y][x]
#[derive(Debug)]
pub struct Target {
    pub name: String,
    pub span: Span,
    pub path: Vec<Access>,
}

#[derive(Debug)]
pub enum StmtKind {
    Assign {
        target: Target,
        annotation: Option<(TypeExpr, Span)>,
        op: AssignOp,
        value: Expr,
    },
    Expr(Expr),
    If {
        cond: Expr,
        then: Vec<Stmt>,
        els: Option<Vec<Stmt>>,
    },
    ForIn {
        var: String,
        var_span: Span,
        index: Option<(String, Span)>,
        iter: Expr,
        body: Vec<Stmt>,
    },
    /// for 条件 { } / for { }
    While {
        cond: Option<Expr>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    Break,
    Continue,
}

/// 型注釈は変数と同じく、トップレベルまたは関数全体に効く。
pub(crate) fn variable_types(stmts: &[Stmt]) -> VariableTypes<'_> {
    fn collect<'a>(stmts: &'a [Stmt], types: &mut VariableTypes<'a>) {
        for stmt in stmts {
            match &stmt.kind {
                StmtKind::Assign { target, annotation: Some((ty, _)), .. } => {
                    types.entry(&target.name).or_insert(ty);
                }
                StmtKind::If { then, els, .. } => {
                    collect(then, types);
                    if let Some(els) = els {
                        collect(els, types);
                    }
                }
                StmtKind::ForIn { body, .. } | StmtKind::While { body, .. } => collect(body, types),
                _ => {}
            }
        }
    }
    let mut types = HashMap::new();
    collect(stmts, &mut types);
    types
}

pub(crate) fn assignment_value_type<'a>(
    target: &Target,
    op: AssignOp,
    vars: &VariableTypes<'a>,
    types: &TypeDecls<'a>,
) -> Option<&'a TypeExpr> {
    let mut ty = *vars.get(target.name.as_str())?;
    for access in &target.path {
        ty = match (access, ty) {
            (Access::Index(_), TypeExpr::Array(inner)) => inner,
            (Access::Field(name, _), TypeExpr::Named(ty_name)) => {
                let decl = types.get(ty_name.as_str())?;
                decl.fields.iter().find(|field| field.name == *name)?.ty.as_ref()?
            }
            _ => return None,
        };
    }
    if op == AssignOp::Add && let Some(inner) = ty.element() {
        return Some(inner);
    }
    Some(ty)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Eq => "==",
            BinOp::NotEq => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::And => "and",
            BinOp::Or => "or",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Debug)]
pub enum StrPart {
    Lit(String),
    Expr(Expr),
}

#[derive(Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Debug)]
pub struct NamedField {
    pub name: String,
    pub span: Span,
    pub value: Expr,
}

#[derive(Debug)]
pub enum ExprKind {
    Int(i64),
    Bool(bool),
    Str(Vec<StrPart>),
    Array(Vec<Expr>),
    Record(Vec<NamedField>),
    Var(String),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    /// a..b（両端を含む）
    Range(Box<Expr>, Box<Expr>),
    /// f(x) または 型の作成 User(...)
    Call {
        name: String,
        args: Vec<Expr>,
    },
    /// x.f(y) は f(x, y) と同じ
    Method {
        recv: Box<Expr>,
        name: String,
        name_span: Span,
        args: Vec<Expr>,
    },
    Field {
        recv: Box<Expr>,
        name: String,
        name_span: Span,
    },
    Index {
        recv: Box<Expr>,
        index: Box<Expr>,
    },
}
