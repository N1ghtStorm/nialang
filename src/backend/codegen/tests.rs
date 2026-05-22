use super::*;
use crate::parser::{Parser, tokenize};
use crate::semantics::typecheck::{check_fn, collect_sigs};

fn emit(src: &str) -> String {
    let (structs, enums, fns, vectors) = Parser::new(tokenize(src)).parse_file().expect("parse");
    let (struct_map, enum_map, vector_map, fn_sigs) =
        collect_sigs(&structs, &enums, &vectors, &fns).expect("sigs");
    for f in &fns {
        check_fn(f, &struct_map, &enum_map, &vector_map, &fn_sigs).expect("typecheck");
    }
    emit_module(&structs, &enums, &vectors, &fns, &fn_sigs)
}

#[test]
fn codegen_extern_fn_exports_c_abi_symbol() {
    let ll = emit(
        r#"
extern fn add(a: i32, b: i32) i32 {
    a + b
}

fn main() i32 {
    add(1, 2)
}
"#,
    );
    assert!(ll.contains("define i32 @add(i32 %a, i32 %b)"), "IR:\n{ll}");
    assert!(ll.contains("call i32 @add(i32 1, i32 2)"), "IR:\n{ll}");
}

#[test]
fn codegen_contains_if_branching() {
    let ll = emit(include_str!("../../../examples/tests/ok_if_return.nia"));
    assert!(ll.contains("br i1"));
    assert!(ll.contains("if.then."));
}

#[test]
fn codegen_quant_expression_emits_tail_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let x = 1;
    let y = quant {
        let local = 41;
        x + local
    };
    y
}
"#,
    );
    assert!(ll.contains("add nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("%local.addr = alloca i32"), "IR:\n{ll}");
}

#[test]
fn codegen_gpu_expression_emits_tail_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let x = 1;
    let y = gpu {
        let local = 41;
        x + local
    };
    y
}
"#,
    );
    assert!(ll.contains("add nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("%local.addr = alloca i32"), "IR:\n{ll}");
}

#[test]
fn codegen_impl_method_lowers_to_function_call() {
    let ll = emit(include_str!("../../../examples/tests/ok_impl_methods.nia"));
    assert!(
        ll.contains("define internal i32 @Point__sum(ptr %self)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("define internal i32 @Point__add(%struct.Point %self, i32 %n)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call i32 @Point__sum(ptr"), "IR:\n{ll}");
    assert!(
        ll.contains("call i32 @Point__add(%struct.Point"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_vector_scalar_mul_emits_mul_nsw() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let v = V2 [X: 2, Y: 3];
    let w = v * 4;
    w.X + w.Y
}
"#;
    let ll = emit(src);
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.V2"));
}

#[test]
fn codegen_vector_mul_emits_component_mul() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 2, Y: 3];
    let v = V2 [X: 4, Y: 5];
    let w = u * v;
    w.X + w.Y
}
"#;
    let ll = emit(src);
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.V2"));
    assert!(ll.contains("insertvalue %struct.V2"));
}

#[test]
fn codegen_vector_dot_emits_mul_and_add() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 2, Y: 3];
    let v = V2 [X: 4, Y: 5];
    u @ v
}
"#;
    let ll = emit(src);
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("add nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.V2"), "IR:\n{ll}");
}

