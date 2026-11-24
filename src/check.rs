// 実行前の検査。ここでエラーが見つかったら実行しない。
//
// v0 で検査すること:
// - 作られていない変数を読んでいる（タイプミスの候補も出す）
// - 値を入れたのに一度も使っていない変数（= に統一したことによるタイプミス対策）
// - ループ変数を変更している
// - 関数の結果を捨てている／値を返さない関数の結果を使っている
// - 関数や型の呼び出しで、渡す値の個数が合わない
// - 存在しない関数・型

use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::error::ZatuError;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SigKind {
    Builtin,
    Func,
    Type,
}

#[derive(Clone, Copy)]
pub struct Sig {
    pub min: usize,
    pub max: usize,
    pub returns: bool,
    pub kind: SigKind,
}

/// (名前, 最少の引数, 最多の引数, 値を返すか)
pub const BUILTINS: &[(&str, usize, usize, bool)] = &[
    ("print", 0, usize::MAX, false),
    ("len", 1, 1, true),
    ("input", 0, 1, true),
    ("random", 2, 2, true),
    ("int", 1, 1, true),
];

const BASIC_TYPES: &[&str] = &["int", "string", "bool"];

/// 関数本体に return 値 があるか
pub fn returns_value(body: &[Stmt]) -> bool {
    body.iter().any(|s| match &s.kind {
        StmtKind::Return(Some(_)) => true,
        StmtKind::If { then, els, .. } => returns_value(then) || els.as_deref().is_some_and(returns_value),
        StmtKind::ForIn { body, .. } | StmtKind::While { body, .. } => returns_value(body),
        _ => false,
    })
}

fn bare_returns(body: &[Stmt], out: &mut Vec<Span>) {
    for s in body {
        match &s.kind {
            StmtKind::Return(None) => out.push(s.span),
            StmtKind::If { then, els, .. } => {
                bare_returns(then, out);
                if let Some(e) = els {
                    bare_returns(e, out);
                }
            }
            StmtKind::ForIn { body, .. } | StmtKind::While { body, .. } => bare_returns(body, out),
            _ => {}
        }
    }
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        prev = cur;
    }
    prev[b.len()]
}

/// 似た名前を探す（タイプミスの候補）
fn closest<'a>(name: &str, cands: impl Iterator<Item = &'a String>) -> Option<String> {
    let n = name.chars().count();
    cands
        .filter(|c| c.as_str() != name)
        .map(|c| (levenshtein(name, c), c))
        .filter(|(d, _)| *d <= 2 && *d < n)
        .min_by_key(|(d, c)| (*d, (*c).clone()))
        .map(|(_, c)| c.clone())
}

