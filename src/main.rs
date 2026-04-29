mod ast;
mod codegen;
mod lexer;
mod parser;
mod nia_std;
mod typecheck;

use std::path::PathBuf;

use parser::{tokenize, Parser};
use typecheck::{check_fn, collect_sigs};

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let in_path = args
        .next()
        .ok_or_else(|| "usage: nialang <file.nia> [-o out.ll]".to_string())?;
    let mut out_path: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        if a == "-o" {
            out_path = Some(
                args.next()
                    .ok_or_else(|| "-o requires a path".to_string())?
                    .into(),
            );
        } else {
            return Err(format!("unknown flag `{a}`"));
        }
    }
    let src = std::fs::read_to_string(&in_path).map_err(|e| e.to_string())?;
    let tokens = tokenize(&src);
    let (structs, fns) = Parser::new(tokens).parse_file()?;
    let (struct_map, fn_sigs) = collect_sigs(&structs, &fns)?;
    for f in &fns {
        check_fn(f, &struct_map, &fn_sigs)?;
    }
    let ll = codegen::emit_module(&structs, &fns, &fn_sigs);
    let out = out_path.unwrap_or_else(|| PathBuf::from(in_path).with_extension("ll"));
    std::fs::write(&out, ll).map_err(|e| e.to_string())?;
    eprintln!("wrote {}", out.display());
    Ok(())
}
