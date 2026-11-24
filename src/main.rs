use std::io::{self, BufReader, Write};
use std::process::exit;

use zatu::Failure;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let [_, path] = args.as_slice() else {
        eprintln!("使い方: zatu <ファイル.zt>");
        exit(2);
    };
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{path} を読み込めませんでした: {e}");
            exit(2);
        }
    };

    let mut input = BufReader::new(io::stdin());
    let mut out = io::stdout();
    let result = zatu::run(&src, &mut out, &mut input);
    let _ = out.flush();
    match result {
        Ok(()) => {}
        Err(Failure::NotRun(errs)) => {
            for e in &errs {
                eprintln!("{}\n", e.render(&src, path));
            }
            eprintln!("直すところが {} 個あるので、実行しませんでした。", errs.len());
            exit(1);
        }
        Err(Failure::Runtime(e)) => {
            eprintln!("\n{}", e.render(&src, path));
            exit(1);
        }
    }
}