pub fn check(prog: &Program) -> Vec<ZatuError> {
    let mut errs = Vec::new();
    let mut sigs: HashMap<String, Sig> = HashMap::new();
    for (name, min, max, returns) in BUILTINS {
        sigs.insert(name.to_string(), Sig { min: *min, max: *max, returns: *returns, kind: SigKind::Builtin });
    }
    let mut type_names: HashSet<String> = BASIC_TYPES.iter().map(|s| s.to_string()).collect();

    for t in &prog.types {
        if sigs.contains_key(&t.name) || type_names.contains(&t.name) {
            errs.push(ZatuError::new(t.span, format!("{} という名前はもう使われています", t.name)));
            continue;
        }
        let n = t.fields.len();
        sigs.insert(t.name.clone(), Sig { min: n, max: n, returns: true, kind: SigKind::Type });
        type_names.insert(t.name.clone());
        let mut seen = HashSet::new();
        for f in &t.fields {
            if !seen.insert(&f.name) {
                errs.push(ZatuError::new(f.span, format!("項目 {} が2回書かれています", f.name)));
            }
        }
    }
    for f in &prog.funcs {
        if sigs.contains_key(&f.name) || type_names.contains(&f.name) {
            errs.push(ZatuError::new(f.span, format!("{} という名前はもう使われています", f.name)));
            continue;
        }
        let returns = returns_value(&f.body);
        if returns {
            let mut bare = Vec::new();
            bare_returns(&f.body, &mut bare);
            for sp in bare {
                errs.push(
                    ZatuError::new(sp, "この関数は値を返す関数なので、return のあとに値が必要です")
                        .hint("値を返すときと返さないときを混ぜることはできません"),
                );
            }
        } else if f.ret.is_some() {
            errs.push(ZatuError::new(f.span, format!("{} は結果の型が書いてあるのに、return で値を返していません", f.name)));
        }
        let n = f.params.len();
        sigs.insert(f.name.clone(), Sig { min: n, max: n, returns, kind: SigKind::Func });
        let mut seen = HashSet::new();
        for p in &f.params {
            if !seen.insert(&p.name) {
                errs.push(ZatuError::new(p.span, format!("引数 {} が2回書かれています", p.name)));
            }
        }
    }

    // 型の名前が存在するか
    let check_ty = |ty: &Option<TypeExpr>, sp: Span, errs: &mut Vec<ZatuError>| {
        let mut t = match ty {
            Some(t) => t,
            None => return,
        };
        while let TypeExpr::Array(inner) = t {
            t = inner;
        }
        if let TypeExpr::Named(n) = t
            && !type_names.contains(n) {
                let mut e = ZatuError::new(sp, format!("{n} という型はありません"));
                if let Some(c) = closest(n, type_names.iter()) {
                    e = e.hint(format!("{c} のまちがいではありませんか？"));
                }
                errs.push(e);
            }
    };
    for t in &prog.types {
        for f in &t.fields {
            check_ty(&f.ty, f.span, &mut errs);
        }
    }
    for f in &prog.funcs {
        for p in &f.params {
            check_ty(&p.ty, p.span, &mut errs);
        }
        check_ty(&f.ret, f.span, &mut errs);
    }

    let empty = HashSet::new();
    let top = Scope::run(&sigs, &prog.body, &[], false, &empty, &mut errs);
    for f in &prog.funcs {
        Scope::run(&sigs, &f.body, &f.params, true, &top, &mut errs);
    }

    errs.sort_by_key(|e| (e.span.line, e.span.col));
    errs
}

/// 変数の有効範囲は関数単位。ループ変数だけはループの中だけで使える。
struct Scope<'a> {
    sigs: &'a HashMap<String, Sig>,
    in_func: bool,
    params: HashSet<String>,
    /// 名前 = 値 で作られる変数と、最初に作られた場所
    assigned: HashMap<String, Span>,
    read: HashSet<String>,
    loops: Vec<String>,
    loop_depth: usize,
    /// 関数の外（トップレベル）の変数。ヒント用
    outer: &'a HashSet<String>,
    errs: &'a mut Vec<ZatuError>,
}

fn collect(stmts: &[Stmt], loops: &mut Vec<String>, params: &HashSet<String>, out: &mut HashMap<String, Span>) {
    for s in stmts {
        match &s.kind {
            StmtKind::Assign { target, op: AssignOp::Set, .. }
                if target.path.is_empty() && !loops.contains(&target.name) && !params.contains(&target.name) =>
            {
                out.entry(target.name.clone()).or_insert(target.span);
            }
            StmtKind::If { then, els, .. } => {
                collect(then, loops, params, out);
                if let Some(e) = els {
                    collect(e, loops, params, out);
                }
            }
            StmtKind::ForIn { var, index, body, .. } => {
                let n = loops.len();
                loops.push(var.clone());
                if let Some((i, _)) = index {
                    loops.push(i.clone());
                }
                collect(body, loops, params, out);
                loops.truncate(n);
            }
            StmtKind::While { body, .. } => collect(body, loops, params, out),
            _ => {}
        }
    }
}

impl<'a> Scope<'a> {
    fn run(
        sigs: &'a HashMap<String, Sig>,
        body: &[Stmt],
        params: &[Param],
        in_func: bool,
        outer: &'a HashSet<String>,
        errs: &'a mut Vec<ZatuError>,
    ) -> HashSet<String> {
        let mut s = Scope {
            sigs,
            in_func,
            params: HashSet::new(),
            assigned: HashMap::new(),
            read: HashSet::new(),
            loops: Vec::new(),
            loop_depth: 0,
            outer,
            errs,
        };
        for p in params {
            if sigs.contains_key(&p.name) {
                s.errs.push(ZatuError::new(p.span, format!("{} は関数の名前なので、引数の名前には使えません", p.name)));
            }
            s.params.insert(p.name.clone());
        }
        collect(body, &mut Vec::new(), &s.params, &mut s.assigned);
        s.stmts(body);

        let mut unused: Vec<(&String, &Span)> = s.assigned.iter().filter(|(n, _)| !s.read.contains(*n)).collect();
        unused.sort_by_key(|(_, sp)| (sp.line, sp.col));
        let known: Vec<String> = s.assigned.keys().chain(s.params.iter()).cloned().collect();
        let mut new_errs = Vec::new();
        for (name, sp) in unused {
            let mut e = ZatuError::new(*sp, format!("{name} に値を入れていますが、一度も使っていません"));
            e = match closest(name, known.iter()) {
                Some(c) => e.hint(format!("{c} のまちがいではありませんか？")),
                None => e.hint("使わないなら、この行を消してください"),
            };
            new_errs.push(e);
        }
        s.errs.extend(new_errs);
        s.assigned.keys().cloned().collect()
    }

