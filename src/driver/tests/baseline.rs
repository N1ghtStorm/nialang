//! Full pipeline baseline: every `ok_*.nia` fixture must compile to LLVM.

use crate::driver::fixtures::{
    all_tier_fixtures, discover_ok_fixtures, read_fixture, ERR_FIXTURES,
};

fn compile_fixture_ok(path: &str) {
    let src = read_fixture(path);
    crate::driver::pipeline::compile_to_ll(&src)
        .unwrap_or_else(|e| panic!("pipeline failed for `{path}`: {e}"));
}

#[test]
fn all_discovered_ok_fixtures_compile() {
    for path in discover_ok_fixtures() {
        compile_fixture_ok(&path);
    }
}

#[test]
fn tier_0_fixtures_compile() {
    for path in crate::driver::fixtures::TIER_0 {
        compile_fixture_ok(path);
    }
}

#[test]
fn tier_1_fixtures_compile() {
    for path in crate::driver::fixtures::TIER_1 {
        compile_fixture_ok(path);
    }
}

#[test]
fn tier_2_fixtures_compile() {
    for path in crate::driver::fixtures::TIER_2 {
        compile_fixture_ok(path);
    }
}

#[test]
fn tier_union_matches_discovered_ok_fixture_count() {
    assert_eq!(
        all_tier_fixtures().len(),
        discover_ok_fixtures().len(),
        "update TIER_0/1/2 when adding or removing ok fixtures"
    );
}

#[test]
fn known_invalid_fixtures_fail() {
    for path in ERR_FIXTURES {
        let src = read_fixture(path);
        let result = crate::driver::pipeline::compile_to_ll(&src);
        assert!(result.is_err(), "fixture unexpectedly compiled: {path}");
    }
}
