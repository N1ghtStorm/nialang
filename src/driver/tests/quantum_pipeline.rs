//! Quantum examples via the new pipeline → QIR (phase 13).

use crate::driver::fixtures::{read_fixture, TIER_3};

#[test]
fn tier_3_quantum_examples_elaborate() {
    for path in TIER_3 {
        let src = read_fixture(path);
        let module = crate::driver::pipeline::parse_module(&src)
            .unwrap_or_else(|e| panic!("parse failed for `{path}`: {e}"));
        let resolved = crate::driver::pipeline::resolve_parsed_module(module)
            .unwrap_or_else(|e| panic!("resolve failed for `{path}`: {e}"));
        crate::driver::pipeline::elaborate_resolved_module(&resolved)
            .unwrap_or_else(|e| panic!("elab failed for `{path}`: {e}"));
    }
}

#[test]
fn tier_3_quantum_examples_emit_qir_from_resolved() {
    for path in TIER_3 {
        let src = read_fixture(path);
        let module = crate::driver::pipeline::parse_module(&src).expect("parse");
        let resolved = crate::driver::pipeline::resolve_parsed_module(module).expect("resolve");
        let ir = crate::backend::qir::emit_from_resolved(&resolved)
            .unwrap_or_else(|e| panic!("emit_from_resolved failed for `{path}`: {e}"));
        assert!(ir.contains("define void @main()"), "{path}: {ir}");
        assert!(ir.contains("qir_major_version"), "{path}: {ir}");
    }
}

#[test]
fn tier_3_quantum_examples_compile_via_new_qir_pipeline() {
    for path in TIER_3 {
        let src = read_fixture(path);
        let ir = crate::driver::pipeline::compile_new_to_qir(&src)
            .unwrap_or_else(|e| panic!("new QIR pipeline failed for `{path}`: {e}"));
        assert!(ir.contains("define void @main()"), "{path}: {ir}");
        assert!(ir.contains("qir_major_version"), "{path}: {ir}");
    }
}

#[test]
fn affine_qubit_errors_are_rejected() {
    for (path, needle) in [
        (
            "examples/tests/err_qubit_after_measure.nia",
            "already measured",
        ),
        ("examples/tests/err_qubit_copy.nia", "cannot copy qubit"),
    ] {
        let src = read_fixture(path);
        let module = crate::driver::pipeline::parse_module(&src).expect("parse");
        let resolved = crate::driver::pipeline::resolve_parsed_module(module).expect("resolve");
        let err = crate::driver::pipeline::elaborate_resolved_module(&resolved).unwrap_err();
        assert!(err.contains(needle), "{path}: {err}");
    }
}
