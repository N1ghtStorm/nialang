use super::*;
use crate::parser::{Parser, tokenize};
use crate::semantics::typecheck::{check_fn, collect_sigs};

fn emit(src: &str) -> String {
    let (structs, enums, fns, vectors) = Parser::new(tokenize(src)).parse_file().expect("parse");
    let (struct_map, enum_map, fn_sigs) = collect_sigs(&structs, &enums, &fns).expect("sigs");
    for f in &fns {
        check_fn(f, &struct_map, &enum_map, &fn_sigs).expect("typecheck");
    }
    emit_module(&structs, &enums, &vectors, &fns, &fn_sigs)
}

#[test]
fn codegen_contains_if_branching() {
    let ll = emit(include_str!("../../../examples/tests/ok_if_return.nia"));
    assert!(ll.contains("br i1"));
    assert!(ll.contains("if.then."));
}

#[test]
fn codegen_contains_tuple_struct_ops() {
    let ll = emit(include_str!("../../../examples/tests/ok_tuple_struct.nia"));
    assert!(ll.contains("insertvalue %struct.Foo"));
    assert!(ll.contains("extractvalue %struct.Foo"));
}

#[test]
fn codegen_contains_print_for_primitives() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_print_primitives.nia"
    ));
    assert!(ll.contains("@printf"));
    assert!(ll.contains("nialang.std.fmt"));
}

#[test]
fn codegen_pointer_load_present() {
    let ll = emit(include_str!("../../../examples/tests/ok_pointers.nia"));
    assert!(ll.contains("load i32, ptr"));
    assert!(ll.contains("define i32 @id_ptr"));
}

#[test]
fn codegen_named_struct_type_decl_present() {
    let ll = emit(include_str!("../../../examples/tests/ok_struct_named.nia"));
    assert!(ll.contains("%struct.Bar = type"));
}

#[test]
fn codegen_tuple_named_mix_contains_pair_and_boxed() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_tuple_named_mix.nia"
    ));
    assert!(ll.contains("%struct.Pair = type"));
    assert!(ll.contains("%struct.Boxed = type"));
    assert!(ll.contains("extractvalue %struct.Pair"));
}

#[test]
fn codegen_nested_if_has_multiple_labels() {
    let ll = emit(include_str!("../../../examples/tests/ok_nested_if.nia"));
    let count_then = ll.matches("if.then.").count();
    assert!(count_then >= 2, "IR:\n{ll}");
}

#[test]
fn codegen_contains_array_type_and_insertvalue() {
    let ll = emit(include_str!("../../../examples/tests/ok_array.nia"));
    assert!(ll.contains("[8 x i8]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [8 x i8]"), "IR:\n{ll}");
}

#[test]
fn codegen_contains_array_index_gep() {
    let ll = emit(include_str!("../../../examples/tests/ok_array_index.nia"));
    assert!(ll.contains("getelementptr inbounds [8 x i8]"), "IR:\n{ll}");
    assert!(ll.contains("load i8"), "IR:\n{ll}");
}

#[test]
fn codegen_array_index_store_emits_store() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_array_index_store.nia"
    ));
    assert!(ll.contains("getelementptr inbounds [3 x i8]"), "IR:\n{ll}");
    assert!(ll.contains("store i8"), "IR:\n{ll}");
}

#[test]
fn codegen_array_reverse_has_helper_and_swaps() {
    let ll = emit(include_str!("../../../examples/tests/ok_array_reverse.nia"));
    assert!(ll.contains("define [8 x i8] @reverse_array"), "IR:\n{ll}");
    assert!(ll.contains("getelementptr inbounds [8 x i8]"), "IR:\n{ll}");
    assert!(ll.matches("store i8").count() >= 3, "IR:\n{ll}");
}

#[test]
fn codegen_builtin_len_emits_array_length_constant() {
    let ll = emit(include_str!("../../../examples/tests/ok_array_len.nia"));
    assert!(ll.contains("ret i32 3"), "IR:\n{ll}");
}

#[test]
fn codegen_comparison_emits_icmp() {
    let src = r#"
fn main() i32 {
    let a: i32 = 3;
    let b: i32 = 4;
    if a < b {
        return 1
    }
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("icmp slt i32"), "IR:\n{ll}");
}

