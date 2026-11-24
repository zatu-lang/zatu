// 構文木をそのままたどって実行するインタプリタ

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, Write};
use std::rc::Rc;

use crate::ast::*;
use crate::check::returns_value;
use crate::error::ZatuError;
use crate::value::*;

type R<T> = Result<T, ZatuError>;
/// 変数の置き場所。関数1回の呼び出しにつき1つ
type Env = HashMap<String, Value>;

const MAX_DEPTH: usize = 3000;
const MAX_RANGE: i64 = 10_000_000;

enum Flow {
    Normal,
    Break,
    Continue,
    Return(Value),
}

enum Key<'a> {
    Field(&'a str, Span),
    Index(Value, Span),
}

pub struct Interp<'p, 'io> {
    prog: &'p Program,
    funcs: HashMap<&'p str, &'p FuncDecl>,
    returns: HashSet<&'p str>,
    types: HashMap<&'p str, Rc<TypeDef>>,
    type_decls: TypeDecls<'p>,
    top_types: Rc<VariableTypes<'p>>,
    local_types: HashMap<&'p str, Rc<VariableTypes<'p>>>,
    out: &'io mut dyn Write,
    input: &'io mut dyn BufRead,
    depth: usize,
    rng: u64,
}

fn err(sp: Span, msg: impl Into<String>) -> ZatuError {
    ZatuError::new(sp, msg)
}

fn truth(v: &Value, sp: Span, ctx: &str) -> R<bool> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => Err(err(sp, format!("{ctx} には true か false になる式を書きます（今は{}）", other.kind_name()))
            .hint("例: x > 3、name == \"ようかん\"")),
    }
}

/// 1 から始まる番号を、内部の 0 から始まる位置に直す
fn index_of(i: &Value, len: usize, sp: Span) -> R<usize> {
    match i {
        Value::Int(0) => Err(err(sp, "番号は 1 から始まります（[0] はありません）")),
        Value::Int(n) if *n >= 1 && (*n as usize) <= len => Ok(*n as usize - 1),
        Value::Int(n) if len == 0 => Err(err(sp, format!("中身が空なので [{n}] は取り出せません"))),
        Value::Int(n) => Err(err(sp, format!("{len} 個しかないので [{n}] は取り出せません")).hint(format!("使える番号は 1 〜 {len} です"))),
        other => Err(err(sp, format!("[ ] の中には数を書きます（今は{}）", other.kind_name()))),
    }
}

/// 配列の中身は同じ種類の値にそろえる
fn check_elem(arr: &[Value], v: &Value, sp: Span) -> R<()> {
    if let Some(first) = arr.first()
        && !first.same_kind(v) {
            let mut e = err(sp, format!("この配列には{}が入っています。{}は入れられません", first.kind_name(), v.kind_name()));
            if matches!(v, Value::Array(_)) {
                e = e.hint("+= は要素を1つ追加します。配列どうしをつなげる書き方は v0 にはまだありません（for で1つずつ += してください）");
            }
            return Err(e);
        }
    Ok(())
}

fn kind_change(name: &str, old: &Value, new: &Value, sp: Span) -> ZatuError {
    err(sp, format!("{name} には{}が入っています。{}は入れられません", old.kind_name(), new.kind_name()))
        .hint("ちがう種類の値を入れたいときは、別の名前の変数を作ってください")
}

