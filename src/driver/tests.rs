/// Integration-style tests: full compile pipeline and path resolution.

fn compile_fixture_ok(path: &str) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let src = std::fs::read_to_string(&p).unwrap();
    crate::driver::pipeline::compile_to_ll(&src).expect("full pipeline");
}

fn compile_fixture_qir_ok(path: &str) -> String {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let src = std::fs::read_to_string(&p).unwrap();
    crate::driver::pipeline::compile_to_ll_with(&src, crate::driver::pipeline::Backend::Qir)
        .expect("qir pipeline")
}

#[test]
/// Ensures a representative set of valid language fixtures still compiles end-to-end.
fn compile_fixtures_pipeline() {
    let files = [
        "examples/tests/ok_minimal.nia",
        "examples/tests/ok_if_return.nia",
        "examples/tests/ok_tuple_struct.nia",
        "examples/tests/ok_struct_named.nia",
        "examples/tests/ok_impl_methods.nia",
        "examples/tests/ok_quant_scope.nia",
        "examples/tests/ok_gpu_scope.nia",
        "examples/tests/ok_print_primitives.nia",
        "examples/tests/ok_floats.nia",
        "examples/tests/ok_pointers.nia",
        "examples/tests/ok_nested_if.nia",
        "examples/tests/ok_tuple_named_mix.nia",
        "examples/tests/ok_array.nia",
        "examples/tests/ok_array_index.nia",
        "examples/tests/ok_array_index_store.nia",
        "examples/tests/ok_array_reverse.nia",
        "examples/tests/ok_array_len.nia",
        "examples/tests/ok_array_to_vec.nia",
        "examples/tests/ok_vector_to_array.nia",
        "examples/tests/ok_array_matrix_conversions.nia",
        "examples/tests/ok_alloc_heap.nia",
        "examples/tests/ok_ptr_write.nia",
        "examples/tests/ok_ptr_array_write.nia",
        "examples/tests/ok_readme_arrays.nia",
        "examples/tests/ok_readme_enums.nia",
        "examples/tests/ok_readme_pointers.nia",
        "examples/tests/ok_enum_match.nia",
        "examples/tests/ok_enum_payload_match.nia",
        "examples/tests/ok_print_enum.nia",
        "examples/tests/ok_for_range.nia",
        "examples/tests/ok_while.nia",
        "examples/tests/ok_loop.nia",
        "examples/tests/ok_compound_assign.nia",
        "examples/tests/ok_string.nia",
        "examples/sample_all.nia",
        "examples/sample_struct_methods_big.nia",
        "examples/sample_matrix_rc.nia",
        "examples/sample_matrix_arith.nia",
        "examples/sample_matrix_det.nia",
        "examples/sample_matrix_vector.nia",
        "examples/sample_complex.nia",
        "examples/sample_dft4.nia",
        "examples/sample_list.nia",
        "examples/sample_dft_list.nia",
        "examples/sample_extern_fn.nia",
        "examples/sample_extern_lib.nia",
    ];
    for f in files {
        compile_fixture_ok(f);
    }
}

#[test]
/// Ensures known-invalid fixtures fail during the compile pipeline.
fn compile_multiple_error_fixtures() {
    let err_files = [
        "examples/tests/err_type_mismatch.nia",
        "examples/tests/err_type_add_bool.nia",
        "examples/tests/err_type_if_non_bool.nia",
        "examples/tests/err_type_tuple_with_named_literal.nia",
        "examples/tests/err_array_len_mismatch.nia",
        "examples/tests/err_shadow_let.nia",
        "examples/tests/err_for_range_bool.nia",
        "examples/tests/err_for_return_in_for.nia",
        "examples/tests/err_while_cond_int.nia",
        "examples/tests/err_loop_no_break.nia",
        "examples/tests/err_break_outside_loop.nia",
        "examples/tests/err_break_in_while.nia",
        "examples/tests/err_div_by_zero.nia",
    ];
    for f in err_files {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(f);
        let src = std::fs::read_to_string(&p).unwrap();
        let r = crate::driver::pipeline::compile_to_ll(&src);
        assert!(r.is_err(), "fixture unexpectedly compiled: {f}");
    }
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
    assert!(ir.contains("\"required_num_qubits\"=\"4\""), "{ir}");
    assert!(ir.contains("\"required_num_results\"=\"4\""), "{ir}");
    assert!(
        ir.contains("call void @__quantum__qis__swap__body(ptr null, ptr inttoptr (i64 3 to ptr))"),
        "{ir}"
    );
}

#[test]
fn compile_iqft4_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/iqft4.nia");
    assert!(ir.contains("\"required_num_qubits\"=\"4\""), "{ir}");
    assert!(ir.contains("\"required_num_results\"=\"4\""), "{ir}");
    assert!(
        ir.contains("call void @__quantum__qis__rz__body(double -7.85398163397448279e-1"),
        "{ir}"
    );
}

#[test]
fn compile_qubit_read_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/qubit_read.nia");
    assert!(ir.contains("\"qir_profiles\"=\"adaptive_profile\""), "{ir}");
    assert!(ir.contains("call i1 @__quantum__rt__read_result"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__rt__bool_record_output(i1 %qread0, ptr null)"),
        "{ir}"
    );
}

#[test]
fn compile_to_ll_with_qir_emits_qir_module() {
    let src = "fn main() i32 { 0 }\n";
    let ir =
        crate::driver::pipeline::compile_to_ll_with(src, crate::driver::pipeline::Backend::Qir)
            .expect("qir module");
    assert!(ir.contains("QIR backend"), "{ir}");
    assert!(ir.contains("define void @main() #0"), "{ir}");
    assert!(ir.contains("qir_major_version"), "{ir}");
    assert!(ir.contains("\"required_num_qubits\"=\"0\""), "{ir}");
}

#[test]
fn compile_to_ll_with_qir_runs_frontend_validation() {
    let src = "fn main() i32 { if 1 { return 0 } 0 }\n";
    let err =
        crate::driver::pipeline::compile_to_ll_with(src, crate::driver::pipeline::Backend::Qir)
            .expect_err("type error must surface even in qir mode");
    assert!(err.contains("type error"), "{err}");
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
    let err =
        crate::driver::pipeline::compile_to_ll(src).expect_err("quant fn requires qir backend");
    assert!(
        err.contains("quantum functions require the QIR backend"),
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
    let err = crate::driver::pipeline::compile_to_ll(src).expect_err("type error");
    assert!(err.contains("type error in function `main`"), "{err}");
    assert!(err.contains("--> line 3"), "{err}");
    assert!(err.contains("if 1 {"), "{err}");
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
/// Verifies that relative fixture paths are discoverable by path resolver logic.
fn resolve_input_path_works_for_existing_relative_fixture() {
    let p = std::path::PathBuf::from("examples/tests/ok_minimal.nia");
    let r = crate::driver::pipeline::resolve_input_path(p);
    assert!(r.is_ok(), "{r:?}");
}
