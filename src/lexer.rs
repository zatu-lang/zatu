// 字句解析：ソースコードをトークンの列にする

use crate::ast::Span;
use crate::error::ZatuError;

/// 文字列リテラルの部品。"こんにちは{name}" → [Lit("こんにちは"), Code("name")]
#[derive(Debug, Clone, PartialEq)]
pub enum RawPart {
    Lit(String),
    /// { } の中身と、その開始位置（行, 列）
    Code(String, usize, usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    Int(i64),
    Str(Vec<RawPart>),
    Ident(String),
    Func,
    Type,
    If,
    Else,
    For,
    In,
    At,
    Return,
    Break,
    Continue,
    True,
    False,
    And,
    Or,
    Not,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Dot,
    DotDot,
    Assign,
    PlusAssign,
    MinusAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
    Newline,
    Eof,
}

/// エラーメッセージ用にトークンを説明する
pub fn describe(t: &Tok) -> String {
    let s = match t {
        Tok::Int(_) => return "数".into(),
        Tok::Str(_) => return "文字列".into(),
        Tok::Ident(n) => return format!("「{n}」"),
        Tok::Newline => return "行の終わり".into(),
        Tok::Eof => return "ファイルの終わり".into(),
        Tok::Func => "func",
        Tok::Type => "type",
        Tok::If => "if",
        Tok::Else => "else",
        Tok::For => "for",
        Tok::In => "in",
        Tok::At => "at",
        Tok::Return => "return",
        Tok::Break => "break",
        Tok::Continue => "continue",
        Tok::True => "true",
        Tok::False => "false",
        Tok::And => "and",
        Tok::Or => "or",
        Tok::Not => "not",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBrace => "{",
        Tok::RBrace => "}",
        Tok::LBracket => "[",
        Tok::RBracket => "]",
        Tok::Comma => ",",
        Tok::Dot => ".",
        Tok::DotDot => "..",
        Tok::Assign => "=",
        Tok::PlusAssign => "+=",
        Tok::MinusAssign => "-=",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Star => "*",
        Tok::Slash => "/",
        Tok::Percent => "%",
        Tok::Eq => "==",
        Tok::NotEq => "!=",
        Tok::Lt => "<",
        Tok::Le => "<=",
        Tok::Gt => ">",
        Tok::Ge => ">=",
    };
    format!("「{s}」")
}

#[derive(Debug, Clone)]
pub struct Token {
    pub tok: Tok,
    pub span: Span,
}

pub struct Lexer {
    chars: Vec<char>,
    pos: usize,
    line: usize,
    col: usize,
    /// ( と [ の深さ。中では改行を無視する
    depth: i32,
}

impl Lexer {
    pub fn new(src: &str) -> Self {
        Self::new_at(src, 1, 1)
    }