pub fn binop(op: BinOp, l: Value, r: Value, sp: Span) -> R<Value> {
    use Value::*;
    let overflow = || err(sp, "数が大きすぎて計算できません");
    let cmp = |o: std::cmp::Ordering| {
        use std::cmp::Ordering::*;
        Bool(match op {
            BinOp::Lt => o == Less,
            BinOp::Le => o != Greater,
            BinOp::Gt => o == Greater,
            _ => o != Less,
        })
    };
    match (op, l, r) {
        (BinOp::Add, Int(a), Int(b)) => a.checked_add(b).map(Int).ok_or_else(overflow),
        (BinOp::Sub, Int(a), Int(b)) => a.checked_sub(b).map(Int).ok_or_else(overflow),
        (BinOp::Mul, Int(a), Int(b)) => a.checked_mul(b).map(Int).ok_or_else(overflow),
        (BinOp::Div | BinOp::Mod, Int(_), Int(0)) => Err(err(sp, "0 で割ることはできません")),
        (BinOp::Div, Int(a), Int(b)) => {
            let rem = a.checked_rem(b).ok_or_else(overflow)?;
            if rem != 0 {
                return Err(err(sp, format!("{a} / {b} は割り切れません（{} あまり {rem}）", a / b))
                    .hint("あまりを知りたいときは % を使います。小数はまだ使えません（v0）"));
            }
            a.checked_div(b).map(Int).ok_or_else(overflow)
        }
        (BinOp::Mod, Int(a), Int(b)) => a.checked_rem(b).map(Int).ok_or_else(overflow),
        (BinOp::Add, Str(a), Str(b)) => Ok(Str(format!("{a}{b}").into())),
        (BinOp::Add, Array(mut a), v) => {
            check_elem(&a, &v, sp)?;
            Rc::make_mut(&mut a).push(v);
            Ok(Array(a))
        }
        (BinOp::Add, l @ Str(_), r) | (BinOp::Add, l, r @ Str(_)) => {
            Err(err(sp, format!("{}と{}は + でつなげられません", l.kind_name(), r.kind_name()))
                .hint("文字の中に値を入れたいときは \"答えは{x}です\" のように書きます"))
        }
        (BinOp::Eq | BinOp::NotEq, l, r) => {
            if !l.same_kind(&r) {
                return Err(err(sp, format!("{}と{}はくらべられません", l.kind_name(), r.kind_name())));
            }
            Ok(Bool((l == r) == (op == BinOp::Eq)))
        }
        (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, Int(a), Int(b)) => Ok(cmp(a.cmp(&b))),
        (BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge, Str(a), Str(b)) => Ok(cmp(a.cmp(&b))),
        (op, l, r) => Err(err(sp, format!("{}と{}に {} は使えません", l.kind_name(), r.kind_name(), op.symbol()))),
    }
}

/// 代入先までたどって、その場所を返す。途中の共有データはここで複製される（Copy-on-Write）
fn navigate<'v>(mut cur: &'v mut Value, keys: &[Key]) -> R<&'v mut Value> {
    for k in keys {
        cur = match (cur, k) {
            (Value::Struct(rc), Key::Field(name, sp)) => {
                let s = Rc::make_mut(rc);
                let Some(i) = s.def.fields.iter().position(|f| f == name) else {
                    return Err(err(*sp, format!("{} には {name} という項目はありません", s.def.name))
                        .hint(format!("項目: {}", s.def.fields.join(", "))));
                };
                &mut s.fields[i]
            }
            (Value::Array(rc), Key::Index(iv, sp)) => {
                let i = index_of(iv, rc.len(), *sp)?;
                &mut Rc::make_mut(rc)[i]
            }
            (Value::Str(_), Key::Index(_, sp)) => {
                return Err(err(*sp, "文字の一部だけを書き換えることはできません"));
            }
            (other, Key::Field(name, sp)) => {
                return Err(err(*sp, format!("{}には {name} という項目はありません", other.kind_name())));
            }
            (other, Key::Index(_, sp)) => {
                return Err(err(*sp, format!("{}は [ ] で取り出せません", other.kind_name())));
            }
        };
    }
    Ok(cur)
}

fn describe_target(t: &Target) -> String {
    let mut s = t.name.clone();
    for a in &t.path {
        match a {
            Access::Field(n, _) => {
                s.push('.');
                s.push_str(n);
            }
            Access::Index(_) => s.push_str("[…]"),
        }
    }
    s
}

