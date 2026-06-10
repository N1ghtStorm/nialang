//! Core-check baseline: parse → resolve → elaborate → Core checker.

use crate::driver::fixtures::{read_fixture, TIER_0, TIER_1, TIER_2};

fn core_check_fixture_ok(path: &str) {
    let src = read_fixture(path);
    let module = crate::driver::pipeline::parse_module(&src)
        .unwrap_or_else(|e| panic!("parse failed for `{path}`: {e}"));
    let resolved = crate::driver::pipeline::resolve_parsed_module(module)
        .unwrap_or_else(|e| panic!("resolve failed for `{path}`: {e}"));
    crate::driver::pipeline::elaborate_resolved_module(&resolved)
        .unwrap_or_else(|e| panic!("core check failed for `{path}`: {e}"));
}

#[test]
fn tier_0_fixtures_core_check() {
    for path in TIER_0 {
        core_check_fixture_ok(path);
    }
}

#[test]
fn tier_1_fixtures_core_check() {
    for path in TIER_1 {
        core_check_fixture_ok(path);
    }
}

#[test]
fn tier_2_fixtures_core_check() {
    for path in TIER_2 {
        core_check_fixture_ok(path);
    }
}
