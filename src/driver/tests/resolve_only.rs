//! Name-resolution baseline for tier fixtures.

use crate::driver::fixtures::{read_fixture, TIER_0, TIER_1, TIER_2};

fn resolve_fixture_ok(path: &str) {
    let src = read_fixture(path);
    let module = crate::driver::pipeline::parse_module(&src)
        .unwrap_or_else(|e| panic!("parse failed for `{path}`: {e}"));
    crate::driver::pipeline::resolve_parsed_module(module)
        .unwrap_or_else(|e| panic!("resolve failed for `{path}`: {e}"));
}

#[test]
fn tier_0_fixtures_resolve() {
    for path in TIER_0 {
        resolve_fixture_ok(path);
    }
}

#[test]
fn tier_1_fixtures_resolve() {
    for path in TIER_1 {
        resolve_fixture_ok(path);
    }
}

#[test]
fn tier_2_fixtures_resolve() {
    for path in TIER_2 {
        resolve_fixture_ok(path);
    }
}

#[test]
fn resolve_only_output_lists_def_ids() {
    let src = read_fixture("examples/tests/ok_struct_named.nia");
    let module = crate::driver::pipeline::parse_module(&src).expect("parse");
    let resolved = crate::driver::pipeline::resolve_parsed_module(module).expect("resolve");
    let rendered = crate::driver::pipeline::format_resolved_ast(&resolved);
    assert!(rendered.contains(";; nialang resolved module"));
    assert!(rendered.contains("DefId("));
}
