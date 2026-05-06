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
    fn compile_fixture_ok(path: &str) {
        let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let src = std::fs::read_to_string(&p).unwrap();
        crate::driver::pipeline::compile_to_ll(&src).expect("full pipeline");
    }

    #[test]
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
        ];
        for f in files {
            compile_fixture_ok(f);
        }
    }

    #[test]
    fn compile_multiple_error_fixtures() {
        let err_files = [
            "examples/tests/err_type_mismatch.nia",
            "examples/tests/err_type_add_bool.nia",
            "examples/tests/err_type_if_non_bool.nia",
            "examples/tests/err_type_tuple_with_named_literal.nia",
        ];
        for f in err_files {
            let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(f);
            let src = std::fs::read_to_string(&p).unwrap();
            let r = crate::driver::pipeline::compile_to_ll(&src);
            assert!(r.is_err(), "fixture unexpectedly compiled: {f}");
        }
    }

    #[test]
    fn resolve_input_path_works_for_existing_relative_fixture() {
        let p = std::path::PathBuf::from("examples/tests/ok_minimal.nia");
        let r = crate::driver::pipeline::resolve_input_path(p);
        assert!(r.is_ok(), "{r:?}");
    }
}
