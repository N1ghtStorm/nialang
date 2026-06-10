//! CLI parsing, diagnostics, QIR integration, and path resolution.

use crate::driver::fixtures::read_fixture;

fn compile_fixture_qir_ok(path: &str) -> String {
    let src = read_fixture(path);
    crate::driver::pipeline::compile_to_ll_with(&src, crate::driver::pipeline::Backend::Qir)
        .expect("qir pipeline")
}

#[test]
fn parse_cli_args_supports_resolve_only_mode() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "--resolve-only",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::ResolveOnly
    );
}

#[test]
fn parse_cli_args_rejects_core_only_with_resolve_only() {
    let err = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "--core-only",
        "--resolve-only",
    ])
    .expect_err("flag conflict");
    assert!(err.contains("mutually exclusive"), "{err}");
}

#[test]
fn parse_cli_args_supports_core_only_mode() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "--core-only",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::CoreOnly
    );
}

#[test]
fn parse_cli_args_rejects_core_only_with_emit_ll() {
    let err = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "--core-only",
        "--emit-ll",
    ])
    .expect_err("core-only conflict");
    assert!(err.contains("cannot be combined"), "{err}");
}

#[test]
fn parse_cli_args_supports_library_mode() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/sample_extern_lib.nia",
        "--lib",
        "-o",
        "build/libnia_sample.dylib",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::Lib {
            out_lib: std::path::PathBuf::from("build/libnia_sample.dylib")
        }
    );
}

#[test]
fn parse_cli_args_rejects_library_mode_without_output() {
    let err = crate::driver::pipeline::parse_cli_args(["examples/sample_extern_lib.nia", "--lib"])
        .expect_err("missing lib output");
    assert!(err.contains("--lib requires -o"), "{err}");
}

#[test]
fn parse_cli_args_supports_qir_flag_short_and_long() {
    for flag in ["-q", "--qir"] {
        let args = crate::driver::pipeline::parse_cli_args(["examples/tests/ok_minimal.nia", flag])
            .expect("parse args");
        assert_eq!(args.backend, crate::driver::pipeline::Backend::Qir);
        assert_eq!(
            args.mode,
            crate::driver::pipeline::BuildMode::QirRun { out_ll: None }
        );
    }
}

#[test]
fn parse_cli_args_qir_run_with_output_path() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "-q",
        "-o",
        "build/out.ll",
    ])
    .expect("parse args");
    assert_eq!(args.backend, crate::driver::pipeline::Backend::Qir);
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::QirRun {
            out_ll: Some(std::path::PathBuf::from("build/out.ll"))
        }
    );
}

#[test]
fn parse_cli_args_supports_dump_hir_mode() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "--dump-hir",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::DumpHir
    );
}

#[test]
fn parse_cli_args_supports_emit_ll_mode_without_output() {
    let args = crate::driver::pipeline::parse_cli_args(["examples/sample_floats.nia", "--emit-ll"])
        .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::EmitLl { out_ll: None }
    );
}

#[test]
fn parse_cli_args_supports_emit_ll_mode_with_output() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-ll",
        "build/sample_floats.ll",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::EmitLl {
            out_ll: Some(std::path::PathBuf::from("build/sample_floats.ll"))
        }
    );
}

#[test]
fn parse_cli_args_rejects_qir_with_lib() {
    let err = crate::driver::pipeline::parse_cli_args([
        "examples/tests/ok_minimal.nia",
        "-q",
        "--lib",
        "-o",
        "lib.dylib",
    ])
    .expect_err("qir + lib must be rejected");
    assert!(err.contains("cannot be combined with `--lib`"), "{err}");
}

