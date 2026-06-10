use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use std::collections::HashSet;

use crate::ast::{Block, Expr, FnDef, Stmt};
use crate::backend::llvm;
use crate::elab::{check_elaborated, elaborate_module, format_elaborated_module};
use crate::verify::{collect_vcs, discharge_vcs, format_vcs};
use crate::erase;
use crate::hir::{format_classical_module, lower_classical, lower_quantum, emit_qir};
use crate::frontend::resolve::{format_resolved_module, resolve_module, ResolvedModule};
use crate::frontend::surface::SurfaceModule;
use crate::frontend::parser::{Parser, tokenize};
use crate::nia_std::is_quantum_builtin_fn;

/// Which backend should consume the validated AST and emit LLVM IR text.
///
/// The default backend lowers nialang to classical LLVM IR consumed by `clang`.
/// `Qir` lowers supported quantum primitives into QIR-flavored LLVM IR.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Default,
    Qir,
}

impl Default for Backend {
    fn default() -> Self {
        Backend::Default
    }
}

/// Parsed surface module produced by the lexer and parser.
pub type ParsedModule = SurfaceModule;

/// Surface module with stable top-level `DefId`s assigned.
pub type ResolvedParsedModule = ResolvedModule;

/// Lexes and parses source text into a surface AST module.
pub fn parse_module(src: &str) -> Result<ParsedModule, String> {
    if let Some((line, column, ch)) = find_unsupported_char(src) {
        return Err(format_diagnostic_at_line(
            src,
            &format!("lex error: unsupported character `{ch}`"),
            line,
            column,
        ));
    }

    let tokens = tokenize(src);
    Parser::new(tokens)
        .parse_file()
        .map_err(|e| format_diagnostic(src, "parse error", None, &e))
}

/// Assigns stable ids to top-level items in a parsed module.
pub fn resolve_parsed_module(module: ParsedModule) -> Result<ResolvedParsedModule, String> {
    resolve_module(module)
}

/// Pretty-prints resolved top-level symbols for `--resolve-only` debugging.
pub fn format_resolved_ast(module: &ResolvedParsedModule) -> String {
    format_resolved_module(module)
}

/// Lowers a resolved module to Core and validates it with the Core checker.
pub fn elaborate_resolved_module(
    module: &ResolvedParsedModule,
) -> Result<crate::elab::ElaboratedModule, String> {
    let elaborated = elaborate_module(module)?;
    check_elaborated(&elaborated)?;
    Ok(elaborated)
}

/// Pretty-prints elaborated Core terms for `--elab-only` debugging.
pub fn format_elaborated_ast(module: &crate::elab::ElaboratedModule) -> String {
    format_elaborated_module(module)
}

/// Pretty-prints verification conditions for `--dump-vc` debugging.
pub fn format_verification_conditions(module: &crate::elab::ElaboratedModule) -> String {
    let mut vcs = collect_vcs(module);
    if let Err(e) = discharge_vcs(module, &mut vcs) {
        vcs.warnings.push(e);
    }
    format_vcs(&vcs)
}

/// Parse → resolve → elab → Core check → erase → HIR → LLVM.
pub fn compile_new_to_ll(src: &str) -> Result<String, String> {
    let module = parse_module(src)?;
    let resolved = resolve_parsed_module(module)?;
    if module_contains_quantum(&resolved) {
        return Err(
            "quantum syntax requires the QIR backend; pass `-q`".into(),
        );
    }
    let elaborated = elaborate_resolved_module(&resolved)?;
    let erased = erase::erase_module(&elaborated)?;
    let classical = lower_classical(resolved, erased);
    Ok(llvm::emit_module(&classical))
}

/// Parse → resolve → elab → Core check → QuantumHir → QIR.
pub fn compile_new_to_qir(src: &str) -> Result<String, String> {
    let module = parse_module(src)?;
    let resolved = resolve_parsed_module(module)?;
    let _elaborated = elaborate_resolved_module(&resolved)?;
    let quantum = lower_quantum(resolved);
    emit_qir(&quantum)
}

