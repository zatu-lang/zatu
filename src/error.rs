// エラーの表現と表示

use crate::ast::Span;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Clone)]
pub struct ZatuError {
    pub span: Span,
    pub msg: String,
    pub hint: Option<String>,
}

impl ZatuError {
    pub fn new(span: Span, msg: impl Into<String>) -> Self {
        ZatuError {
            span,
            msg: msg.into(),
            hint: None,
        }
    }

    pub fn hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// ソースの該当行と ^^^ 付きで表示用の文字列を作る
    pub fn render(&self, src: &str, file: &str) -> String {
        let mut s = format!("エラー: {}:{}:{}\n", file, self.span.line, self.span.col);
        if let Some(line) = src.lines().nth(self.span.line.saturating_sub(1)) {
            let chars: Vec<char> = line.chars().map(|c| if c == '\t' { ' ' } else { c }).collect();
            let start = self.span.col.saturating_sub(1).min(chars.len());
            let end = (start + self.span.len.max(1)).min(chars.len()).max(start);
            let width = |cs: &[char]| cs.iter().map(|c| c.width().unwrap_or(0)).sum::<usize>();
            let pad = width(&chars[..start]);
            let caret = width(&chars[start..end]).max(1);
            let num = self.span.line.to_string();
            let shown: String = chars.iter().collect();
            s += &format!(" {} | {}\n", num, shown);
            s += &format!(" {} | {}{}\n", " ".repeat(num.len()), " ".repeat(pad), "^".repeat(caret));
        }
        s += &self.msg;
        if let Some(h) = &self.hint {
            s += "\nヒント: ";
            s += h;
        }
        s
    }
}