#[test]
fn codegen_anon_vector_arithmetic_emits_array_aggregate_ops() {
    let src = r#"
fn main() i32 {
    let a = <1, 2, 3>;
    let b = <4, 5, 6>;
    let c = a + b;
    let d = c * b;
    let e = d * 2;
    let dot = e @ a;
    println(e);
    dot
}
"#;
    let ll = emit(src);
    assert!(ll.contains("[3 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [3 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue [3 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("add nsw i32"), "IR:\n{ll}");
}

#[test]
fn codegen_anon_vector_type_annotation_uses_element_hint() {
    let src = r#"
fn main() i32 {
    let a: i64<3> = <1, 2, 3>;
    println(a);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("[3 x i64]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [3 x i64]"), "IR:\n{ll}");
}

#[test]
fn codegen_println_anon_vectors_use_typed_brace_format() {
    let src = r#"
fn main() i32 {
    let stack: i32<3> = <1, 2, 3>;
    let heap: f64<> = <1.0, 4.2, 10.3>;
    println(stack);
    println(heap);
    vector_drop(heap);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("nialang.std.txt.anonvec.open.i32"), "IR:\n{ll}");
    assert!(ll.contains("nialang.std.txt.anonvec.open.f64"), "IR:\n{ll}");
    assert!(ll.contains("nialang.std.txt.anonvec.close_ln"), "IR:\n{ll}");
}

#[test]
fn codegen_heap_anon_vector_uses_rc_header_and_len() {
    let src = r#"
fn main() i32 {
    let v: f64<> = <1.0, 2.0, 3.0>;
    println(vector_len(v));
    println(len(v));
    println(vector_refcount(v));
    let shared: f64<> = vector_clone(v);
    vector_set(shared, 1, 9.0);
    println(vector_get(v, 1));
    println(v);
    vector_drop(shared);
    vector_drop(v);
    0
}
"#;
    let ll = emit(src);
    assert!(
        ll.contains("getelementptr inbounds { i64, ptr, i64 }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call ptr @malloc(i64 24)"), "IR:\n{ll}");
    assert!(ll.contains("heap.vector.drop.free"), "IR:\n{ll}");
    assert!(ll.contains("println.heap.vector.cond"), "IR:\n{ll}");
}

#[test]
fn codegen_heap_anon_vector_arithmetic_checks_lengths() {
    let src = r#"
fn main() i32 {
    let a: i32<> = <1, 2, 3>;
    let b: i32<> = <4, 5, 6>;
    let c: i32<> = a + b;
    let d: i32<> = c * 2;
    let dot: i32 = d @ b;
    println(dot);
    vector_drop(d);
    vector_drop(c);
    vector_drop(b);
    vector_drop(a);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("heap.vector.add.len.ok"), "IR:\n{ll}");
    assert!(ll.contains("heap.vector.scalar.mul.cond"), "IR:\n{ll}");
    assert!(ll.contains("heap.vector.dot.len.ok"), "IR:\n{ll}");
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
}

#[test]
fn codegen_outer_emits_matrix_allocation_and_products() {
    let src = r#"
vector V3 i32 [ X, Y, Z ]
vector V2 i32 [ U, V ]

fn main() i32 {
    let a = V3 [X: 1, Y: 2, Z: 3];
    let b = V2 [U: 4, V: 5];
    let c: i32[] = outer(a, b);
    println(c);
    matrix_drop(c);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("call ptr @malloc(i64 24)"), "IR:\n{ll}");
    assert!(ll.contains("store i64 3"), "IR:\n{ll}");
    assert!(ll.contains("store i64 2"), "IR:\n{ll}");
    assert!(ll.matches("mul nsw i32").count() >= 6, "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.V3"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.V2"), "IR:\n{ll}");
}

#[test]
fn codegen_contains_tuple_struct_ops() {
    let ll = emit(include_str!("../../../examples/tests/ok_tuple_struct.nia"));
    assert!(ll.contains("insertvalue %struct.Foo"));
    assert!(ll.contains("extractvalue %struct.Foo"));
}

#[test]
fn codegen_vector_add_emits_extract_and_component_add() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 10, Y: 20];
    let w = u + v;
    w.X + w.Y
}
"#;
    let ll = emit(src);
    assert!(ll.contains("extractvalue %struct.V2"));
    assert!(ll.contains("add nsw i32"));
    assert!(ll.contains("insertvalue %struct.V2"));
}

#[test]
fn codegen_vector_sub_emits_component_sub() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 10, Y: 20];
    let v = V2 [X: 1, Y: 2];
    let w = u - v;
    w.X + w.Y
}
"#;
    let ll = emit(src);
    assert!(ll.contains("sub nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.V2"));
}