    fn err(&mut self, e: ZatuError) {
        self.errs.push(e);
    }

    fn is_var(&self, n: &str) -> bool {
        self.loops.iter().any(|l| l == n) || self.params.contains(n) || self.assigned.contains_key(n)
    }

    fn stmts(&mut self, ss: &[Stmt]) {
        for s in ss {
            self.stmt(s);
        }
    }

    fn stmt(&mut self, s: &Stmt) {
        match &s.kind {
            StmtKind::Assign { target, op, value } => {
                self.expr(value);
                for a in &target.path {
                    if let Access::Index(e) = a {
                        self.expr(e);
                    }
                }
                let n = &target.name;
                if self.loops.contains(n) {
                    let hint = if target.path.is_empty() {
                        "別の名前の変数を作って、そちらに入れてください"
                    } else {
                        "配列の中身を変えたいときは for x at i in xs と書いて、xs[i] を変更します"
                    };
                    self.err(ZatuError::new(target.span, format!("{n} はループの変数なので変更できません")).hint(hint));
                    return;
                }
                if self.sigs.contains_key(n) {
                    self.err(ZatuError::new(target.span, format!("{n} は関数の名前なので、変数の名前には使えません")));
                    return;
                }
                let creates = target.path.is_empty() && *op == AssignOp::Set;
                if !creates && !self.params.contains(n) && !self.assigned.contains_key(n) {
                    self.undefined(n, target.span);
                }
            }
            StmtKind::Expr(e) => self.expr_stmt(e),
            StmtKind::If { cond, then, els } => {
                self.expr(cond);
                self.stmts(then);
                if let Some(e) = els {
                    self.stmts(e);
                }
            }
            StmtKind::ForIn { var, var_span, index, iter, body } => {
                self.expr(iter);
                let mut names = vec![(var.clone(), *var_span)];
                if let Some((i, sp)) = index {
                    if i == var {
                        self.err(ZatuError::new(*sp, "要素と番号に同じ名前は使えません"));
                    }
                    names.push((i.clone(), *sp));
                }
                for (n, sp) in &names {
                    if self.is_var(n) || self.sigs.contains_key(n) {
                        self.err(
                            ZatuError::new(*sp, format!("{n} はもう別のところで使われている名前です"))
                                .hint("ループの変数には、まだ使っていない名前を付けてください"),
                        );
                    }
                }
                let before = self.loops.len();
                self.loops.extend(names.into_iter().map(|(n, _)| n));
                self.loop_depth += 1;
                self.stmts(body);
                self.loop_depth -= 1;
                self.loops.truncate(before);
            }
            StmtKind::While { cond, body } => {
                if let Some(c) = cond {
                    self.expr(c);
                }
                self.loop_depth += 1;
                self.stmts(body);
                self.loop_depth -= 1;
            }
            StmtKind::Return(v) => {
                if !self.in_func {
                    self.err(ZatuError::new(s.span, "return は関数の中でだけ使えます"));
                }
                if let Some(v) = v {
                    self.expr(v);
                }
            }
            StmtKind::Break | StmtKind::Continue => {
                if self.loop_depth == 0 {
                    self.err(ZatuError::new(s.span, "break と continue は for の中でだけ使えます"));
                }
            }
        }
    }

