mod ast;
mod backend;
mod codegen;
mod driver;
mod frontend;
mod lexer;
mod nia_std;
mod parser;
mod semantics;
mod typecheck;

/// Entrypoint of the compiler executable.
///
/// Delegates all real CLI work to `driver::pipeline::run_cli`, then mirrors the
/// compiled program exit code back to the operating system. Any compilation/runtime
/// failure is printed to stderr and returned as process exit code `1`.
fn main() {
    match driver::pipeline::run_cli() {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod self_test {
    /// Helper used by integration-like unit tests:
    /// reads a fixture file and runs the full compile pipeline on it.
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
            "examples/tests/ok_pointers.nia",
            "examples/tests/ok_nested_if.nia",
            "examples/tests/ok_tuple_named_mix.nia",
            "examples/tests/ok_array.nia",
            "examples/tests/ok_array_index.nia",
            "examples/tests/ok_array_index_store.nia",
            "examples/tests/ok_alloc_heap.nia",
            "examples/tests/ok_ptr_write.nia",
            "examples/tests/ok_enum_match.nia",
            "examples/tests/ok_enum_payload_match.nia",
            "examples/tests/ok_print_enum.nia",
            "examples/tests/ok_for_range.nia",
            "examples/tests/ok_while.nia",
            "examples/tests/ok_loop.nia",
            "examples/tests/ok_compound_assign.nia",
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
    /// Verifies that relative fixture paths are discoverable by path resolver logic.
    fn resolve_input_path_works_for_existing_relative_fixture() {
        let p = std::path::PathBuf::from("examples/tests/ok_minimal.nia");
        let r = crate::driver::pipeline::resolve_input_path(p);
        assert!(r.is_ok(), "{r:?}");
    }
}