impl<'p, 'io> Interp<'p, 'io> {
    pub fn new(prog: &'p Program, out: &'io mut dyn Write, input: &'io mut dyn BufRead) -> Self {
        let funcs = prog.funcs.iter().map(|f| (f.name.as_str(), f)).collect();
        let returns = prog.funcs.iter().filter(|f| returns_value(&f.body)).map(|f| f.name.as_str()).collect();
        let type_decls = prog.types.iter().map(|t| (t.name.as_str(), t)).collect();
        let top_types = Rc::new(variable_types(&prog.body));
        let local_types = prog.funcs.iter().map(|f| (f.name.as_str(), Rc::new(variable_types(&f.body)))).collect();
        let types = prog
            .types
            .iter()
            .map(|t| {
                let def = TypeDef {
                    name: t.name.clone(),
                    fields: t.fields.iter().map(|f| f.name.clone()).collect(),
                    types: t.fields.iter().map(|f| f.ty.clone()).collect(),
                };
                (t.name.as_str(), Rc::new(def))
            })
            .collect();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x2545F4914F6CDD1D);
        Interp { prog, funcs, returns, types, type_decls, top_types, local_types, out, input, depth: 0, rng: seed | 1 }
    }

    pub fn run(&mut self) -> R<()> {
        let prog = self.prog;
        let mut env = Env::new();
        self.block(&prog.body, &mut env, &self.top_types.clone())?;
        let _ = self.out.flush();
        Ok(())
    }

    fn next_rand(&mut self) -> u64 {
        // xorshift64
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        x
    }

    fn block(&mut self, ss: &[Stmt], env: &mut Env, types: &VariableTypes<'p>) -> R<Flow> {
        for s in ss {
            match self.stmt(s, env, types)? {
                Flow::Normal => {}
                f => return Ok(f),
            }
        }
        Ok(Flow::Normal)
    }

    fn stmt(&mut self, s: &Stmt, env: &mut Env, types: &VariableTypes<'p>) -> R<Flow> {
        match &s.kind {
            StmtKind::Assign { target, op, value, .. } => {
                let expected = assignment_value_type(target, *op, types, &self.type_decls);
                let v = self.eval_with_type(value, expected, env)?;
                self.assign(target, *op, v, value.span, env)?;
            }
            StmtKind::Expr(e) => {
                self.eval(e, env)?;
            }
            StmtKind::If { cond, then, els } => {
                let c = self.eval(cond, env)?;
                if truth(&c, cond.span, "if の条件")? {
                    return self.block(then, env, types);
                } else if let Some(e) = els {
                    return self.block(e, env, types);
                }
            }
            StmtKind::ForIn { var, index, iter, body, .. } => {
                let items: Rc<Vec<Value>> = match self.eval(iter, env)? {
                    // くり返す前の状態を使う。ループの中で配列を変えても、くり返しの回数は変わらない
                    Value::Array(a) => a,
                    Value::Str(s) => Rc::new(graphemes(&s).into_iter().map(|g| Value::Str(g.into())).collect()),
                    other => {
                        return Err(err(iter.span, format!("{}は for でくり返せません", other.kind_name()))
                            .hint("1..10 と書くと、1 から 10 までくり返せます"));
                    }
                };
                let mut result = Flow::Normal;
                for (i, item) in items.iter().enumerate() {
                    env.insert(var.clone(), item.clone());
                    if let Some((iname, _)) = index {
                        env.insert(iname.clone(), Value::Int(i as i64 + 1));
                    }
                    match self.block(body, env, types)? {
                        Flow::Break => break,
                        Flow::Return(v) => {
                            result = Flow::Return(v);
                            break;
                        }
                        Flow::Normal | Flow::Continue => {}
                    }
                }
                env.remove(var);
                if let Some((iname, _)) = index {
                    env.remove(iname);
                }
                return Ok(result);
            }
            StmtKind::While { cond, body } => loop {
                if let Some(c) = cond {
                    let v = self.eval(c, env)?;
                    if !truth(&v, c.span, "for の条件")? {
                        break;
                    }
                }
                match self.block(body, env, types)? {
                    Flow::Break => break,
                    Flow::Return(v) => return Ok(Flow::Return(v)),
                    Flow::Normal | Flow::Continue => {}
                }
            },
            StmtKind::Return(v) => {
                let v = match v {
                    Some(e) => self.eval(e, env)?,
                    None => Value::None,
                };
                return Ok(Flow::Return(v));
            }
            StmtKind::Break => return Ok(Flow::Break),
            StmtKind::Continue => return Ok(Flow::Continue),
        }
        Ok(Flow::Normal)
    }

    fn assign(&mut self, t: &Target, op: AssignOp, v: Value, sp: Span, env: &mut Env) -> R<()> {
        let mut keys = Vec::with_capacity(t.path.len());
        for a in &t.path {
            keys.push(match a {
                Access::Field(n, s) => Key::Field(n.as_str(), *s),
                Access::Index(e) => Key::Index(self.eval(e, env)?, e.span),
            });
        }
        if keys.is_empty() && op == AssignOp::Set {
            if let Some(old) = env.get(&t.name)
                && !old.same_kind(&v) {
                    return Err(kind_change(&t.name, old, &v, sp));
                }
            env.insert(t.name.clone(), v);
            return Ok(());
        }
        let slot = env
            .get_mut(&t.name)
            .ok_or_else(|| err(t.span, format!("{} にはまだ値が入っていません", t.name)))?;
        let slot = navigate(slot, &keys)?;
        match op {
            AssignOp::Set => {
                if !slot.same_kind(&v) {
                    return Err(kind_change(&describe_target(t), slot, &v, sp));
                }
                *slot = v;
            }
            AssignOp::Add => {
                if let Value::Array(rc) = slot {
                    // 配列への += はその場で追加する（共有されていなければ複製しない）
                    check_elem(rc, &v, sp)?;
                    Rc::make_mut(rc).push(v);
                } else {
                    let cur = std::mem::replace(slot, Value::None);
                    *slot = binop(BinOp::Add, cur, v, sp)?;
                }
            }
            AssignOp::Sub => {
                let cur = std::mem::replace(slot, Value::None);
                *slot = binop(BinOp::Sub, cur, v, sp)?;
            }
        }
        Ok(())
    }

    fn eval_with_type(&mut self, e: &Expr, expected: Option<&TypeExpr>, env: &mut Env) -> R<Value> {
        let Some(ty) = expected else { return self.eval(e, env) };
        match (&e.kind, ty) {
            (ExprKind::Record(fields), TypeExpr::Named(name)) => self.record(name, fields, e.span, env),
            (ExprKind::Array(xs), TypeExpr::Array(inner)) => {
                let mut values = Vec::with_capacity(xs.len());
                for x in xs {
                    values.push(self.eval_with_type(x, Some(inner), env)?);
                }
                Ok(Value::Array(Rc::new(values)))
            }
            _ => {
                let value = self.eval(e, env)?;
                if type_matches(&value, ty) {
                    return Ok(value);
                }
                let message = match (ty, &value) {
                    (TypeExpr::Array(inner), Value::Array(_)) => {
                        format!("ここには {} が入ります。配列の中身を {} にそろえてください", ty.show(), inner.show())
                    }
                    _ => format!("ここには {} が入ります（今は{}）", ty.show(), value.kind_name()),
                };
                Err(err(e.span, message))
            }
        }
    }

    fn record(&mut self, name: &str, fields: &[NamedField], span: Span, env: &mut Env) -> R<Value> {
        let def = self.types.get(name).cloned()
            .ok_or_else(|| err(span, format!("{name} には名前で指定できる項目がありません")))?;
        let mut values = vec![None; def.fields.len()];
        // 式は書かれた順に実行し、値は型の項目順に格納する。
        for field in fields {
            let i = def.fields.iter().position(|n| n == &field.name)
                .ok_or_else(|| err(field.span, format!("{name} には {} という項目はありません", field.name)))?;
            if values[i].is_some() {
                return Err(err(field.span, format!("項目 {} が2回書かれています", field.name)));
            }
            values[i] = Some(self.eval_with_type(&field.value, def.types[i].as_ref(), env)?);
        }
        let fields = values.into_iter().zip(&def.fields).map(|(v, field)| {
            v.ok_or_else(|| err(span, format!("{name} を作るには {field} の値が必要です")))
        }).collect::<R<Vec<_>>>()?;
        Ok(Value::Struct(Rc::new(StructVal { def, fields })))
    }

    fn eval(&mut self, e: &Expr, env: &mut Env) -> R<Value> {
        Ok(match &e.kind {
            ExprKind::Int(n) => Value::Int(*n),
            ExprKind::Bool(b) => Value::Bool(*b),
            ExprKind::Str(parts) => {
                let mut s = String::new();
                for p in parts {
                    match p {
                        StrPart::Lit(l) => s.push_str(l),
                        StrPart::Expr(x) => s.push_str(&self.eval(x, env)?.show()),
                    }
                }
                Value::Str(s.into())
            }
            ExprKind::Array(xs) => {
                let mut v: Vec<Value> = Vec::with_capacity(xs.len());
                for x in xs {
                    let val = self.eval(x, env)?;
                    check_elem(&v, &val, x.span)?;
                    v.push(val);
                }
                Value::Array(Rc::new(v))
            }
            ExprKind::Record(_) => return Err(err(e.span, "{ } でどの型の値を作るか分かりません")),
            ExprKind::Var(n) => env
                .get(n)
                .cloned()
                .ok_or_else(|| err(e.span, format!("{n} にはまだ値が入っていません")).hint("この行より前で値を入れてください"))?,
            ExprKind::Binary(BinOp::And, a, b) => {
                let l = self.eval(a, env)?;
                if !truth(&l, a.span, "and の左右")? {
                    Value::Bool(false)
                } else {
                    let r = self.eval(b, env)?;
                    Value::Bool(truth(&r, b.span, "and の左右")?)
                }
            }
            ExprKind::Binary(BinOp::Or, a, b) => {
                let l = self.eval(a, env)?;
                if truth(&l, a.span, "or の左右")? {
                    Value::Bool(true)
                } else {
                    let r = self.eval(b, env)?;
                    Value::Bool(truth(&r, b.span, "or の左右")?)
                }
            }
            ExprKind::Binary(op, a, b) => {
                let l = self.eval(a, env)?;
                let r = self.eval(b, env)?;
                binop(*op, l, r, e.span)?
            }
            ExprKind::Unary(UnOp::Neg, a) => match self.eval(a, env)? {
                Value::Int(n) => Value::Int(n.checked_neg().ok_or_else(|| err(e.span, "数が大きすぎて計算できません"))?),
                other => return Err(err(e.span, format!("{}に - は付けられません", other.kind_name()))),
            },
            ExprKind::Unary(UnOp::Not, a) => {
                let v = self.eval(a, env)?;
                Value::Bool(!truth(&v, a.span, "not のあと")?)
            }
            ExprKind::Range(a, b) => {
                let l = self.eval(a, env)?;
                let r = self.eval(b, env)?;
                match (l, r) {
                    (Value::Int(l), Value::Int(r)) => {
                        if r >= l && r.saturating_sub(l) >= MAX_RANGE {
                            return Err(err(e.span, format!("範囲が大きすぎます（{MAX_RANGE} 個まで）")));
                        }
                        Value::Array(Rc::new((l..=r).map(Value::Int).collect()))
                    }
                    _ => return Err(err(e.span, "a..b の a と b には数を書きます")),
                }
            }
            ExprKind::Call { name, args } => {
                let mut vals = Vec::with_capacity(args.len());
                for a in args {
                    vals.push(self.eval(a, env)?);
                }
                self.call(name, vals, e.span)?
            }
            ExprKind::Method { recv, name, name_span, args } => {
                let mut vals = Vec::with_capacity(args.len() + 1);
                vals.push(self.eval(recv, env)?);
                for a in args {
                    vals.push(self.eval(a, env)?);
                }
                self.call(name, vals, *name_span)?
            }
            ExprKind::Field { recv, name, name_span } => {
                let v = self.eval(recv, env)?;
                match &v {
                    Value::Struct(s) => match s.def.fields.iter().position(|f| f == name) {
                        Some(i) => s.fields[i].clone(),
                        None => {
                            return Err(err(*name_span, format!("{} には {name} という項目はありません", s.def.name))
                                .hint(format!("項目: {}", s.def.fields.join(", "))));
                        }
                    },
                    other => {
                        let mut er = err(*name_span, format!("{}には {name} という項目はありません", other.kind_name()));
                        if self.funcs.contains_key(name.as_str()) || crate::check::BUILTINS.iter().any(|b| b.0 == name) {
                            er = er.hint(format!("{name} は関数です。x.{name}() のように ( ) を付けて呼び出します"));
                        }
                        return Err(er);
                    }
                }
            }
            ExprKind::Index { recv, index } => {
                let c = self.eval(recv, env)?;
                let i = self.eval(index, env)?;
                match c {
                    Value::Array(a) => a[index_of(&i, a.len(), index.span)?].clone(),
                    Value::Str(s) => {
                        let g = graphemes(&s);
                        Value::Str(g[index_of(&i, g.len(), index.span)?].into())
                    }
                    other => return Err(err(e.span, format!("{}は [ ] で取り出せません", other.kind_name()))),
                }
            }
        })
    }

    fn call(&mut self, name: &str, args: Vec<Value>, sp: Span) -> R<Value> {
        if let Some(def) = self.types.get(name).cloned() {
            for (i, (v, ty)) in args.iter().zip(&def.types).enumerate() {
                if let Some(ty) = ty
                    && !type_matches(v, ty) {
                        return Err(err(
                            sp,
                            format!("{}.{} には {} を入れます（渡されたのは{}）", def.name, def.fields[i], ty.show(), v.kind_name()),
                        ));
                    }
            }
            return Ok(Value::Struct(Rc::new(StructVal { def, fields: args })));
        }
        if let Some(f) = self.funcs.get(name).copied() {
            if self.depth >= MAX_DEPTH {
                return Err(err(sp, "関数の呼び出しが深くなりすぎました").hint("関数が自分自身を終わりなく呼び出していませんか？"));
            }
            let mut env = Env::new();
            for (p, v) in f.params.iter().zip(args) {
                if let Some(ty) = &p.ty
                    && !type_matches(&v, ty) {
                        return Err(err(sp, format!("{name} の {} には {} を渡します（渡されたのは{}）", p.name, ty.show(), v.kind_name())));
                    }
                env.insert(p.name.clone(), v);
            }
            self.depth += 1;
            let types = self.local_types[name].clone();
            let flow = self.block(&f.body, &mut env, &types);
            self.depth -= 1;
            let v = match flow? {
                Flow::Return(v) => v,
                _ => Value::None,
            };
            if self.returns.contains(name) && v == Value::None {
                return Err(err(f.span, format!("{name} が値を return しないまま最後まで来ました"))
                    .hint("どの道を通っても return で値を返すようにしてください"));
            }
            if let Some(ty) = &f.ret
                && !type_matches(&v, ty) {
                    return Err(err(sp, format!("{name} は {} を返すはずですが、{}を返しました", ty.show(), v.kind_name())));
                }
            return Ok(v);
        }
        self.builtin(name, args, sp)
    }

    fn builtin(&mut self, name: &str, args: Vec<Value>, sp: Span) -> R<Value> {
        match name {
            "print" => {
                let parts: Vec<String> = args.iter().map(|a| a.show()).collect();
                let _ = writeln!(self.out, "{}", parts.join(" "));
                Ok(Value::None)
            }
            "len" => match &args[0] {
                Value::Str(s) => Ok(Value::Int(graphemes(s).len() as i64)),
                Value::Array(a) => Ok(Value::Int(a.len() as i64)),
                other => Err(err(sp, format!("{}には len を使えません（文字か配列に使います）", other.kind_name()))),
            },
            "input" => {
                if let Some(p) = args.first() {
                    let _ = write!(self.out, "{}", p.show());
                }
                let _ = self.out.flush();
                let mut line = String::new();
                let _ = self.input.read_line(&mut line);
                Ok(Value::Str(line.trim_end_matches(['\n', '\r']).into()))
            }
            "random" => match (&args[0], &args[1]) {
                (Value::Int(a), Value::Int(b)) if a <= b => {
                    let width = (*b as i128 - *a as i128 + 1) as u128;
                    let r = (self.next_rand() as u128 % width) as i128;
                    Ok(Value::Int((*a as i128 + r) as i64))
                }
                (Value::Int(_), Value::Int(_)) => Err(err(sp, "random(a, b) は a を b 以下にします")),
                _ => Err(err(sp, "random には数を2つ渡します（random(1, 6) など）")),
            },
            "int" => match &args[0] {
                Value::Int(n) => Ok(Value::Int(*n)),
                Value::Str(s) if s.trim().is_empty() => Err(err(sp, "空の文字は数に直せません（何も入力されていないかもしれません）")),
                Value::Str(s) => s
                    .trim()
                    .parse::<i64>()
                    .map(Value::Int)
                    .map_err(|_| err(sp, format!("「{s}」は数に直せません"))),
                other => Err(err(sp, format!("{}は数に直せません", other.kind_name()))),
            },
            _ => Err(err(sp, format!("{name} という関数はありません"))),
        }
    }
}