/// Parse → resolve → elab → erase → print classical HIR.
pub fn format_hir_ast(src: &str) -> Result<String, String> {
    let module = parse_module(src)?;
    let resolved = resolve_parsed_module(module)?;
    let elaborated = elaborate_resolved_module(&resolved)?;
    let erased = erase::erase_module(&elaborated)?;
    let classical = lower_classical(resolved, erased);
    Ok(format_classical_module(&classical))
}

/// Pretty-prints the parsed surface AST for `--core-only` debugging.
pub fn format_surface_ast(module: &ParsedModule) -> String {
    let mut out = String::from(";; nialang surface ast\n");
    if !module.structs.is_empty() {
        out.push_str(&format!("\n;; structs ({})\n", module.structs.len()));
        for item in &module.structs {
            out.push_str(&format!("{item:#?}\n"));
        }
    }
    if !module.enums.is_empty() {
        out.push_str(&format!("\n;; enums ({})\n", module.enums.len()));
        for item in &module.enums {
            out.push_str(&format!("{item:#?}\n"));
        }
    }
    if !module.vectors.is_empty() {
        out.push_str(&format!("\n;; vectors ({})\n", module.vectors.len()));
        for item in &module.vectors {
            out.push_str(&format!("{item:#?}\n"));
        }
    }
    if !module.fns.is_empty() {
        out.push_str(&format!("\n;; functions ({})\n", module.fns.len()));
        for item in &module.fns {
            out.push_str(&format!("{item:#?}\n"));
        }
    }
    out
}

/// Compiles nialang source text into LLVM IR via the new pipeline
/// (parse → resolve → elab → Core check → erase → LLVM).
pub fn compile_to_ll(src: &str) -> Result<String, String> {
    compile_to_ll_with(src, Backend::Default)
}

/// Dispatches to the new classical pipeline or the legacy QIR pipeline.
pub fn compile_to_ll_with(src: &str, backend: Backend) -> Result<String, String> {
    match backend {
        Backend::Default => compile_new_to_ll(src),
        Backend::Qir => compile_new_to_qir(src),
    }
}

fn module_contains_quantum(resolved: &ResolvedModule) -> bool {
    let quantum_fns: HashSet<&str> = resolved
        .fns
        .iter()
        .filter(|f| f.def.is_quantum)
        .map(|f| f.name.as_str())
        .collect();
    resolved
        .fns
        .iter()
        .any(|f| fn_contains_quantum(&f.def, &quantum_fns))
}

fn fn_contains_quantum(f: &FnDef, quantum_fns: &HashSet<&str>) -> bool {
    f.is_quantum || block_contains_quantum(&f.body, quantum_fns)
}

fn block_contains_quantum(block: &Block, quantum_fns: &HashSet<&str>) -> bool {
    block.stmts.iter().any(|st| stmt_contains_quantum(st, quantum_fns))
        || block
            .tail
            .as_ref()
            .is_some_and(|e| expr_contains_quantum(e, quantum_fns))
}

fn stmt_contains_quantum(st: &Stmt, quantum_fns: &HashSet<&str>) -> bool {
    match st {
        Stmt::Let { init, .. } | Stmt::Expr(init) | Stmt::Return(init) => {
            expr_contains_quantum(init, quantum_fns)
        }
        Stmt::Assign { target, value } => {
            expr_contains_quantum(target, quantum_fns) || expr_contains_quantum(value, quantum_fns)
        }
        Stmt::If { cond, then_block } => {
            expr_contains_quantum(cond, quantum_fns) || block_contains_quantum(then_block, quantum_fns)
        }
        Stmt::While { cond, body } => {
            expr_contains_quantum(cond, quantum_fns) || block_contains_quantum(body, quantum_fns)
        }
        Stmt::Loop { body } | Stmt::Gpu { body } | Stmt::Quant { body } => {
            block_contains_quantum(body, quantum_fns)
        }
        Stmt::For {
            start, end, body, ..
        } => {
            expr_contains_quantum(start, quantum_fns)
                || expr_contains_quantum(end, quantum_fns)
                || block_contains_quantum(body, quantum_fns)
        }
        Stmt::Admit(_) => false,
        Stmt::Break => false,
    }
}