#[test]
fn codegen_println_array_uses_array_text_constants() {
    let ll = emit(include_str!("../../../examples/tests/ok_print_array.nia"));
    assert!(ll.contains("nialang.std.txt.arr_open"), "IR:\n{ll}");
    assert!(ll.contains("nialang.std.txt.arr_sep"), "IR:\n{ll}");
    assert!(ll.contains("nialang.std.txt.arr_close_ln"), "IR:\n{ll}");
}

#[test]
fn codegen_println_structs_and_ptr_hex() {
    let ll = emit(include_str!("../../../examples/tests/ok_print_structs.nia"));
    assert!(ll.contains("nialang.std.txt.obj_open"), "IR:\n{ll}");
    assert!(ll.contains("nialang.std.txt.tuple_open"), "IR:\n{ll}");
    assert!(ll.contains("nialang.std.fmt.ptrhex"), "IR:\n{ll}");
    assert!(ll.contains("ptrtoint ptr"), "IR:\n{ll}");
}

#[test]
fn codegen_alloc_realloc_dealloc_calls_present() {
    let ll = emit(include_str!("../../../examples/tests/ok_alloc_heap.nia"));
    assert!(ll.contains("call ptr @malloc"), "IR:\n{ll}");
    assert!(ll.contains("call ptr @realloc"), "IR:\n{ll}");
    assert!(ll.contains("call void @free"), "IR:\n{ll}");
}

#[test]
fn codegen_pointer_write_emits_store_through_ptr() {
    let ll = emit(include_str!("../../../examples/tests/ok_ptr_write.nia"));
    assert!(ll.contains("store i32 99, ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_pointer_array_write_emits_indexed_store_through_ptr() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_ptr_array_write.nia"
    ));
    assert!(
        ll.contains("getelementptr inbounds [4 x i8], ptr %"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("store i8 9, ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_readme_arrays_uses_comparisons_and_indexing() {
    let ll = emit(include_str!("../../../examples/tests/ok_readme_arrays.nia"));
    assert!(ll.contains("icmp"), "IR:\n{ll}");
    assert!(ll.contains("getelementptr inbounds [8 x i8]"), "IR:\n{ll}");
}

#[test]
fn codegen_enum_match_uses_switch() {
    let ll = emit(include_str!("../../../examples/tests/ok_enum_match.nia"));
    assert!(ll.contains("switch i32"), "IR:\n{ll}");
    assert!(ll.contains("match.arm."), "IR:\n{ll}");
}

#[test]
fn codegen_enum_payload_match_extracts_payloads() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_enum_payload_match.nia"
    ));
    assert!(ll.contains("%enum.Value = type"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %enum.Value"), "IR:\n{ll}");
}

#[test]
fn codegen_println_enum_emits_prefix_constants_and_switch() {
    let ll = emit(include_str!("../../../examples/tests/ok_print_enum.nia"));
    assert!(
        ll.contains("nialang.std.txt.enumprefix.Color.Red"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("nialang.std.txt.enumprefix.Msg.Point"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("println.enum.arm"), "IR:\n{ll}");
}

#[test]
fn codegen_for_range_emits_phi_and_latch() {
    let ll = emit(include_str!("../../../examples/tests/ok_for_range.nia"));
    assert!(ll.contains("for.pre."), "IR:\n{ll}");
    assert!(ll.contains("for.header."), "IR:\n{ll}");
    assert!(ll.contains("for.latch."), "IR:\n{ll}");
    assert!(ll.contains("phi i32"), "IR:\n{ll}");
}

#[test]
fn codegen_while_emits_cond_and_backedge() {
    let ll = emit(include_str!("../../../examples/tests/ok_while.nia"));
    assert!(ll.contains("while.cond."), "IR:\n{ll}");
    assert!(ll.contains("while.body."), "IR:\n{ll}");
    assert!(ll.contains("while.exit."), "IR:\n{ll}");
}

#[test]
fn codegen_loop_emits_iter_and_exit_labels() {
    let ll = emit(include_str!("../../../examples/tests/ok_loop.nia"));
    assert!(ll.contains("loop.iter."), "IR:\n{ll}");
    assert!(ll.contains("loop.exit."), "IR:\n{ll}");
}
