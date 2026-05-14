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
    if let Some((line, column, ch)) = find_unsupported_char(src) {
        return Err(format_diagnostic_at_line(
            src,
            &format!("lex error: unsupported character `{ch}`"),
            line,
            column,
        ));
    }

    let tokens = tokenize(src);
    let (structs, enums, fns, vectors) = Parser::new(tokens)
        .parse_file()
        .map_err(|e| format_diagnostic(src, "parse error", None, &e))?;
    let (struct_map, enum_map, vector_map, fn_sigs) =
        collect_sigs(&structs, &enums, &vectors, &fns)
            .map_err(|e| format_diagnostic(src, "semantic error", None, &e))?;
    for f in &fns {
        check_fn(f, &struct_map, &enum_map, &vector_map, &fn_sigs)
            .map_err(|e| format_diagnostic(src, "type error", Some(&f.name), &e))?;
    }
    Ok(codegen::emit_module(
        &structs, &enums, &vectors, &fns, &fn_sigs,
    ))
}

fn format_diagnostic(src: &str, kind: &str, function: Option<&str>, message: &str) -> String {
    let header = match function {
        Some(name) => format!("{kind} in function `{name}`: {message}"),
        None => format!("{kind}: {message}"),
    };
    let Some(line) = find_problem_line(src, message, function) else {
        return header;
    };

    let number_width = line.number.to_string().len();
    let caret_padding = " ".repeat(line.column.saturating_sub(1));
    format!(
        "{header}\n --> line {line_no}\n{bar:>number_width$} |\n{line_no:>number_width$} | {text}\n{bar:>number_width$} | {caret_padding}^",
        line_no = line.number,
        bar = "",
        text = line.text,
    )
}

fn format_diagnostic_at_line(src: &str, header: &str, line_no: usize, column: usize) -> String {
    let Some(text) = src.lines().nth(line_no.saturating_sub(1)) else {
        return header.to_string();
    };
    let number_width = line_no.to_string().len();
    let caret_padding = " ".repeat(column.saturating_sub(1));
    format!(
        "{header}\n --> line {line_no}\n{bar:>number_width$} |\n{line_no:>number_width$} | {text}\n{bar:>number_width$} | {caret_padding}^",
        bar = "",
    )
}

fn find_unsupported_char(src: &str) -> Option<(usize, usize, char)> {
    for (line_idx, line) in src.lines().enumerate() {
        let line_no = line_idx + 1;
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0usize;
        let mut in_string = false;
        let mut escape = false;
        while i < chars.len() {
            let ch = chars[i];
            let column = i + 1;

            if !in_string && ch == '/' && chars.get(i + 1) == Some(&'/') {
                break;
            }

            if in_string {
                if escape {
                    escape = false;
                    i += 1;
                    continue;
                }
                if ch == '\\' {
                    escape = true;
                    i += 1;
                    continue;
                }
                if ch == '"' {
                    in_string = false;
                    i += 1;
                    continue;
                }
                i += 1;
                continue;
            }

            if ch == '"' {
                in_string = true;
                i += 1;
                continue;
            }

            if ch.is_whitespace() || is_supported_source_char(ch) {
                i += 1;
                continue;
            }
            return Some((line_no, column, ch));
        }
    }
    None
}

fn is_supported_source_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric()
        || matches!(
            ch,
            '_' | ':'
                | ','
                | ';'
                | '('
                | ')'
                | '{'
                | '}'
                | '['
                | ']'
                | '+'
                | '-'
                | '*'
                | '@'
                | '/'
                | '&'
                | '.'
                | '='
                | '!'
                | '<'
                | '>'
                | '"'
                | '\\'
        )
}

#[derive(Debug, Clone)]
struct ProblemLine {
    number: usize,
    column: usize,
    text: String,
}

fn find_problem_line(src: &str, message: &str, function: Option<&str>) -> Option<ProblemLine> {
    let terms = diagnostic_terms(message);
    let function_bounds = function.and_then(|name| find_function_bounds(src, name));
    let mut best: Option<(usize, ProblemLine)> = None;

    for (idx, text) in src.lines().enumerate() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        let line_no = idx + 1;
        let in_function = function_bounds
            .map(|(start, end)| (start..=end).contains(&line_no))
            .unwrap_or(false);
        let mut score = usize::from(in_function) * 3;

        for term in &terms {
            if line_contains_term(text, term) {
                score += term_score(term);
            }
        }
        score += structural_score(message, trimmed);

        if score == 0 {
            continue;
        }

        let column = terms
            .iter()
            .filter_map(|term| find_column(text, term))
            .next()
            .or_else(|| first_non_ws_column(text))
            .unwrap_or(1);
        let line = ProblemLine {
            number: line_no,
            column,
            text: text.to_string(),
        };

        if best
            .as_ref()
            .map(|(best_score, _)| score > *best_score)
            .unwrap_or(true)
        {
            best = Some((score, line));
        }
    }

    best.map(|(_, line)| line).or_else(|| {
        src.lines()
            .enumerate()
            .find(|(_, line)| !line.trim().is_empty())
            .map(|(idx, text)| ProblemLine {
                number: idx + 1,
                column: first_non_ws_column(text).unwrap_or(1),
                text: text.to_string(),
            })
    })
}