    fn expr_stmt(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Call { name, args } => self.call(name, None, args, e.span, false),
            ExprKind::Method { recv, name, name_span, args } => self.call(name, Some(recv), args, *name_span, false),
            _ => {
                self.expr(e);
                self.err(
                    ZatuError::new(e.span, "この行は計算しただけで、結果を何にも使っていません")
                        .hint("変数に入れる（x = ...）か、print(...) で表示してください"),
                );
            }
        }
    }

    fn call(&mut self, name: &str, recv: Option<&Expr>, args: &[Expr], span: Span, used: bool) {
        if let Some(r) = recv {
            self.expr(r);
        }
        for a in args {
            self.expr(a);
        }
        let n = args.len() + recv.is_some() as usize;
        let Some(sig) = self.sigs.get(name).copied() else {
            let mut e = ZatuError::new(span, format!("{name} という関数はありません"));
            if self.is_var(name) {
                e = e.hint(format!("{name} は変数です。( ) を付けて呼び出すことはできません"));
            } else if let Some(c) = closest(name, self.sigs.keys()) {
                e = e.hint(format!("{c} のまちがいではありませんか？"));
            } else if recv.is_some() {
                e = e.hint(format!("x.{name}() は {name}(x) と同じ意味です。func {name}(...) を作ると使えます"));
            }
            self.err(e);
            return;
        };
        if n < sig.min || n > sig.max {
            let msg = match sig.kind {
                SigKind::Type => format!("{name} を作るには {} 個の値が必要です（今は {n} 個）", sig.min),
                _ if sig.min == sig.max => format!("{name} には {} 個の値を渡します（今は {n} 個）", sig.min),
                _ => format!("{name} には {}〜{} 個の値を渡します（今は {n} 個）", sig.min, sig.max),
            };
            let mut e = ZatuError::new(span, msg);
            if recv.is_some() {
                e = e.hint(format!("x.{name}(...) の x も1個めの値として数えます"));
            }
            self.err(e);
        }
        if used && !sig.returns {
            self.err(ZatuError::new(span, format!("{name} は値を返さないので、結果を使うことはできません")));
        }
        if !used && sig.returns {
            let hint = match sig.kind {
                SigKind::Func => format!(
                    "結果を変数に入れるか、print(...) で表示してください。値を変えたいときは x = {name}(x) のように入れ直します"
                ),
                _ => "結果を変数に入れるか、print(...) で表示してください".to_string(),
            };
            self.err(ZatuError::new(span, format!("{name}(...) の結果を使っていません")).hint(hint));
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Int(_) | ExprKind::Bool(_) => {}
            ExprKind::Str(parts) => {
                for p in parts {
                    if let StrPart::Expr(x) = p {
                        self.expr(x);
                    }
                }
            }
            ExprKind::Array(xs) => {
                for x in xs {
                    self.expr(x);
                }
            }
            ExprKind::Var(n) => {
                if self.loops.contains(n) {
                } else if self.params.contains(n) || self.assigned.contains_key(n) {
                    self.read.insert(n.clone());
                } else {
                    self.undefined(n, e.span);
                }
            }
            ExprKind::Binary(_, a, b) | ExprKind::Range(a, b) => {
                self.expr(a);
                self.expr(b);
            }
            ExprKind::Unary(_, a) => self.expr(a),
            ExprKind::Call { name, args } => self.call(name, None, args, e.span, true),
            ExprKind::Method { recv, name, name_span, args } => self.call(name, Some(recv), args, *name_span, true),
            ExprKind::Field { recv, .. } => self.expr(recv),
            ExprKind::Index { recv, index } => {
                self.expr(recv);
                self.expr(index);
            }
        }
    }

    fn undefined(&mut self, n: &str, sp: Span) {
        let mut e = ZatuError::new(sp, format!("{n} という変数はまだ作られていません"));
        let known: Vec<String> =
            self.assigned.keys().chain(self.params.iter()).chain(self.loops.iter()).cloned().collect();
        e = if self.in_func && self.outer.contains(n) {
            e.hint("関数の中からは、関数の外の変数は見えません。引数で渡してください")
        } else if self.sigs.contains_key(n) {
            e.hint(format!("{n} は関数です。呼び出すときは {n}(...) と書きます"))
        } else if let Some(c) = closest(n, known.iter()) {
            e.hint(format!("{c} のまちがいではありませんか？"))
        } else {
            e.hint(format!("変数は {n} = 値 と書くと作れます"))
        };
        self.err(e);
    }
}
