use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::backend::codegen;
use crate::frontend::parser::{tokenize, Parser};
use crate::semantics::typecheck::{check_fn, collect_sigs};

/// Compiles nialang source text into one textual LLVM IR module.
///
/// ## What this function guarantees
/// - Returns IR only after the source passes *all* frontend and semantic checks.
/// - Fails fast on the first error and returns a human-readable message.
/// - Produces backend input that is already type-consistent.
///
/// ## Internal stages (in strict order)
/// 1. **Lexing**: converts text into tokens.
/// 2. **Parsing**: builds AST (`structs`, `fns`) from token stream.
/// 3. **Signature collection**: builds global symbol tables and validates duplicates.
/// 4. **Type checking**: validates each function body against collected signatures.
/// 5. **Code generation**: lowers validated AST to LLVM IR text.
///
/// Each stage depends on previous output, so failures are intentionally not aggregated.
pub fn compile_to_ll(src: &str) -> Result<String, String> {
    let tokens = tokenize(src);
    let (structs, fns) = Parser::new(tokens).parse_file()?;
    let (struct_map, fn_sigs) = collect_sigs(&structs, &fns)?;
    for f in &fns {
        check_fn(f, &struct_map, &fn_sigs)?;
    }
    Ok(codegen::emit_module(&structs, &fns, &fn_sigs))
}

/// High-level CLI execution pipeline used by `main`.
///
/// ## Supported CLI shape
/// `nialang <file.nia> [-o out.ll]`
///
/// ## Behavior
/// - Resolves input path robustly for `cargo run` and direct binary usage.
/// - Compiles `.nia` source into LLVM IR.
/// - Optionally dumps generated IR to disk when `-o` is provided.
/// - Invokes `clang` to produce a temporary native executable.
/// - Runs that executable and returns *its* exit status to caller.
/// - Removes temporary artifacts (`.ll` and executable) best-effort.
///
/// ## Errors
/// Returns descriptive errors for bad CLI flags, read/write failures, missing clang,
/// clang compile failures, and runtime launch failures.
pub fn run_cli() -> Result<i32, String> {
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
    let in_path = resolve_input_path(in_path)?;
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

    // Read the input program after path resolution, so diagnostics include final path.
    let src = std::fs::read_to_string(&in_path)
        .map_err(|e| format!("{}: {e}", in_path.display()))?;
    let ll = compile_to_ll(&src)?;

    if let Some(ref p) = out_ll {
        std::fs::write(p, &ll).map_err(|e| e.to_string())?;
        eprintln!("wrote {}", p.display());
    }

    // Use pid + timestamp nonce to avoid tmp filename collisions between runs.
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();
    let tmp_ll = tmp_dir.join(format!("nialang-{pid}-{nonce}.ll"));
    let tmp_exe = tmp_exe_path(&tmp_dir, pid, nonce);

    std::fs::write(&tmp_ll, &ll).map_err(|e| e.to_string())?;

    // Compile generated IR into a native executable via system clang.
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

    // Execute produced binary and propagate its status code back to the caller.
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

/// Resolves user-supplied input file path to an existing file.
///
/// Resolution strategy for **relative** paths:
/// 1. Current working directory.
/// 2. Runtime `CARGO_MANIFEST_DIR` (common when launched via `cargo run`).
/// 3. Compile-time `env!("CARGO_MANIFEST_DIR")` fallback.
///
/// Absolute paths are returned unchanged.
///
/// This keeps `examples/foo.nia` usable even when the process cwd differs from
/// repository root.
pub fn resolve_input_path(p: PathBuf) -> Result<PathBuf, String> {
    if p.is_absolute() {
        return Ok(p);
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let from_cwd = cwd.join(&p);
    if from_cwd.is_file() {
        return Ok(from_cwd);
    }
    if let Ok(root) = std::env::var("CARGO_MANIFEST_DIR") {
        let q = PathBuf::from(root).join(&p);
        if q.is_file() {
            return Ok(q);
        }
    }
    let from_build = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(&p);
    if from_build.is_file() {
        return Ok(from_build);
    }
    Err(format!(
        "could not find `{}` (tried {} and {})",
        p.display(),
        from_cwd.display(),
        from_build.display()
    ))
}

/// Builds temporary executable file path for current platform.
///
/// The name includes process id and nonce to reduce collisions between concurrent runs.
/// Uses `.exe` suffix on Windows and no suffix on Unix-like systems.
fn tmp_exe_path(tmp_dir: &std::path::Path, pid: u32, nonce: u128) -> PathBuf {
    if cfg!(windows) {
        tmp_dir.join(format!("nialang-{pid}-{nonce}-run.exe"))
    } else {
        tmp_dir.join(format!("nialang-{pid}-{nonce}-run"))
    }
}
