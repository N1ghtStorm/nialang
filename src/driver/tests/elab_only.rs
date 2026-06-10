//! Elaboration + Core check baseline for tier-0 fixtures.

use crate::driver::fixtures::{read_fixture, TIER_0};

fn elab_fixture_ok(path: &str) {
    let src = read_fixture(path);
    let module = crate::driver::pipeline::parse_module(&src)
        .unwrap_or_else(|e| panic!("parse failed for `{path}`: {e}"));
    let resolved = crate::driver::pipeline::resolve_parsed_module(module)
        .unwrap_or_else(|e| panic!("resolve failed for `{path}`: {e}"));
    let elaborated = crate::driver::pipeline::elaborate_resolved_module(&resolved)
        .unwrap_or_else(|e| panic!("elab failed for `{path}`: {e}"));
    let rendered = crate::driver::pipeline::format_elaborated_ast(&elaborated);
    assert!(rendered.contains(";; nialang elaborated core"));
    assert!(rendered.contains("fn main"));
}

#[test]
fn tier_0_fixtures_elaborate_and_check() {
    for path in TIER_0 {
        elab_fixture_ok(path);
    }
}

#[test]
fn ambiguous_implicit_reports_source_line() {
    let src = read_fixture("examples/tests/core/err_implicit_ambiguous.nia");
    let module = crate::driver::pipeline::parse_module(&src).expect("parse");
    let resolved = crate::driver::pipeline::resolve_parsed_module(module).expect("resolve");
    let err = crate::driver::pipeline::elaborate_resolved_module(&resolved).unwrap_err();
    let err = crate::driver::pipeline::format_elab_diagnostic(&src, &err);
    assert!(
        err.contains("ambiguous implicit argument `a`"),
        "{err}"
    );
    assert!(err.contains("line 6"), "expected source line in diagnostic: {err}");
    assert!(err.contains("pair(1, true)"), "{err}");
}
