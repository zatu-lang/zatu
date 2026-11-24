// zatu — 雑に書けば、動く。

pub mod ast;
pub mod check;
pub mod error;
pub mod eval;
pub mod lexer;
pub mod parser;
pub mod value;

use std::io::{BufRead, Write};

use error::ZatuError;

pub enum Failure {
    /// 実行前に見つかったエラー。この場合は1行も実行しない
    NotRun(Vec<ZatuError>),
    /// 実行中に起きたエラー
    Runtime(ZatuError),
}

/// 深い再帰でも Rust のスタックがあふれないよう、大きめのスタックを持つスレッドで実行する
const STACK_SIZE: usize = 512 * 1024 * 1024;

pub fn run(src: &str, out: &mut (dyn Write + Send), input: &mut (dyn BufRead + Send)) -> Result<(), Failure> {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(STACK_SIZE)
            .spawn_scoped(s, || run_here(src, out, input))
            .expect("スレッドを作れませんでした")
            .join()
            .expect("インタプリタが異常終了しました")
    })
}

fn run_here(src: &str, out: &mut dyn Write, input: &mut dyn BufRead) -> Result<(), Failure> {
    let prog = parser::parse(src).map_err(|e| Failure::NotRun(vec![e]))?;
    let errs = check::check(&prog);
    if !errs.is_empty() {
        return Err(Failure::NotRun(errs));
    }
    eval::Interp::new(&prog, out, input).run().map_err(Failure::Runtime)
}
