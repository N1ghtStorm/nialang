/// Integration-style tests: full compile pipeline and path resolution.

fn compile_fixture_ok(path: &str) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let src = std::fs::read_to_string(&p).unwrap();
    crate::driver::pipeline::compile_to_ll(&src).expect("full pipeline");
}

#[test]
/// Ensures a representative set of valid language fixtures still compiles end-to-end.
fn compile_fixtures_pipeline() {
    let files = [
        "examples/tests/ok_minimal.nia",
        "examples/tests/ok_if_return.nia",
        "examples/tests/ok_tuple_struct.nia",
        "examples/tests/ok_struct_named.nia",
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
        "examples/sample_matrix_rc.nia",
        "examples/sample_matrix_arith.nia",
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
