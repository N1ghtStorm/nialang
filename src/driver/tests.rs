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
        "examples/tests/ok_bitwise.nia",
        "examples/tests/ok_atomic_bool.nia",
        "examples/tests/ok_atomic_ptr.nia",
        "examples/tests/ok_atomic_int.nia",
        "examples/tests/ok_atomic_narrow_int.nia",
        "examples/tests/ok_threads_minimal.nia",
        "examples/sample_threads.nia",
        "examples/tests/ok_string.nia",
        "examples/sample_all.nia",
        "examples/sample_struct_methods_big.nia",
        "examples/sample_atomic_ptr.nia",
        "examples/sample_atomic_int.nia",
        "examples/sample_atomic_narrow_int.nia",
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
        "examples/abilities/copy_move_basics.nia",
        "examples/abilities/custom_clone.nia",
        "examples/abilities/custom_deref.nia",
        "examples/abilities/explicit_drop.nia",
        "examples/abilities/auto_drop_scope.nia",
        "examples/abilities/drop_flags.nia",
        "examples/abilities/aggregate_drop.nia",
        "examples/abilities/closure_captures.nia",
        "examples/abilities/primitive_abilities.nia",
        "examples/abilities/move_closure_captures.nia",
        "examples/abilities/function_value_abilities.nia",
        "examples/crypto/merkle_builtin.nia",
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
    assert!(ir.contains("@__quantum__rt__qubit_allocate"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__qis__swap__body(ptr"),
        "{ir}"
    );
}

#[test]
fn compile_iqft4_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/iqft4.nia");
    assert!(ir.contains("@__quantum__rt__qubit_allocate"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__qis__rz__body(double %"),
        "{ir}"
    );
}

#[test]
fn compile_qubit_read_example_to_qir() {
    let ir = compile_fixture_qir_ok("examples/quantum/qubit_read.nia");
    assert!(ir.contains("@__quantum__rt__qubit_allocate"), "{ir}");
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
fn compile_full_nialang_quantum_example_to_qir_runner_ir() {
    let ir = compile_fixture_qir_ok("examples/quantum/qir_full_nialang.nia");
    assert!(ir.contains("for.header"), "{ir}");
    assert!(ir.contains("if.then"), "{ir}");
    assert!(ir.contains("call void @__quantum__qis__x__body"), "{ir}");
    assert!(
        ir.contains("call void @__quantum__rt__int_record_output"),
        "{ir}"
    );
}

#[test]
fn compile_to_ll_with_qir_emits_qir_module() {
    let src = "fn main() i32 { 0 }\n";
    let ir =
        crate::driver::pipeline::compile_to_ll_with(src, crate::driver::pipeline::Backend::Qir)
            .expect("qir module");
    assert!(ir.contains("generated by nialang"), "{ir}");
    assert!(ir.contains("define i32 @main()"), "{ir}");
    assert!(!ir.contains("QIR backend"), "{ir}");
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
fn compile_inline_modules_with_rust_like_paths() {
    let src = r#"
fn root_value() i32 {
    2
}

pub mod math {
    pub struct Pair { value: i32 }

    pub enum Choice {
        Empty,
        Number(i32),
        Named { value: i32 },
    }

    pub fn add(a: i32, b: i32) i32 {
        a + b
    }

    fn via_self() i32 {
        self::add(20, 20)
    }

    fn via_crate() i32 {
        crate::root_value()
    }

    mod nested {
        fn forty_two() i32 {
            super::add(40, 2)
        }
    }
}

fn main() i32 {
    let p = math::Pair { value: math::nested::forty_two() };
    let c: math::Choice = math::Choice::Named { value: p.value };
    let v = match c {
        math::Choice::Empty => 0,
        math::Choice::Number(n) => n,
        math::Choice::Named { value } => value,
    };
    v + math::via_self() + math::via_crate()
}
"#;
    let ir = crate::driver::pipeline::compile_to_ll(src).expect("module pipeline");
    assert!(ir.contains("@math__nested__forty_two"), "{ir}");
    assert!(ir.contains("%struct.math__Pair"), "{ir}");
    assert!(ir.contains("%enum.math__Choice"), "{ir}");
}

#[test]
fn compile_file_modules_from_mod_declaration() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "nialang-file-modules-{}-{nonce}",
        std::process::id()
    ));
    let math_dir = root.join("math");
    std::fs::create_dir_all(&math_dir).unwrap();
    std::fs::write(
        root.join("main.nia"),
        r#"
pub mod math;

fn main() i32 {
    math::nested::forty_two()
}
"#,
    )
    .unwrap();
    std::fs::write(
        root.join("math.nia"),
        r#"
mod nested;

fn add(a: i32, b: i32) i32 {
    a + b
}
"#,
    )
    .unwrap();
    std::fs::write(
        math_dir.join("nested.nia"),
        r#"
fn forty_two() i32 {
    super::add(40, 2)
}
"#,
    )
    .unwrap();

    let ir =
        crate::driver::pipeline::compile_file_to_ll(&root.join("main.nia")).expect("file modules");
    assert!(ir.contains("@math__nested__forty_two"), "{ir}");

    let _ = std::fs::remove_dir_all(root);
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
    let err =
        crate::driver::pipeline::compile_to_ll(src).expect_err("quant block requires qir backend");
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