fn diagnostic_terms(message: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut rest = message;
    while let Some(start) = rest.find('`') {
        rest = &rest[start + 1..];
        let Some(end) = rest.find('`') else {
            break;
        };
        let term = &rest[..end];
        if !term.is_empty() && !terms.iter().any(|existing| existing == term) {
            terms.push(term.to_string());
        }
        rest = &rest[end + 1..];
    }

    if message.contains("division by zero") {
        terms.push("/".to_string());
        terms.push("0".to_string());
    }
    if message.contains("array index") || message.contains("indexing requires array") {
        terms.push("[".to_string());
    }
    if message.contains("field access") || message.contains("has no field") {
        terms.push(".".to_string());
    }
    if let Some(token) = message.strip_prefix("expected ").and_then(|_| {
        message
            .split_once("got ")
            .map(|(_, token)| token.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
    }) {
        if let Some(lexeme) = token_lexeme(token) {
            terms.push(lexeme.to_string());
        }
    }

    terms
}

fn token_lexeme(token: &str) -> Option<&'static str> {
    match token {
        "Fn" => Some("fn"),
        "Let" => Some("let"),
        "Struct" => Some("struct"),
        "Vector" => Some("vector"),
        "Enum" => Some("enum"),
        "If" => Some("if"),
        "While" => Some("while"),
        "Loop" => Some("loop"),
        "Break" => Some("break"),
        "For" => Some("for"),
        "In" => Some("in"),
        "Match" => Some("match"),
        "Return" => Some("return"),
        "Colon" => Some(":"),
        "Comma" => Some(","),
        "Semi" => Some(";"),
        "LParen" => Some("("),
        "RParen" => Some(")"),
        "LBrace" => Some("{"),
        "RBrace" => Some("}"),
        "LBracket" => Some("["),
        "RBracket" => Some("]"),
        "Plus" => Some("+"),
        "Minus" => Some("-"),
        "Star" => Some("*"),
        "At" => Some("@"),
        "Slash" => Some("/"),
        "Amp" => Some("&"),
        "Dot" => Some("."),
        "DotDot" => Some(".."),
        "DoubleColon" => Some("::"),
        "FatArrow" => Some("=>"),
        "Eq" => Some("="),
        "EqEq" => Some("=="),
        "NotEq" => Some("!="),
        "Lt" => Some("<"),
        "Le" => Some("<="),
        "Gt" => Some(">"),
        "Ge" => Some(">="),
        _ => None,
    }
}

fn line_contains_term(line: &str, term: &str) -> bool {
    if term.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .any(|part| part == term)
    } else {
        line.contains(term)
    }
}

fn term_score(term: &str) -> usize {
    if term.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        6
    } else {
        4
    }
}

fn structural_score(message: &str, trimmed_line: &str) -> usize {
    let mut score = 0;
    if message.contains("`if` condition") && trimmed_line.starts_with("if ") {
        score += 8;
    }
    if message.contains("literal cannot satisfy bool")
        && (trimmed_line.starts_with("if ") || trimmed_line.starts_with("while "))
    {
        score += 8;
    }
    if message.contains("literal cannot satisfy bool") && trimmed_line.contains(": bool") {
        score += 6;
    }
    if message.contains("`while` condition") && trimmed_line.starts_with("while ") {
        score += 8;
    }
    if message.contains("for range") && trimmed_line.starts_with("for ") {
        score += 8;
    }
    if message.contains("return") && trimmed_line.starts_with("return") {
        score += 8;
    }
    if message.contains("break") && trimmed_line.starts_with("break") {
        score += 8;
    }
    score
}

fn find_column(line: &str, term: &str) -> Option<usize> {
    line.find(term).map(|idx| line[..idx].chars().count() + 1)
}

fn first_non_ws_column(line: &str) -> Option<usize> {
    line.chars()
        .position(|c| !c.is_whitespace())
        .map(|idx| idx + 1)
}

fn find_function_bounds(src: &str, name: &str) -> Option<(usize, usize)> {
    let needle = format!("fn {name}");
    let lines: Vec<&str> = src.lines().collect();
    let start_idx = lines
        .iter()
        .position(|line| line.trim_start().starts_with(&needle))?;
    let mut depth = 0isize;
    let mut saw_body = false;

    for (idx, line) in lines.iter().enumerate().skip(start_idx) {
        for ch in line.chars() {
            match ch {
                '{' => {
                    depth += 1;
                    saw_body = true;
                }
                '}' if saw_body => {
                    depth -= 1;
                    if depth <= 0 {
                        return Some((start_idx + 1, idx + 1));
                    }
                }
                _ => {}
            }
        }
    }

    Some((start_idx + 1, lines.len()))
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
    let src =
        std::fs::read_to_string(&in_path).map_err(|e| format!("{}: {e}", in_path.display()))?;
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
