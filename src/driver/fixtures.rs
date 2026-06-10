//! Fixture registry for the compiler rewrite baseline (phase 0).
//!
//! Tier lists mirror [docs/rewrite-tasks.md](../../docs/rewrite-tasks.md).

use std::fs;
use std::path::{Path, PathBuf};

/// Repository root for nialang (`CARGO_MANIFEST_DIR`).
pub fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Reads a fixture path relative to the crate manifest directory.
pub fn read_fixture(rel_path: &str) -> String {
    let path = manifest_dir().join(rel_path);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read fixture `{rel_path}`: {e}"))
}

/// Tier 0 — minimal language (parse → elab → Core check target).
pub const TIER_0: &[&str] = &[
    "examples/tests/ok_minimal.nia",
    "examples/tests/ok_if_return.nia",
    "examples/tests/ok_struct_named.nia",
    "examples/tests/ok_tuple_struct.nia",
    "examples/tests/ok_tuple_named_mix.nia",
    "examples/tests/ok_nested_if.nia",
    "examples/tests/ok_print_primitives.nia",
    "examples/tests/ok_enum_match.nia",
    "examples/tests/ok_enum_payload_match.nia",
    "examples/tests/ok_print_enum.nia",
];

/// Tier 1 — control flow, arrays, pointers (+ LLVM on the new pipeline later).
pub const TIER_1: &[&str] = &[
    "examples/tests/ok_for_range.nia",
    "examples/tests/ok_while.nia",
    "examples/tests/ok_loop.nia",
    "examples/tests/ok_array.nia",
    "examples/tests/ok_array_index.nia",
    "examples/tests/ok_array_len.nia",
    "examples/tests/ok_array_index_store.nia",
    "examples/tests/ok_array_reverse.nia",
    "examples/tests/ok_readme_arrays.nia",
    "examples/tests/ok_readme_enums.nia",
    "examples/tests/ok_pointers.nia",
    "examples/tests/ok_ptr_write.nia",
    "examples/tests/ok_ptr_array_write.nia",
    "examples/tests/ok_readme_pointers.nia",
    "examples/tests/ok_alloc_heap.nia",
    "examples/tests/ok_compound_assign.nia",
    "examples/tests/ok_bitwise.nia",
];

/// Tier 2 — floats, strings, impl, vectors, matrices, scopes.
pub const TIER_2: &[&str] = &[
    "examples/tests/ok_floats.nia",
    "examples/tests/ok_string.nia",
    "examples/tests/ok_impl_methods.nia",
    "examples/tests/ok_print_structs.nia",
    "examples/tests/ok_print_array.nia",
    "examples/tests/ok_vector_to_array.nia",
    "examples/tests/ok_array_to_vec.nia",
    "examples/tests/ok_array_matrix_conversions.nia",
    "examples/tests/ok_gpu_scope.nia",
    "examples/tests/ok_quant_scope.nia",
    "examples/tests/ok_safe_div.nia",
];

/// Tier 3 — quantum examples (new pipeline → QIR).
pub const TIER_3: &[&str] = &[
    "examples/quantum/qubit_create.nia",
    "examples/quantum/qubit_read.nia",
    "examples/quantum/qubit_manipulation.nia",
    "examples/quantum/qft4.nia",
    "examples/quantum/iqft4.nia",
    "examples/quantum/deutsch_jozsa_1bit.nia",
];

/// Known-invalid fixtures that must fail the default pipeline.
pub const ERR_FIXTURES: &[&str] = &[
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
    "examples/tests/err_io_in_tot.nia",
    "examples/tests/err_quant_fn_outside_scope.nia",
    "examples/tests/err_qubit_after_measure.nia",
    "examples/tests/err_qubit_copy.nia",
];

/// Discovers every `examples/tests/ok_*.nia` fixture on disk.
pub fn discover_ok_fixtures() -> Vec<String> {
    let dir = manifest_dir().join("examples/tests");
    let mut paths = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("failed to read `{}`: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| is_ok_fixture(path))
        .map(|path| {
            path.strip_prefix(manifest_dir())
                .expect("fixture under manifest dir")
                .to_string_lossy()
                .into_owned()
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn is_ok_fixture(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "nia")
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("ok_"))
}

/// Returns every tier-0/1/2 fixture path (deduplicated, sorted).
pub fn all_tier_fixtures() -> Vec<&'static str> {
    let mut out = Vec::new();
    out.extend_from_slice(TIER_0);
    out.extend_from_slice(TIER_1);
    out.extend_from_slice(TIER_2);
    out.sort_unstable();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_lists_cover_every_discovered_ok_fixture() {
        let discovered = discover_ok_fixtures();
        let tiered: std::collections::BTreeSet<_> = all_tier_fixtures().into_iter().collect();

        let missing: Vec<_> = discovered
            .iter()
            .filter(|path| !tiered.contains(path.as_str()))
            .cloned()
            .collect();
        assert!(
            missing.is_empty(),
            "add these ok fixtures to TIER_0/1/2 in driver/fixtures.rs: {missing:?}"
        );

        let extra: Vec<_> = tiered
            .iter()
            .filter(|path| !discovered.iter().any(|d| d == *path))
            .cloned()
            .collect();
        assert!(
            extra.is_empty(),
            "tier lists reference missing files: {extra:?}"
        );
    }
}
