//! Frontend-only baseline: lexer + parser, no semantics or codegen.

use crate::driver::fixtures::{read_fixture, TIER_0, TIER_1, TIER_2};

fn parse_fixture_ok(path: &str) {
    let src = read_fixture(path);
    crate::driver::pipeline::parse_module(&src)
        .unwrap_or_else(|e| panic!("parse failed for `{path}`: {e}"));
}

#[test]
fn tier_0_fixtures_parse() {
    for path in TIER_0 {
        parse_fixture_ok(path);
    }
}

#[test]
fn tier_1_fixtures_parse() {
    for path in TIER_1 {
        parse_fixture_ok(path);
    }
}

#[test]
fn tier_2_fixtures_parse() {
    for path in TIER_2 {
        parse_fixture_ok(path);
    }
}

#[test]
fn core_only_output_contains_surface_sections() {
    let src = read_fixture("examples/tests/ok_minimal.nia");
    let module = crate::driver::pipeline::parse_module(&src).expect("parse");
    let rendered = crate::driver::pipeline::format_surface_ast(&module);
    assert!(rendered.contains(";; nialang surface ast"));
    assert!(rendered.contains(";; functions"));
}