fn expr_contains_quantum(e: &Expr, quantum_fns: &HashSet<&str>) -> bool {
    match e {
        Expr::Quant { body } => block_contains_quantum(body, quantum_fns),
        Expr::Neg(inner)
        | Expr::Not(inner)
        | Expr::BitNot(inner)
        | Expr::AddrOf(inner)
        | Expr::Deref(inner)
        | Expr::Field(inner, _) => expr_contains_quantum(inner, quantum_fns),
        Expr::Add(l, r)
        | Expr::Sub(l, r)
        | Expr::Mul(l, r)
        | Expr::VecDot(l, r)
        | Expr::Div(l, r)
        | Expr::Rem(l, r)
        | Expr::BitAnd(l, r)
        | Expr::BitOr(l, r)
        | Expr::BitXor(l, r)
        | Expr::Shl(l, r)
        | Expr::Shr(l, r)
        | Expr::Eq(l, r)
        | Expr::Ne(l, r)
        | Expr::Lt(l, r)
        | Expr::Le(l, r)
        | Expr::Gt(l, r)
        | Expr::Ge(l, r)
        | Expr::Index(l, r) => {
            expr_contains_quantum(l, quantum_fns) || expr_contains_quantum(r, quantum_fns)
        }
        Expr::Call { name, args, .. } => {
            is_quantum_call(name, quantum_fns)
                || args.iter().any(|a| expr_contains_quantum(a, quantum_fns))
        }
        Expr::GenericCall { name, args, .. } => {
            is_quantum_call(name, quantum_fns)
                || args.iter().any(|a| expr_contains_quantum(a, quantum_fns))
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_contains_quantum(receiver, quantum_fns)
                || args.iter().any(|a| expr_contains_quantum(a, quantum_fns))
        }
        Expr::StructLit { fields, .. }
        | Expr::VectorLit { fields, .. }
        | Expr::EnumStruct { fields, .. } => fields
            .iter()
            .any(|(_, e)| expr_contains_quantum(e, quantum_fns)),
        Expr::AnonVectorLit(elems)
        | Expr::ArrayLit(elems)
        | Expr::EnumTuple { args: elems, .. } => {
            elems.iter().any(|e| expr_contains_quantum(e, quantum_fns))
        }
        Expr::Match { scrutinee, arms } => {
            expr_contains_quantum(scrutinee, quantum_fns)
                || arms
                    .iter()
                    .any(|(_, e)| expr_contains_quantum(e, quantum_fns))
        }
        Expr::Gpu { body } => block_contains_quantum(body, quantum_fns),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Ident(_)
        | Expr::EnumVariant { .. } => false,
    }
}

fn is_quantum_call(name: &str, quantum_fns: &HashSet<&str>) -> bool {
    is_quantum_builtin_fn(name) || quantum_fns.contains(name)
}

