mod ast;
mod codegen;
mod lexer;
mod nia_std;
mod parser;
mod typecheck;

use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use parser::{Parser, tokenize};
use typecheck::{check_fn, collect_sigs};

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

fn compile_to_ll(src: &str) -> Result<String, String> {
    let tokens = tokenize(src);
    let (structs, fns) = Parser::new(tokens).parse_file()?;
    let (struct_map, fn_sigs) = collect_sigs(&structs, &fns)?;
    for f in &fns {
        check_fn(f, &struct_map, &fn_sigs)?;
    }
    Ok(codegen::emit_module(&structs, &fns, &fn_sigs))
}

fn run() -> Result<i32, String> {
    let mut args = std::env::args().skip(1);
    let in_path: PathBuf = args
        .next()
        .ok_or_else(|| {
            "usage: nialang <file.nia> [-o out.ll]\n\
             Compiles the file with clang, runs the executable, then exits with its status.\n\
             Use -o to also save LLVM IR to a file."
                .to_string()
        })?
        .into();
    let mut out_ll: Option<PathBuf> = None;
    while let Some(a) = args.next() {
        if a == "-o" {
            out_ll = Some(
                args.next()
                    .ok_or_else(|| "-o requires a path".to_string())?
                    .into(),
            );
        } else {
            return Err(format!("unknown flag `{a}`"));
        }
    }

    let src = std::fs::read_to_string(&in_path).map_err(|e| e.to_string())?;
    let ll = compile_to_ll(&src)?;

    if let Some(ref p) = out_ll {
        std::fs::write(p, &ll).map_err(|e| e.to_string())?;
        eprintln!("wrote {}", p.display());
    }

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();
    let tmp_ll = tmp_dir.join(format!("nialang-{pid}-{nonce}.ll"));
    let tmp_exe = tmp_exe_path(&tmp_dir, pid, nonce);

    std::fs::write(&tmp_ll, &ll).map_err(|e| e.to_string())?;

    let clang_ok = Command::new("clang")
        .arg(&tmp_ll)
        .arg("-o")
        .arg(&tmp_exe)
        .status()
        .map_err(|e| {
            format!(
                "failed to run `clang`: {e}\n\
                 Install LLVM/clang and ensure `clang` is on PATH."
            )
        })?;
    if !clang_ok.success() {
        let _ = std::fs::remove_file(&tmp_ll);
        let _ = std::fs::remove_file(&tmp_exe);
        return Err("clang failed to compile the generated LLVM IR".into());
    }

    let run_status = Command::new(&tmp_exe)
        .status()
        .map_err(|e| format!("failed to run compiled program: {e}"))?;

    let _ = std::fs::remove_file(&tmp_ll);
    let _ = std::fs::remove_file(&tmp_exe);

    let code = run_status
        .code()
        .unwrap_or(if run_status.success() { 0 } else { 101 });
    Ok(code)
}

fn tmp_exe_path(tmp_dir: &std::path::Path, pid: u32, nonce: u128) -> PathBuf {
    if cfg!(windows) {
        tmp_dir.join(format!("nialang-{pid}-{nonce}-run.exe"))
    } else {
        tmp_dir.join(format!("nialang-{pid}-{nonce}-run"))
    }
}
