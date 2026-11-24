// 構文解析：トークン列を構文木にする（手書きの再帰下降）

use crate::ast::*;
use crate::error::ZatuError;
use crate::lexer::{Lexer, RawPart, Tok, Token, describe};

type PResult<T> = Result<T, ZatuError>;

pub fn parse(src: &str) -> PResult<Program> {
    let toks = Lexer::new(src).tokenize()?;
    Parser { toks, pos: 0 }.program()
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

fn bin(op: BinOp, l: Expr, r: Expr) -> Expr {
    let span = l.span.to(r.span);
    Expr { kind: ExprKind::Binary(op, Box::new(l), Box::new(r)), span }
}

fn cmp_op(t: &Tok) -> Option<BinOp> {
    Some(match t {
        Tok::Eq => BinOp::Eq,
        Tok::NotEq => BinOp::NotEq,
        Tok::Lt => BinOp::Lt,
        Tok::Le => BinOp::Le,
        Tok::Gt => BinOp::Gt,
        Tok::Ge => BinOp::Ge,
        _ => return None,
    })
}

/// 式を代入先に変換する。x / user.age / xs[1] だけを許す
fn to_target(e: Expr) -> PResult<Target> {
    let mut path = Vec::new();
    let mut cur = e;
    loop {
        let span = cur.span;
        match cur.kind {
            ExprKind::Var(name) => {
                path.reverse();
                return Ok(Target { name, span, path });
            }
            ExprKind::Field { recv, name, name_span } => {
                path.push(Access::Field(name, name_span));
                cur = *recv;
            }
            ExprKind::Index { recv, index } => {
                path.push(Access::Index(*index));
                cur = *recv;
            }
            _ => {
                return Err(ZatuError::new(span, "ここには値を入れられません")
                    .hint("= の左側には変数（x、user.age、xs[1] など）を書きます"));
            }
        }
    }
}

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }

    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }

    fn span(&self) -> Span {
        self.toks[self.pos].span
    }

    fn prev_span(&self) -> Span {
        self.toks[self.pos.saturating_sub(1)].span
    }

    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos + 1 < self.toks.len() {
            self.pos += 1;
        }
        t
    }

    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }

    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn unexpected(&self, what: &str) -> ZatuError {
        ZatuError::new(self.span(), format!("ここには {what} が必要です（{} があります）", describe(self.peek())))
    }

    fn expect(&mut self, t: Tok, what: &str) -> PResult<Token> {
        if self.at(&t) { Ok(self.advance()) } else { Err(self.unexpected(what)) }
    }

    fn ident(&mut self, what: &str) -> PResult<(String, Span)> {
        match self.peek().clone() {
            Tok::Ident(n) => {
                let sp = self.span();
                self.advance();
                Ok((n, sp))
            }
            _ => Err(self.unexpected(what)),
        }
    }

    fn skip_newlines(&mut self) {
        while self.at(&Tok::Newline) {
            self.advance();
        }
    }

    fn end_stmt(&mut self) -> PResult<()> {
        match self.peek() {
            Tok::Newline => {
                self.advance();
                Ok(())
            }
            Tok::RBrace | Tok::Eof => Ok(()),
            t => Err(ZatuError::new(self.span(), format!("ここで文が終わるはずですが、{} があります", describe(t)))
                .hint("1行に書ける文は1つだけです")),
        }
    }

    fn program(mut self) -> PResult<Program> {
        let mut prog = Program::default();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::Eof => return Ok(prog),
                Tok::Func => {
                    let f = self.func_decl()?;
                    prog.funcs.push(f);
                }
                Tok::Type => {
                    let t = self.type_decl()?;
                    prog.types.push(t);
                }
                _ => {
                    let s = self.stmt()?;
                    prog.body.push(s);
                }
            }
            self.end_stmt()?;
        }
    }

    fn func_decl(&mut self) -> PResult<FuncDecl> {
        self.advance(); // func
        let (name, span) = self.ident("関数の名前")?;
        self.expect(Tok::LParen, "(")?;
        let mut params = Vec::new();
        while !self.at(&Tok::RParen) {
            let (pname, psp) = self.ident("引数の名前")?;
            let ty = if matches!(self.peek(), Tok::Comma | Tok::RParen) { None } else { Some(self.type_expr()?) };
            params.push(Param { name: pname, span: psp, ty });
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen, ")")?;
        let ret = if self.at(&Tok::LBrace) { None } else { Some(self.type_expr()?) };
        let body = self.block()?;
        Ok(FuncDecl { name, span, params, ret, body })
    }

    fn type_expr(&mut self) -> PResult<TypeExpr> {
        if self.eat(&Tok::LBracket) {
            self.expect(Tok::RBracket, "]")?;
            return Ok(TypeExpr::Array(Box::new(self.type_expr()?)));
        }
        let (n, _) = self.ident("型の名前")?;
        Ok(TypeExpr::Named(n))
    }

    fn type_decl(&mut self) -> PResult<TypeDecl> {
        self.advance(); // type
        let (name, span) = self.ident("型の名前")?;
        self.expect(Tok::LBrace, "{")?;
        let mut fields = Vec::new();
        loop {
            self.skip_newlines();
            if self.eat(&Tok::RBrace) {
                break;
            }
            let (fname, fsp) = self.ident("項目の名前")?;
            let ty = if matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Comma) {
                None
            } else {
                Some(self.type_expr()?)
            };
            fields.push(Param { name: fname, span: fsp, ty });
            if !self.eat(&Tok::Comma) && !matches!(self.peek(), Tok::Newline | Tok::RBrace) {
                return Err(self.unexpected("改行"));
            }
        }
        Ok(TypeDecl { name, span, fields })
    }

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        let open = self.span();
        self.expect(Tok::LBrace, "{")?;
        let mut out = Vec::new();
        loop {
            self.skip_newlines();
            match self.peek() {
                Tok::RBrace => {
                    self.advance();
                    return Ok(out);
                }
                Tok::Eof => return Err(ZatuError::new(open, "この { に対応する } がありません")),
                _ => {
                    out.push(self.stmt()?);
                    self.end_stmt()?;
                }
            }
        }
    }

    /// 他の言語の書き方で書かれたときに案内する
    fn foreign_keyword(&self, w: &str) -> Option<&'static str> {
        match (w, self.peek_at(1)) {
            ("let" | "var" | "const", Tok::Ident(_)) => Some("zatu では変数を作るときに何も付けません。x = 1 のように書きます"),
            ("fn" | "def" | "function" | "fun", Tok::Ident(_)) => Some("関数は func で作ります。func add(a, b) { ... }"),
            ("while", Tok::Assign) => None,
            ("while", _) => Some("くり返しは for を使います。for x < 10 { ... }"),
            ("elif" | "elsif", _) => Some("else if と書きます"),
            _ => None,
        }
    }

    fn stmt(&mut self) -> PResult<Stmt> {
        let span = self.span();
        let kind = match self.peek().clone() {
            Tok::If => return self.if_stmt(),
            Tok::For => return self.for_stmt(),
            Tok::Return => {
                self.advance();
                let v = if matches!(self.peek(), Tok::Newline | Tok::RBrace | Tok::Eof) {
                    None
                } else {
                    Some(self.expr()?)
                };
                StmtKind::Return(v)
            }
            Tok::Break => {
                self.advance();
                StmtKind::Break
            }
            Tok::Continue => {
                self.advance();
                StmtKind::Continue
            }
            Tok::Func | Tok::Type => {
                return Err(ZatuError::new(span, "関数や型は、いちばん外側（ほかの { } の外）で作ってください"));
            }
            Tok::Else => return Err(ZatuError::new(span, "else の前には if が必要です")),
            Tok::Ident(w) if self.foreign_keyword(&w).is_some() => {
                let h = self.foreign_keyword(&w).unwrap_or_default();
                return Err(ZatuError::new(span, format!("{w} は zatu では使いません")).hint(h));
            }
            _ => {
                let e = self.expr()?;
                if self.eat(&Tok::Colon) {
                    let target = to_target(e)?;
                    if !target.path.is_empty() {
                        return Err(ZatuError::new(target.span, "型は変数の名前のあとに書きます")
                            .hint("例: users: []User = []"));
                    }
                    let type_span = self.span();
                    let ty = self.type_expr()?;
                    let type_span = type_span.to(self.prev_span());
                    self.expect(Tok::Assign, "=")?;
                    let value = self.expr()?;
                    return Ok(Stmt {
                        kind: StmtKind::Assign { target, annotation: Some((ty, type_span)), op: AssignOp::Set, value },
                        span,
                    });
                }
                let op = match self.peek() {
                    Tok::Assign => Some(AssignOp::Set),
                    Tok::PlusAssign => Some(AssignOp::Add),
                    Tok::MinusAssign => Some(AssignOp::Sub),
                    _ => None,
                };
                match op {
                    Some(op) => {
                        self.advance();
                        let value = self.expr()?;
                        let target = to_target(e)?;
                        StmtKind::Assign { target, annotation: None, op, value }
                    }
                    None => StmtKind::Expr(e),
                }
            }
        };
        Ok(Stmt { kind, span })
    }

    fn no_assign_in_cond(&self) -> PResult<()> {
        if self.at(&Tok::Assign) {
            return Err(ZatuError::new(self.span(), "条件の中で = は使えません").hint("くらべるときは == を使います"));
        }
        Ok(())
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let span = self.span();
        self.advance(); // if
        let cond = self.expr()?;
        self.no_assign_in_cond()?;
        let then = self.block()?;
        let save = self.pos;
        self.skip_newlines();
        let els = if self.eat(&Tok::Else) {
            if self.at(&Tok::If) { Some(vec![self.if_stmt()?]) } else { Some(self.block()?) }
        } else {
            self.pos = save;
            None
        };
        Ok(Stmt { kind: StmtKind::If { cond, then, els }, span })
    }

    fn for_stmt(&mut self) -> PResult<Stmt> {
        let span = self.span();
        self.advance(); // for
        if self.at(&Tok::LBrace) {
            let body = self.block()?;
            return Ok(Stmt { kind: StmtKind::While { cond: None, body }, span });
        }
        if let Tok::Ident(_) = self.peek() {
            if matches!(self.peek_at(1), Tok::In | Tok::At) {
                let (var, var_span) = self.ident("変数の名前")?;
                let index = if self.eat(&Tok::At) { Some(self.ident("番号の変数の名前")?) } else { None };
                self.expect(Tok::In, "in")?;
                let iter = self.expr()?;
                let body = self.block()?;
                return Ok(Stmt { kind: StmtKind::ForIn { var, var_span, index, iter, body }, span });
            }
            if matches!(self.peek_at(1), Tok::Comma) {
                return Err(ZatuError::new(self.span(), "for a, b in ... という書き方はありません")
                    .hint("番号もほしいときは for x at i in xs と書きます"));
            }
        }
        let cond = self.expr()?;
        self.no_assign_in_cond()?;
        let body = self.block()?;
        Ok(Stmt { kind: StmtKind::While { cond: Some(cond), body }, span })
    }

    // ---- 式 ----
    // 優先順位（低い順）: ..  or  and  not  比較  + -  * / %  -x  後置(. [] ())

    fn expr(&mut self) -> PResult<Expr> {
        let l = self.or_expr()?;
        if self.eat(&Tok::DotDot) {
            let r = self.or_expr()?;
            let span = l.span.to(r.span);
            return Ok(Expr { kind: ExprKind::Range(Box::new(l), Box::new(r)), span });
        }
        Ok(l)
    }

    fn or_expr(&mut self) -> PResult<Expr> {
        let mut l = self.and_expr()?;
        while self.eat(&Tok::Or) {
            let r = self.and_expr()?;
            l = bin(BinOp::Or, l, r);
        }
        Ok(l)
    }

    fn and_expr(&mut self) -> PResult<Expr> {
        let mut l = self.not_expr()?;
        while self.eat(&Tok::And) {
            let r = self.not_expr()?;
            l = bin(BinOp::And, l, r);
        }
        Ok(l)
    }

    fn not_expr(&mut self) -> PResult<Expr> {
        if self.at(&Tok::Not) {
            let sp = self.span();
            self.advance();
            let e = self.not_expr()?;
            let span = sp.to(e.span);
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Not, Box::new(e)), span });
        }
        self.cmp_expr()
    }

    fn cmp_expr(&mut self) -> PResult<Expr> {
        let l = self.add_expr()?;
        if let Some(op) = cmp_op(self.peek()) {
            self.advance();
            let r = self.add_expr()?;
            if cmp_op(self.peek()).is_some() {
                return Err(ZatuError::new(self.span(), "比較は1つずつ書きます").hint("1 < x and x < 10 のように and でつなぎます"));
            }
            return Ok(bin(op, l, r));
        }
        Ok(l)
    }

    fn add_expr(&mut self) -> PResult<Expr> {
        let mut l = self.mul_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Plus => BinOp::Add,
                Tok::Minus => BinOp::Sub,
                _ => return Ok(l),
            };
            self.advance();
            let r = self.mul_expr()?;
            l = bin(op, l, r);
        }
    }

    fn mul_expr(&mut self) -> PResult<Expr> {
        let mut l = self.unary_expr()?;
        loop {
            let op = match self.peek() {
                Tok::Star => BinOp::Mul,
                Tok::Slash => BinOp::Div,
                Tok::Percent => BinOp::Mod,
                _ => return Ok(l),
            };
            self.advance();
            let r = self.unary_expr()?;
            l = bin(op, l, r);
        }
    }

    fn unary_expr(&mut self) -> PResult<Expr> {
        if self.at(&Tok::Minus) {
            let sp = self.span();
            self.advance();
            let e = self.unary_expr()?;
            let span = sp.to(e.span);
            return Ok(Expr { kind: ExprKind::Unary(UnOp::Neg, Box::new(e)), span });
        }
        self.postfix_expr()
    }

    fn postfix_expr(&mut self) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            match self.peek() {
                Tok::Dot => {
                    self.advance();
                    let (name, name_span) = self.ident("項目か関数の名前")?;
                    if self.at(&Tok::LParen) {
                        let args = self.args()?;
                        let span = e.span.to(self.prev_span());
                        e = Expr { kind: ExprKind::Method { recv: Box::new(e), name, name_span, args }, span };
                    } else {
                        let span = e.span.to(name_span);
                        e = Expr { kind: ExprKind::Field { recv: Box::new(e), name, name_span }, span };
                    }
                }
                Tok::LBracket => {
                    self.advance();
                    let index = self.expr()?;
                    self.expect(Tok::RBracket, "]")?;
                    let span = e.span.to(self.prev_span());
                    e = Expr { kind: ExprKind::Index { recv: Box::new(e), index: Box::new(index) }, span };
                }
                _ => return Ok(e),
            }
        }
    }

    fn args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(Tok::LParen, "(")?;
        let mut args = Vec::new();
        while !self.at(&Tok::RParen) {
            args.push(self.expr()?);
            if !self.eat(&Tok::Comma) {
                break;
            }
        }
        self.expect(Tok::RParen, ")")?;
        Ok(args)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let sp = self.span();
        let kind = match self.peek().clone() {
            Tok::Int(n) => {
                self.advance();
                ExprKind::Int(n)
            }
            Tok::True => {
                self.advance();
                ExprKind::Bool(true)
            }
            Tok::False => {
                self.advance();
                ExprKind::Bool(false)
            }
            Tok::Str(raw) => {
                self.advance();
                let mut parts = Vec::new();
                for r in raw {
                    match r {
                        RawPart::Lit(s) => parts.push(StrPart::Lit(s)),
                        RawPart::Code(code, line, col) => parts.push(StrPart::Expr(parse_sub(&code, line, col)?)),
                    }
                }
                ExprKind::Str(parts)
            }
            Tok::Ident(name) => {
                self.advance();
                if self.at(&Tok::LParen) {
                    let args = self.args()?;
                    let span = sp.to(self.prev_span());
                    return Ok(Expr { kind: ExprKind::Call { name, args }, span });
                }
                ExprKind::Var(name)
            }
            Tok::LParen => {
                self.advance();
                let e = self.expr()?;
                self.expect(Tok::RParen, ")")?;
                return Ok(e);
            }
            Tok::LBracket => {
                self.advance();
                let mut xs = Vec::new();
                while !self.at(&Tok::RBracket) {
                    xs.push(self.expr()?);
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBracket, "]")?;
                let span = sp.to(self.prev_span());
                return Ok(Expr { kind: ExprKind::Array(xs), span });
            }
            Tok::LBrace => {
                self.advance();
                self.skip_newlines();
                let mut fields = Vec::new();
                while !self.at(&Tok::RBrace) {
                    let (name, span) = self.ident("項目の名前")?;
                    self.expect(Tok::Colon, ":")?;
                    self.skip_newlines();
                    let value = self.expr()?;
                    fields.push(NamedField { name, span, value });
                    self.skip_newlines();
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                    self.skip_newlines();
                }
                self.expect(Tok::RBrace, "}")?;
                return Ok(Expr { kind: ExprKind::Record(fields), span: sp.to(self.prev_span()) });
            }
            _ => return Err(self.unexpected("値や式")),
        };
        Ok(Expr { kind, span: sp })
    }
}

/// 文字列の { } の中身を式として解析する
fn parse_sub(code: &str, line: usize, col: usize) -> PResult<Expr> {
    let toks = Lexer::new_at(code, line, col).tokenize()?;
    let mut p = Parser { toks, pos: 0 };
    p.skip_newlines();
    let e = p.expr()?;
    p.skip_newlines();
    if !p.at(&Tok::Eof) {
        return Err(p.unexpected("}"));
    }
    Ok(e)
}