#[test]
fn codegen_vector_float_add_emits_fadd() {
    let src = r#"
vector Vf f64 [ X, Y ]

fn main() i32 {
    let u = Vf [X: 1.0, Y: 2.0];
    let v = Vf [X: 3.0, Y: 4.0];
    let _w = u + v;
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("fadd double"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.Vf"));
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
    assert!(ll.contains("define internal i32 @id_ptr"));
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
    assert!(
        ll.contains("define internal [8 x i8] @reverse_array"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("getelementptr inbounds [8 x i8]"), "IR:\n{ll}");
    assert!(ll.matches("store i8").count() >= 3, "IR:\n{ll}");
}

#[test]
fn codegen_builtin_len_emits_array_length_constant() {
    let ll = emit(include_str!("../../../examples/tests/ok_array_len.nia"));
    assert!(ll.contains("ret i32 3"), "IR:\n{ll}");
}

#[test]
fn codegen_array_to_vec_reuses_array_aggregate_as_anon_vector() {
    let ll = emit(include_str!("../../../examples/tests/ok_array_to_vec.nia"));
    assert!(ll.contains("[4 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [4 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue [4 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
}

#[test]
fn codegen_vector_to_array_reuses_anon_vector_aggregate_as_array() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_vector_to_array.nia"
    ));
    assert!(ll.contains("[3 x double]"), "IR:\n{ll}");
    assert!(ll.contains("[3 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [3 x double]"), "IR:\n{ll}");
    assert!(ll.contains("getelementptr inbounds [3 x i32]"), "IR:\n{ll}");
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
fn codegen_float_arithmetic_emits_farith() {
    let src = r#"
fn main() i32 {
    let a: f64 = 1.0 + 2.0;
    let b: f64 = a * 3.0;
    let c: f64 = b / 2.0;
    let d: f64 = c - 1.0;
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("fadd double"), "IR:\n{ll}");
    assert!(ll.contains("fmul double"), "IR:\n{ll}");
    assert!(ll.contains("fdiv double"), "IR:\n{ll}");
    assert!(ll.contains("fsub double"), "IR:\n{ll}");
}

#[test]
fn codegen_float_comparison_emits_fcmp() {
    let src = r#"
fn main() i32 {
    let a: f64 = 1.0;
    let b: bool = a < 2.0;
    if b {
        1
    }
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("fcmp olt double"), "IR:\n{ll}");
}

#[test]
fn codegen_float_neg_emits_fneg() {
    let src = r#"
fn main() i32 {
    let a: f64 = 1.0;
    let b: f64 = -a;
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("fneg double"), "IR:\n{ll}");
}

#[test]
fn codegen_float_fixture_contains_half() {
    let ll = emit(include_str!("../../../examples/tests/ok_floats.nia"));
    assert!(ll.contains("half"), "IR:\n{ll}");
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
fn codegen_matrix_rc_helpers_present() {
    let ll = emit(include_str!("../../../examples/sample_matrix_rc.nia"));
    assert!(ll.contains("call ptr @malloc"), "IR:\n{ll}");
    assert!(ll.contains("println.matrix.row.cond"), "IR:\n{ll}");
    assert!(ll.contains("println.matrix.col.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.drop.free"), "IR:\n{ll}");
    assert!(
        ll.contains("getelementptr inbounds { i64, ptr, i64, i64 }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("getelementptr inbounds i32"), "IR:\n{ll}");
    assert!(ll.contains("load i32"), "IR:\n{ll}");
}

#[test]
fn codegen_matrix_arith_helpers_present() {
    let ll = emit(include_str!("../../../examples/sample_matrix_arith.nia"));
    assert!(ll.contains("matrix.add.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("matrix.add.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.sub.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("matrix.sub.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.mul.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("matrix.mul.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.scalar.mul.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.matmul.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("matrix.matmul.row.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.matmul.inner.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.det.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("matrix.det.lu.k.cond"), "IR:\n{ll}");
    assert!(ll.contains("sdiv i32"), "IR:\n{ll}");
    assert!(ll.contains("call void @abort()"), "IR:\n{ll}");
    assert!(ll.contains("add nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("sub nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("mul nsw i32"), "IR:\n{ll}");
    assert!(ll.contains("fadd double"), "IR:\n{ll}");
    assert!(ll.contains("fsub double"), "IR:\n{ll}");
    assert!(ll.contains("fmul double"), "IR:\n{ll}");
}

#[test]
fn codegen_matrix_vector_products_emit_shape_checks() {
    let ll = emit(include_str!("../../../examples/sample_matrix_vector.nia"));
    assert!(ll.contains("matrix.vector.matmul.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("vector.matrix.matmul.shape.ok"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.Vec3i"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.Vec2i"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue %struct.Vec2i"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue %struct.Vec3i"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [2 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [3 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("call void @abort()"), "IR:\n{ll}");
}

#[test]
fn codegen_float_matrix_det_uses_lu() {
    let src = r#"
fn main() f64 {
    let m: f64[] = matrix([
        [0.0, 1.0],
        [2.0, 3.0],
    ]);
    m.det()
}
"#;
    let ll = emit(src);
    assert!(ll.contains("matrix.det.lu.k.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.det.lu.find.cond"), "IR:\n{ll}");
    assert!(ll.contains("matrix.det.lu.swap.do"), "IR:\n{ll}");
    assert!(ll.contains("fdiv double"), "IR:\n{ll}");
    assert!(!ll.contains("matrix.det.heap.cond"), "IR:\n{ll}");
    assert!(!ll.contains("matrix.det.term.cond"), "IR:\n{ll}");
}

#[test]
fn codegen_int_matrix_det_method_uses_lu() {
    let src = r#"
fn main() i32 {
    let m: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    m.det()
}
"#;
    let ll = emit(src);
    assert!(ll.contains("matrix.det.lu.k.cond"), "IR:\n{ll}");
    assert!(ll.contains("sdiv i32"), "IR:\n{ll}");
    assert!(!ll.contains("matrix.det.heap.cond"), "IR:\n{ll}");
    assert!(!ll.contains("matrix.det.term.cond"), "IR:\n{ll}");
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