pub(crate) fn format_elab_diagnostic(src: &str, message: &str) -> String {
    let function = message
        .strip_prefix("in `")
        .and_then(|rest| rest.split('`').next());
    let detail = message
        .strip_prefix("in `")
        .and_then(|rest| rest.split_once("`: "))
        .map(|(_, detail)| detail)
        .unwrap_or(message);
    format_diagnostic(src, "error", function, detail)
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
                | '%'
                | '&'
                | '|'
                | '^'
                | '~'
                | '.'
                | '='
                | '!'
                | '#'
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
        if function_bounds.is_some() && !in_function {
            continue;
        }
        let mut score = usize::from(in_function) * 3;

        for term in &terms {
            if line_contains_term(text, term) {
                score += term_score(term);
            }
        }
        score += structural_score(message, trimmed);
        if in_function && trimmed.starts_with("fn ") {
            let penalty = if message.contains("argument") { 10 } else { 5 };
            score = score.saturating_sub(penalty);
        }
        if message.contains("argument") && trimmed.contains('(') {
            score += 8;
        }

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
        "Extern" => Some("extern"),
        "Fn" => Some("fn"),
        "Let" => Some("let"),
        "Struct" => Some("struct"),
        "Vector" => Some("vector"),
        "Enum" => Some("enum"),
        "Quant" => Some("quant"),
        "Gpu" => Some("gpu"),
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
        "Bang" => Some("!"),
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
    let quant_needle = format!("quant fn {name}");
    let lines: Vec<&str> = src.lines().collect();
    let start_idx = lines.iter().position(|line| {
        let trimmed = line.trim_start();
        trimmed.starts_with(&needle) || trimmed.starts_with(&quant_needle)
    })?;
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
/// `nialang <file.nia> --emit-ll [out.ll]`
/// `nialang <file.nia> --emit-asm [out.s]`
/// `nialang <file.nia> --lib -o libname.{dylib,so,dll}`
///
/// ## Behavior
/// - Resolves input path robustly for `cargo run` and direct binary usage.
/// - Compiles `.nia` source into LLVM IR.
/// - Optionally emits generated IR to disk and exits.
/// - Optionally lowers generated IR to native assembly on disk and exits.
/// - Optionally runs generated QIR through qir-runner when `-q` is selected.
/// - Optionally dumps generated IR to disk when `-o` is provided in run mode.
/// - Invokes `clang` to produce a temporary native executable.
/// - Runs that executable and returns *its* exit status to caller.
/// - Removes temporary artifacts (`.ll` and executable) best-effort.
///
/// ## Errors
/// Returns descriptive errors for bad CLI flags, read/write failures, missing clang,
/// clang compile failures, and runtime launch failures.
pub fn run_cli() -> Result<i32, String> {
    let cli = parse_cli_args(std::env::args().skip(1))?;
    let in_path = resolve_input_path(cli.in_path)?;

    // Read the input program after path resolution, so diagnostics include final path.
    let src =
        std::fs::read_to_string(&in_path).map_err(|e| format!("{}: {e}", in_path.display()))?;
    if matches!(cli.mode, BuildMode::CoreOnly) {
        let module = parse_module(&src)?;
        print!("{}", format_surface_ast(&module));
        return Ok(0);
    }
    if matches!(cli.mode, BuildMode::ResolveOnly) {
        let module = parse_module(&src)?;
        let resolved = resolve_parsed_module(module)?;
        print!("{}", format_resolved_ast(&resolved));
        return Ok(0);
    }
    if matches!(cli.mode, BuildMode::ElabOnly) {
        let module = parse_module(&src)?;
        let resolved = resolve_parsed_module(module)?;
        let elaborated = elaborate_resolved_module(&resolved)
            .map_err(|e| format_elab_diagnostic(&src, &e))?;
        print!("{}", format_elaborated_ast(&elaborated));
        return Ok(0);
    }
    if matches!(cli.mode, BuildMode::DumpVc) {
        let module = parse_module(&src)?;
        let resolved = resolve_parsed_module(module)?;
        let elaborated = elaborate_resolved_module(&resolved)
            .map_err(|e| format_elab_diagnostic(&src, &e))?;
        print!("{}", format_verification_conditions(&elaborated));
        return Ok(0);
    }
    if matches!(cli.mode, BuildMode::DumpHir) {
        let out = format_hir_ast(&src).map_err(|e| format_elab_diagnostic(&src, &e))?;
        print!("{out}");
        return Ok(0);
    }

    let ll = compile_to_ll_with(&src, cli.backend)
        .map_err(|e| format_elab_diagnostic(&src, &e))?;

    match cli.mode {
        BuildMode::CoreOnly
        | BuildMode::ResolveOnly
        | BuildMode::ElabOnly
        | BuildMode::DumpVc
        | BuildMode::DumpHir => {
            unreachable!("handled before codegen")
        }
        BuildMode::QirRun { out_ll } => run_qir_runner_mode(&ll, out_ll),
        BuildMode::Run { out_ll } => run_executable_mode(&ll, out_ll),
        BuildMode::EmitLl { out_ll } => emit_ll_mode(&ll, &in_path, out_ll),
        BuildMode::EmitAsm { out_asm } => emit_asm_mode(&ll, &in_path, out_asm),
        BuildMode::Lib { out_lib } => {
            build_shared_library(&ll, &out_lib)?;
            eprintln!("built {}", out_lib.display());
            Ok(0)
        }
    }
}

fn run_qir_runner_mode(ll: &str, out_ll: Option<PathBuf>) -> Result<i32, String> {
    run_qir_runner_mode_impl(ll, out_ll)
}

#[cfg(feature = "qir-runner")]
fn run_qir_runner_mode_impl(ll: &str, out_ll: Option<PathBuf>) -> Result<i32, String> {
    let qir_runner_ll = prepare_for_qir_runner(ll)?;
    if let Some(ref p) = out_ll {
        write_text_file(p, &qir_runner_ll)?;
        eprintln!("wrote {}", p.display());
    }

    let mut stdout = std::io::stdout();
    runner::run_bytes(qir_runner_ll.as_bytes(), Some("main"), 1, None, &mut stdout)
        .map_err(|e| format!("qir-runner failed: {e}"))?;
    Ok(0)
}

#[cfg(not(feature = "qir-runner"))]
fn run_qir_runner_mode_impl(_ll: &str, _out_ll: Option<PathBuf>) -> Result<i32, String> {
    Err(
        "`-q` requires the optional `qir-runner` feature for now: run with `cargo run --features qir-runner -- <file> -q`"
            .into(),
    )
}

#[cfg(feature = "qir-runner")]
fn prepare_for_qir_runner(ll: &str) -> Result<String, String> {
    let ll = ll.replace(
        "target triple = \"unknown-unknown-unknown\"",
        &format!("target triple = \"{}\"", qir_runner_host_triple()),
    );
    if ll.contains("\"entry_point\"") {
        return Ok(ll);
    }
    attach_qir_runner_entry_point(&ll)
}

#[cfg(feature = "qir-runner")]
fn qir_runner_host_triple() -> &'static str {
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        "arm64-apple-darwin"
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        "x86_64-apple-darwin"
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        "x86_64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
        "aarch64-unknown-linux-gnu"
    } else if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        "x86_64-pc-windows-msvc"
    } else {
        "unknown-unknown-unknown"
    }
}