    pub fn new_at(src: &str, line: usize, col: usize) -> Self {
        Lexer {
            chars: src.chars().collect(),
            pos: 0,
            line,
            col,
            depth: 0,
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn peek2(&self) -> Option<char> {
        self.chars.get(self.pos + 1).copied()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += 1;
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn skip_space(&mut self) {
        while let Some(c) = self.peek() {
            // 全角スペースも空白として扱う（IMEで入りがちなため）
            if c == ' ' || c == '\t' || c == '\r' || c == '\u{3000}' {
                self.bump();
            } else if c == '/' && self.peek2() == Some('/') {
                while let Some(c) = self.peek() {
                    if c == '\n' {
                        break;
                    }
                    self.bump();
                }
            } else {
                break;
            }
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, ZatuError> {
        let mut toks = Vec::new();
        loop {
            self.skip_space();
            let (line, col, start) = (self.line, self.col, self.pos);
            let here = Span::new(line, col, 1);
            let Some(c) = self.peek() else {
                toks.push(Token { tok: Tok::Eof, span: here });
                return Ok(toks);
            };
            let tok = match c {
                '\n' => {
                    self.bump();
                    if self.depth > 0 {
                        continue;
                    }
                    Tok::Newline
                }
                '0'..='9' => self.number(here)?,
                '"' => self.string(here)?,
                c if c.is_alphabetic() || c == '_' => self.ident(),
                _ => self.symbol(c, here)?,
            };
            let len = if tok == Tok::Newline { 1 } else { (self.pos - start).max(1) };
            toks.push(Token { tok, span: Span::new(line, col, len) });
        }
    }

    fn number(&mut self, here: Span) -> Result<Tok, ZatuError> {
        let mut s = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_ascii_digit()) {
            s.push(c);
            self.bump();
        }
        if self.peek() == Some('.') && self.peek2().is_some_and(|c| c.is_ascii_digit()) {
            return Err(ZatuError::new(here, "小数（3.5 など）はまだ使えません（v0）"));
        }
        s.parse::<i64>()
            .map(Tok::Int)
            .map_err(|_| ZatuError::new(Span::new(here.line, here.col, s.chars().count()), "数が大きすぎます"))
    }

    fn ident(&mut self) -> Tok {
        let mut s = String::new();
        while let Some(c) = self.peek().filter(|c| c.is_alphanumeric() || *c == '_') {
            s.push(c);
            self.bump();
        }
        match s.as_str() {
            "func" => Tok::Func,
            "type" => Tok::Type,
            "if" => Tok::If,
            "else" => Tok::Else,
            "for" => Tok::For,
            "in" => Tok::In,
            "at" => Tok::At,
            "return" => Tok::Return,
            "break" => Tok::Break,
            "continue" => Tok::Continue,
            "true" => Tok::True,
            "false" => Tok::False,
            "and" => Tok::And,
            "or" => Tok::Or,
            "not" => Tok::Not,
            _ => Tok::Ident(s),
        }
    }

    fn string(&mut self, here: Span) -> Result<Tok, ZatuError> {
        self.bump(); // "
        let mut parts = Vec::new();
        let mut buf = String::new();
        loop {
            match self.peek() {
                None | Some('\n') => {
                    return Err(ZatuError::new(here, "文字列が閉じていません").hint("文字列の最後に \" を付けてください"));
                }
                Some('"') => {
                    self.bump();
                    break;
                }
                Some('\\') => {
                    let esc = Span::new(self.line, self.col, 2);
                    self.bump();
                    let c = match self.bump() {
                        Some('n') => '\n',
                        Some('t') => '\t',
                        Some('"') => '"',
                        Some('\\') => '\\',
                        Some('{') => '{',
                        Some('}') => '}',
                        other => {
                            let shown = other.map(String::from).unwrap_or_default();
                            return Err(ZatuError::new(esc, format!("\\{shown} という書き方はありません"))
                                .hint("使えるのは \\n \\t \\\" \\\\ \\{ \\} です"));
                        }
                    };
                    buf.push(c);
                }
                Some('{') => {
                    let open = Span::new(self.line, self.col, 1);
                    self.bump();
                    let (cl, cc) = (self.line, self.col);
                    let mut code = String::new();
                    let mut depth = 0;
                    loop {
                        match self.peek() {
                            None | Some('\n') => {
                                return Err(ZatuError::new(open, "{ に対応する } がありません")
                                    .hint("文字の中に { そのものを書きたいときは \\{ と書きます"));
                            }
                            Some('"') => {
                                return Err(ZatuError::new(
                                    Span::new(self.line, self.col, 1),
                                    "{ } の中では \" は使えません（v0）",
                                ));
                            }
                            Some('{') => {
                                depth += 1;
                                code.push('{');
                                self.bump();
                            }
                            Some('}') => {
                                self.bump();
                                if depth == 0 {
                                    break;
                                }
                                depth -= 1;
                                code.push('}');
                            }
                            Some(c) => {
                                code.push(c);
                                self.bump();
                            }
                        }
                    }
                    if code.trim().is_empty() {
                        return Err(ZatuError::new(open, "{ } の中が空です").hint(
                            "{name} のように中に変数や式を書きます。{ そのものを書きたいときは \\{ と書きます",
                        ));
                    }
                    if !buf.is_empty() {
                        parts.push(RawPart::Lit(std::mem::take(&mut buf)));
                    }
                    parts.push(RawPart::Code(code, cl, cc));
                }
                Some(c) => {
                    buf.push(c);
                    self.bump();
                }
            }
        }
        if !buf.is_empty() || parts.is_empty() {
            parts.push(RawPart::Lit(buf));
        }
        Ok(Tok::Str(parts))
    }

    fn symbol(&mut self, c: char, here: Span) -> Result<Tok, ZatuError> {
        self.bump();
        let next = self.peek();
        let two = Span::new(here.line, here.col, 2);
        let tok = match (c, next) {
            ('(', _) => {
                self.depth += 1;
                Tok::LParen
            }
            (')', _) => {
                self.depth = (self.depth - 1).max(0);
                Tok::RParen
            }
            ('[', _) => {
                self.depth += 1;
                Tok::LBracket
            }
            (']', _) => {
                self.depth = (self.depth - 1).max(0);
                Tok::RBracket
            }
            ('{', _) => Tok::LBrace,
            ('}', _) => Tok::RBrace,
            (',', _) => Tok::Comma,
            ('.', Some('.')) => {
                self.bump();
                Tok::DotDot
            }
            ('.', _) => Tok::Dot,
            ('=', Some('=')) => {
                self.bump();
                Tok::Eq
            }
            ('=', _) => Tok::Assign,
            ('!', Some('=')) => {
                self.bump();
                Tok::NotEq
            }
            ('<', Some('=')) => {
                self.bump();
                Tok::Le
            }
            ('<', _) => Tok::Lt,
            ('>', Some('=')) => {
                self.bump();
                Tok::Ge
            }
            ('>', _) => Tok::Gt,
            ('+', Some('=')) => {
                self.bump();
                Tok::PlusAssign
            }
            ('+', _) => Tok::Plus,
            ('-', Some('=')) => {
                self.bump();
                Tok::MinusAssign
            }
            ('-', _) => Tok::Minus,
            ('*', _) => Tok::Star,
            ('/', _) => Tok::Slash,
            ('%', _) => Tok::Percent,
            (':', Some('=')) => {
                return Err(ZatuError::new(two, ":= は使いません").hint("zatu では = だけで変数を作れます（x = 1）"));
            }
            ('!', _) => {
                return Err(ZatuError::new(here, "! は使いません").hint("「〜ではない」は not を使います（not done）"));
            }
            ('&', Some('&')) => {
                return Err(ZatuError::new(two, "&& は使いません").hint("「かつ」は and を使います（a and b）"));
            }
            ('|', Some('|')) => {
                return Err(ZatuError::new(two, "|| は使いません").hint("「または」は or を使います（a or b）"));
            }
            ('\u{FF01}'..='\u{FF5E}', _) => {
                let half = char::from_u32(c as u32 - 0xFEE0).unwrap_or(c);
                return Err(ZatuError::new(here, format!("全角の「{c}」が入っています"))
                    .hint(format!("半角の「{half}」にしてください")));
            }
            ('“' | '”' | '「' | '」', _) => {
                return Err(ZatuError::new(here, format!("「{c}」は使えません")).hint("文字列は半角の \" で囲みます"));
            }
            _ => return Err(ZatuError::new(here, format!("「{c}」は zatu では使えない記号です"))),
        };
        Ok(tok)
    }
}