#[test]
fn compile_qft4_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/qft4.nia");
    assert!(ir.contains("; qubit 0:"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__qis__swap__body(ptr"),
        "{ir}"
    );
}

#[test]
fn compile_iqft4_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/iqft4.nia");
    assert!(ir.contains("; qubit 0:"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__qis__rz__body(double "),
        "{ir}"
    );
}

#[test]
fn compile_qubit_read_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/qubit_read.nia");
    assert!(ir.contains("; qubit 0:"), "{ir}");
    assert!(ir.contains("call void @__quantum__qis__x__body"), "{ir}");
    assert!(ir.contains("call void @__quantum__qis__mz__body"), "{ir}");
    assert!(ir.contains("call i1 @__quantum__rt__read_result"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__rt__bool_record_output(i1 %"),
        "{ir}"
    );
    assert!(
        ir.contains("call void @__quantum__rt__message(ptr %"),
        "{ir}"
    );
}

#[test]
fn compile_full_nialang_quantum_example_still_needs_classical_elab_work() {
    let src = read_fixture("examples/quantum/qir_full_nialang.nia");
    let err = crate::driver::pipeline::compile_to_ll_with(
        &src,
        crate::driver::pipeline::Backend::Qir,
    )
    .expect_err("variable for-range in main not yet supported by new elab");
    assert!(err.contains("cannot unify") || err.contains("Pi {"), "{err}");
}

#[test]
fn compile_to_ll_with_qir_emits_qir_module() {
    let src = "fn main() i32 { 0 }\n";
    let ir =
        crate::driver::pipeline::compile_to_ll_with(src, crate::driver::pipeline::Backend::Qir)
            .expect("qir module");
    assert!(ir.contains("generated by nialang (QIR backend)"), "{ir}");
    assert!(ir.contains("define void @main()"), "{ir}");
}

#[test]
fn compile_to_ll_with_qir_runs_frontend_validation() {
    let src = "fn main() i32 { if 1 { return 0 } 0 }\n";
    let err =
        crate::driver::pipeline::compile_to_ll_with(src, crate::driver::pipeline::Backend::Qir)
            .expect_err("type error must surface even in qir mode");
    assert!(
        err.contains("cannot unify") || err.contains("type error"),
        "{err}"
    );
}

#[test]
fn compile_to_ll_rejects_quant_fn_without_qir_backend() {
    let src = r#"
quant fn prepare() {
    let q = qubit();
    H(q);
}

fn main() i32 {
    quant {
        prepare();
    }
    0
}
"#;
    let err = crate::driver::pipeline::compile_to_ll(src)
        .expect_err("quant fn requires qir backend");
    assert!(
        err.contains("quantum syntax requires the QIR backend"),
        "{err}"
    );
}

#[test]
fn compile_to_ll_rejects_quant_block_without_qir_backend() {
    let src = r#"
fn main() i32 {
    quant {
        let q = qubit();
        H(q);
    }
    0
}
"#;
    let err = crate::driver::pipeline::compile_to_ll(src)
        .expect_err("quant block requires qir backend");
    assert!(
        err.contains("quantum syntax requires the QIR backend"),
        "{err}"
    );
}

#[test]
fn parse_cli_args_supports_emit_ll_mode_with_dash_o() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-ll",
        "-o",
        "build/sample_floats.ll",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::EmitLl {
            out_ll: Some(std::path::PathBuf::from("build/sample_floats.ll"))
        }
    );
}

#[test]
fn parse_cli_args_supports_emit_asm_mode_without_output() {
    let args =
        crate::driver::pipeline::parse_cli_args(["examples/sample_floats.nia", "--emit-asm"])
            .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::EmitAsm { out_asm: None }
    );
}

#[test]
fn parse_cli_args_supports_emit_asm_mode_with_output() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-asm",
        "build/sample_floats.s",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::EmitAsm {
            out_asm: Some(std::path::PathBuf::from("build/sample_floats.s"))
        }
    );
}

#[test]
fn parse_cli_args_supports_emit_asm_mode_with_dash_o() {
    let args = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-asm",
        "-o",
        "build/sample_floats.s",
    ])
    .expect("parse args");
    assert_eq!(
        args.mode,
        crate::driver::pipeline::BuildMode::EmitAsm {
            out_asm: Some(std::path::PathBuf::from("build/sample_floats.s"))
        }
    );
}

#[test]
fn parse_cli_args_rejects_emit_ll_with_library_mode() {
    let err = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-ll",
        "--lib",
        "-o",
        "build/libbad.dylib",
    ])
    .expect_err("emit ll and lib conflict");
    assert!(err.contains("--lib and --emit-ll"), "{err}");
}

#[test]
fn parse_cli_args_rejects_emit_asm_with_library_mode() {
    let err = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-asm",
        "--lib",
        "-o",
        "build/libbad.dylib",
    ])
    .expect_err("emit asm and lib conflict");
    assert!(err.contains("--lib and --emit-asm"), "{err}");
}

#[test]
fn parse_cli_args_rejects_emit_asm_with_emit_ll_mode() {
    let err = crate::driver::pipeline::parse_cli_args([
        "examples/sample_floats.nia",
        "--emit-asm",
        "--emit-ll",
    ])
    .expect_err("emit asm and emit ll conflict");
    assert!(err.contains("--emit-ll and --emit-asm"), "{err}");
}

#[test]
fn compile_error_includes_problem_line_for_type_errors() {
    let src = r#"
fn main() i32 {
    if 1 {
        return 0
    }
    0
}
"#;
    let err = crate::driver::pipeline::compile_new_to_ll(src).expect_err("type error");
    assert!(err.contains("in `main`"), "{err}");
    assert!(err.contains("cannot unify") || err.contains("expects bool"), "{err}");
}

#[test]
fn compile_error_includes_source_line_for_parse_errors() {
    let src = r#"
fn main() i32 {
    let x = 1
    let y = 2;
    0
}
"#;
    let err = crate::driver::pipeline::compile_to_ll(src).expect_err("parse error");
    assert!(err.contains("parse error:"), "{err}");
    assert!(err.contains("|"), "{err}");
    assert!(err.contains("let"), "{err}");
}

#[test]
fn compile_error_reports_unsupported_characters_at_their_line() {
    let src = r#"
vector Vec2 i32 [
    X,ыввыв
    Y,
]

fn main() i32 { 0 }
"#;
    let err = crate::driver::pipeline::compile_to_ll(src).expect_err("lex error");
    assert!(err.contains("lex error: unsupported character"), "{err}");
    assert!(err.contains("--> line 3"), "{err}");
    assert!(err.contains("X,ыввыв"), "{err}");
}

#[test]
fn resolve_input_path_works_for_existing_relative_fixture() {
    let p = std::path::PathBuf::from("examples/tests/ok_minimal.nia");
    let r = crate::driver::pipeline::resolve_input_path(p);
    assert!(r.is_ok(), "{r:?}");
}