#[cfg(feature = "qir-runner")]
fn attach_qir_runner_entry_point(ll: &str) -> Result<String, String> {
    let attr_id = next_llvm_attr_id(ll);
    let mut saw_main = false;
    let mut out = String::new();

    for line in ll.lines() {
        if line.starts_with("define ") && line.contains("@main(") {
            saw_main = true;
            if line.contains(" #") {
                return Err(
                    "qir-runner mode cannot attach entry_point to a main function that already has LLVM attributes"
                        .into(),
                );
            }
            let Some(prefix) = line.strip_suffix(" {") else {
                return Err(format!(
                    "qir-runner mode could not rewrite main definition: {line}"
                ));
            };
            out.push_str(prefix);
            out.push_str(&format!(" #{attr_id} {{\n"));
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }

    if !saw_main {
        return Err("qir-runner mode requires a `main` function".into());
    }

    out.push('\n');
    out.push_str(&format!(
        "attributes #{attr_id} = {{ \"entry_point\" \"qir_profiles\"=\"adaptive_profile\" \"required_num_qubits\"=\"0\" \"required_num_results\"=\"0\" }}\n"
    ));
    Ok(out)
}

#[cfg(feature = "qir-runner")]
fn next_llvm_attr_id(ll: &str) -> usize {
    ll.lines()
        .filter_map(|line| line.strip_prefix("attributes #"))
        .filter_map(|rest| rest.split_whitespace().next())
        .filter_map(|n| n.parse::<usize>().ok())
        .max()
        .map_or(0, |n| n + 1)
}

fn emit_ll_mode(ll: &str, in_path: &Path, out_ll: Option<PathBuf>) -> Result<i32, String> {
    let out_path = match out_ll {
        Some(path) => path,
        None => default_output_path(in_path, "ll")?,
    };
    write_text_file(&out_path, ll)?;
    eprintln!("wrote {}", out_path.display());
    Ok(0)
}

fn emit_asm_mode(ll: &str, in_path: &Path, out_asm: Option<PathBuf>) -> Result<i32, String> {
    let out_path = match out_asm {
        Some(path) => path,
        None => default_output_path(in_path, "s")?,
    };
    ensure_parent_dir(&out_path)?;

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let pid = std::process::id();
    let tmp_ll = std::env::temp_dir().join(format!("nialang-{pid}-{nonce}-asm.ll"));
    std::fs::write(&tmp_ll, ll).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("clang");
    configure_clang_sdk(&mut cmd)?;
    let clang_ok = cmd
        .arg("-S")
        .arg(&tmp_ll)
        .arg("-o")
        .arg(&out_path)
        .status()
        .map_err(|e| {
            format!(
                "failed to run `clang`: {e}\n\
                 Install LLVM/clang and ensure `clang` is on PATH."
            )
        })?;

    let _ = std::fs::remove_file(&tmp_ll);
    if !clang_ok.success() {
        return Err("clang failed to emit assembly from the generated LLVM IR".into());
    }

    eprintln!("wrote {}", out_path.display());
    Ok(0)
}

fn run_executable_mode(ll: &str, out_ll: Option<PathBuf>) -> Result<i32, String> {
    if let Some(ref p) = out_ll {
        write_text_file(p, ll)?;
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

    std::fs::write(&tmp_ll, ll).map_err(|e| e.to_string())?;

    // Compile generated IR into a native executable via system clang.
    let mut cmd = Command::new("clang");
    configure_clang_sdk(&mut cmd)?;
    cmd.arg(&tmp_ll).arg("-o").arg(&tmp_exe);
    if should_link_explicit_libm() {
        cmd.arg("-lm");
    }
    let clang_ok = cmd.status().map_err(|e| {
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

fn build_shared_library(ll: &str, out_lib: &Path) -> Result<(), String> {
    ensure_parent_dir(out_lib)?;
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let pid = std::process::id();
    let tmp_dir = std::env::temp_dir();
    let tmp_ll = tmp_dir.join(format!("nialang-{pid}-{nonce}-lib.ll"));
    std::fs::write(&tmp_ll, ll).map_err(|e| e.to_string())?;

    let mut cmd = Command::new("clang");
    configure_clang_sdk(&mut cmd)?;
    if cfg!(target_os = "macos") {
        cmd.arg("-dynamiclib");
    } else {
        cmd.arg("-shared");
        if !cfg!(windows) {
            cmd.arg("-fPIC");
        }
    }
    cmd.arg(&tmp_ll).arg("-o").arg(out_lib);
    if should_link_explicit_libm() {
        cmd.arg("-lm");
    }
    let clang_ok = cmd.status().map_err(|e| {
        format!(
            "failed to run `clang`: {e}\n\
                 Install LLVM/clang and ensure `clang` is on PATH."
        )
    })?;

    let _ = std::fs::remove_file(&tmp_ll);
    if !clang_ok.success() {
        return Err(format!(
            "clang failed to build shared library `{}`",
            out_lib.display()
        ));
    }
    Ok(())
}

fn write_text_file(path: &Path, text: &str) -> Result<(), String> {
    ensure_parent_dir(path)?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

fn should_link_explicit_libm() -> bool {
    !cfg!(any(windows, target_os = "macos"))
}

fn configure_clang_sdk(cmd: &mut Command) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }

    let sdk = Command::new("xcrun")
        .args(["--sdk", "macosx", "--show-sdk-path"])
        .output()
        .map_err(|e| format!("failed to run `xcrun --sdk macosx --show-sdk-path`: {e}"))?;
    if !sdk.status.success() {
        let stderr = String::from_utf8_lossy(&sdk.stderr);
        return Err(format!("failed to resolve macOS SDK with xcrun: {stderr}"));
    }

    let path = String::from_utf8_lossy(&sdk.stdout).trim().to_string();
    if path.is_empty() || !Path::new(&path).exists() {
        return Err(format!("xcrun returned missing macOS SDK path `{path}`"));
    }

    cmd.env_remove("SDKROOT");
    cmd.arg("-isysroot").arg(path);
    Ok(())
}

fn default_output_path(in_path: &Path, extension: &str) -> Result<PathBuf, String> {
    let stem = in_path
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("out");
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    Ok(cwd.join(format!("{stem}.{extension}")))
}

fn ensure_parent_dir(path: &Path) -> Result<(), String> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CliArgs {
    pub(crate) in_path: PathBuf,
    pub(crate) mode: BuildMode,
    pub(crate) backend: Backend,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BuildMode {
    /// Parse only and print the surface AST (new pipeline debugging).
    CoreOnly,
    /// Parse and resolve top-level names, then print id tables.
    ResolveOnly,
    /// Parse, resolve, elaborate to Core, typecheck, and print terms.
    ElabOnly,
    /// Parse, resolve, elaborate, collect VCs, and print verification goals.
    DumpVc,
    /// Parse, resolve, elaborate, erase, and print classical HIR.
    DumpHir,
    /// Run generated QIR through qir-runner.
    QirRun {
        out_ll: Option<PathBuf>,
    },
    Run {
        out_ll: Option<PathBuf>,
    },
    EmitLl {
        out_ll: Option<PathBuf>,
    },
    EmitAsm {
        out_asm: Option<PathBuf>,
    },
    Lib {
        out_lib: PathBuf,
    },
}

pub(crate) fn parse_cli_args<I, S>(args: I) -> Result<CliArgs, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = args.into_iter().map(Into::into).peekable();
    let in_path: PathBuf = args.next().ok_or_else(usage)?.into();
    let mut out: Option<PathBuf> = None;
    let mut lib = false;
    let mut qir = false;
    let mut emit_ll = false;
    let mut emit_asm = false;
    let mut core_only = false;
    let mut resolve_only = false;
    let mut elab_only = false;
    let mut dump_vc = false;
    let mut dump_hir = false;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--core-only" => core_only = true,
            "--resolve-only" => resolve_only = true,
            "--elab-only" => elab_only = true,
            "--dump-vc" => dump_vc = true,
            "--dump-hir" => dump_hir = true,
            "--lib" => lib = true,
            "-q" | "--qir" => qir = true,
            "--emit-ll" => {
                if emit_ll {
                    return Err("duplicate --emit-ll flag".into());
                }
                emit_ll = true;
                if let Some(next) = args.peek() {
                    if !next.starts_with('-') {
                        if out.is_some() {
                            return Err("duplicate output path".into());
                        }
                        out = Some(args.next().expect("peeked arg exists").into());
                    }
                }
            }
            "--emit-asm" => {
                if emit_asm {
                    return Err("duplicate --emit-asm flag".into());
                }
                emit_asm = true;
                if let Some(next) = args.peek() {
                    if !next.starts_with('-') {
                        if out.is_some() {
                            return Err("duplicate output path".into());
                        }
                        out = Some(args.next().expect("peeked arg exists").into());
                    }
                }
            }
            "-o" => {
                if out.is_some() {
                    return Err("duplicate -o flag".into());
                }
                out = Some(
                    args.next()
                        .ok_or_else(|| "-o requires a path".to_string())?
                        .into(),
                );
            }
            _ => return Err(format!("unknown flag `{a}`\n{}", usage())),
        }
    }

    let debug_flags = [core_only, resolve_only, elab_only, dump_vc, dump_hir]
        .into_iter()
        .filter(|&b| b)
        .count();
    if debug_flags > 1 {
        return Err(
            "`--core-only`, `--resolve-only`, `--elab-only`, `--dump-vc`, and `--dump-hir` are mutually exclusive"
                .into(),
        );
    }
    if core_only {
        if lib || qir || emit_ll || emit_asm || out.is_some() {
            return Err("`--core-only` cannot be combined with other output or backend flags".into());
        }
        return Ok(CliArgs {
            in_path,
            mode: BuildMode::CoreOnly,
            backend: Backend::Default,
        });
    }
    if resolve_only {
        if lib || qir || emit_ll || emit_asm || out.is_some() {
            return Err("`--resolve-only` cannot be combined with other output or backend flags".into());
        }
        return Ok(CliArgs {
            in_path,
            mode: BuildMode::ResolveOnly,
            backend: Backend::Default,
        });
    }
    if elab_only {
        if lib || qir || emit_ll || emit_asm || out.is_some() {
            return Err("`--elab-only` cannot be combined with other output or backend flags".into());
        }
        return Ok(CliArgs {
            in_path,
            mode: BuildMode::ElabOnly,
            backend: Backend::Default,
        });
    }
    if dump_vc {
        if lib || qir || emit_ll || emit_asm || out.is_some() {
            return Err("`--dump-vc` cannot be combined with other output or backend flags".into());
        }
        return Ok(CliArgs {
            in_path,
            mode: BuildMode::DumpVc,
            backend: Backend::Default,
        });
    }
    if dump_hir {
        if lib || qir || emit_ll || emit_asm || out.is_some() {
            return Err("`--dump-hir` cannot be combined with other output or backend flags".into());
        }
        return Ok(CliArgs {
            in_path,
            mode: BuildMode::DumpHir,
            backend: Backend::Default,
        });
    }

    if qir && lib {
        return Err("`-q`/`--qir` cannot be combined with `--lib`".into());
    }
    if qir && emit_ll {
        return Err("`-q`/`--qir` cannot be combined with `--emit-ll`".into());
    }
    if qir && emit_asm {
        return Err("`-q`/`--qir` cannot be combined with `--emit-asm`".into());
    }
    if lib && emit_ll {
        return Err("--lib and --emit-ll cannot be used together".into());
    }
    if lib && emit_asm {
        return Err("--lib and --emit-asm cannot be used together".into());
    }
    if emit_ll && emit_asm {
        return Err("--emit-ll and --emit-asm cannot be used together".into());
    }

    let backend = if qir { Backend::Qir } else { Backend::Default };
    let mode = if lib {
        BuildMode::Lib {
            out_lib: out.ok_or_else(|| "--lib requires -o <library-path>".to_string())?,
        }
    } else if qir {
        BuildMode::QirRun { out_ll: out }
    } else if emit_ll {
        BuildMode::EmitLl { out_ll: out }
    } else if emit_asm {
        BuildMode::EmitAsm { out_asm: out }
    } else {
        BuildMode::Run { out_ll: out }
    };
    Ok(CliArgs {
        in_path,
        mode,
        backend,
    })
}

fn usage() -> String {
    "usage: nialang <file.nia> [-o out.ll]\n\
     usage: nialang <file.nia> --emit-ll [out.ll]\n\
     usage: nialang <file.nia> --emit-asm [out.s]\n\
     usage: nialang <file.nia> --lib -o libname.{dylib,so,dll}\n\
     usage: nialang <file.nia> -q [-o out.ll]   (run QIR through qir-runner)\n\
     usage: nialang <file.nia> --core-only      (parse and print surface AST)\n\
     usage: nialang <file.nia> --resolve-only  (parse, resolve names, print ids)\n\
     usage: nialang <file.nia> --elab-only     (parse, resolve, elaborate Core, check)\n\
     usage: nialang <file.nia> --dump-vc       (parse, resolve, elaborate, print VCs)\n\
     usage: nialang <file.nia> --dump-hir      (parse, resolve, elaborate, erase, print HIR)\n\
     Default mode: parse → resolve → elab → erase → LLVM.\n\
     Use --emit-ll to write textual LLVM IR and exit. Without a path, writes <input>.ll in the current directory.\n\
     Use --emit-asm to write native assembly and exit. Without a path, writes <input>.s in the current directory.\n\
     Use --lib to build a shared library instead of running the program.\n\
     Use -q / --qir to lower supported quantum primitives to QIR and run via qir-runner.\n\
     Use --core-only to debug the new compiler frontend without typechecking or codegen.\n\
     Use --resolve-only to inspect assigned DefIds before elaboration.\n\
     Use --elab-only to inspect elaborated Core terms after the tier-0 checker.\n\
     Use --dump-vc to inspect refinement verification conditions.\n\
     Use --dump-hir to inspect erased classical HIR before LLVM lowering."
        .to_string()
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
