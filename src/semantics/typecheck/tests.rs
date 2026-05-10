use super::*;
use crate::{
    ast::VectorDef,
    parser::{Parser, tokenize},
};

fn parse(src: &str) -> (Vec<StructDef>, Vec<EnumDef>, Vec<FnDef>, Vec<VectorDef>) {
    Parser::new(tokenize(src))
        .parse_file()
        .expect("parse success")
}

fn check_all(src: &str) -> Result<(), String> {
    let (structs, enums, fns, vectors) = parse(src);
    let (struct_map, enum_map, vector_map, fn_sigs) =
        collect_sigs(&structs, &enums, &vectors, &fns)?;
    for f in &fns {
        check_fn(f, &struct_map, &enum_map, &vector_map, &fn_sigs)?;
    }
    Ok(())
}

#[test]
fn typecheck_ok_fixtures() {
    let ok_files = [
        include_str!("../../../examples/tests/ok_minimal.nia"),
        include_str!("../../../examples/tests/ok_if_return.nia"),
        include_str!("../../../examples/tests/ok_tuple_struct.nia"),
        include_str!("../../../examples/tests/ok_struct_named.nia"),
        include_str!("../../../examples/tests/ok_print_primitives.nia"),
        include_str!("../../../examples/tests/ok_pointers.nia"),
        include_str!("../../../examples/tests/ok_nested_if.nia"),
        include_str!("../../../examples/tests/ok_tuple_named_mix.nia"),
        include_str!("../../../examples/tests/ok_array.nia"),
        include_str!("../../../examples/tests/ok_array_index.nia"),
        include_str!("../../../examples/tests/ok_array_index_store.nia"),
        include_str!("../../../examples/tests/ok_array_reverse.nia"),
        include_str!("../../../examples/tests/ok_array_len.nia"),
        include_str!("../../../examples/tests/ok_print_array.nia"),
        include_str!("../../../examples/tests/ok_print_structs.nia"),
        include_str!("../../../examples/tests/ok_alloc_heap.nia"),
        include_str!("../../../examples/tests/ok_ptr_write.nia"),
        include_str!("../../../examples/tests/ok_ptr_array_write.nia"),
        include_str!("../../../examples/tests/ok_readme_arrays.nia"),
        include_str!("../../../examples/tests/ok_readme_enums.nia"),
        include_str!("../../../examples/tests/ok_readme_pointers.nia"),
        include_str!("../../../examples/tests/ok_enum_match.nia"),
        include_str!("../../../examples/tests/ok_enum_payload_match.nia"),
        include_str!("../../../examples/tests/ok_print_enum.nia"),
        include_str!("../../../examples/tests/ok_for_range.nia"),
        include_str!("../../../examples/tests/ok_while.nia"),
        include_str!("../../../examples/tests/ok_loop.nia"),
        include_str!("../../../examples/tests/ok_compound_assign.nia"),
        include_str!("../../../examples/tests/ok_floats.nia"),
    ];
    for src in ok_files {
        let r = check_all(src);
        assert!(r.is_ok(), "{r:?}");
    }
}

#[test]
fn typecheck_float_ops_and_comparisons_ok() {
    let src = r#"
fn main() i32 {
    let a: f64 = 1.0 + 2.0;
    let b: f64 = a * 3.0;
    let c: f64 = b / 2.0;
    let d: f64 = c - 1.0;
    let _e: bool = d < 5.0;
    let f: f64 = -d;
    let _g: bool = f == f;
    let h: f32 = 1.0;
    let _i: f32 = h + 2.0;
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_rejects_float_literal_binop_assigned_to_narrower_float() {
    let src = r#"
fn main() i32 {
    let x: f32 = 1.0 + 2.0;
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_add_f32_and_i32() {
    let src = r#"
fn main() i32 {
    let x: f32 = 1.0;
    let y: i32 = 2;
    x + y
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_detects_mismatch_fixture() {
    let src = include_str!("../../../examples/tests/err_type_mismatch.nia");
    let r = check_all(src);
    assert!(r.is_err());
}

#[test]
fn typecheck_detects_add_bool_fixture() {
    let src = include_str!("../../../examples/tests/err_type_add_bool.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_detects_if_non_bool_fixture() {
    let src = include_str!("../../../examples/tests/err_type_if_non_bool.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_detects_tuple_named_literal_fixture() {
    let src = include_str!("../../../examples/tests/err_type_tuple_with_named_literal.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_detects_array_len_mismatch_fixture() {
    let src = include_str!("../../../examples/tests/err_array_len_mismatch.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_shadowing_let_fixture() {
    let src = include_str!("../../../examples/tests/err_shadow_let.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_for_range_non_integer_fixture() {
    let src = include_str!("../../../examples/tests/err_for_range_bool.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_return_inside_for_fixture() {
    let src = include_str!("../../../examples/tests/err_for_return_in_for.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_while_cond_non_bool_fixture() {
    let src = include_str!("../../../examples/tests/err_while_cond_int.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_loop_without_break_fixture() {
    let src = include_str!("../../../examples/tests/err_loop_no_break.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_break_outside_loop_fixture() {
    let src = include_str!("../../../examples/tests/err_break_outside_loop.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_break_inside_while_fixture() {
    let src = include_str!("../../../examples/tests/err_break_in_while.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_div_by_zero_literal_fixture() {
    let src = include_str!("../../../examples/tests/err_div_by_zero.nia");
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_index_on_non_array() {
    let src = r#"
fn main() i32 {
let x: i32 = 1;
x[0]
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_wrong_tuple_arity() {
    let src = r#"
struct Foo (u8, i32)
fn main() i32 {
let f = Foo(1);
f.1
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_return_in_void_fn() {
    let src = r#"
fn f() {
return 1
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_len_non_array() {
    let src = r#"
fn main() i32 {
    len(1)
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_order_on_bool() {
    let src = r#"
fn main() i32 {
    if true < false {
        1
    }
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_rejects_deref_non_pointer() {
    let src = r#"
fn main() i32 {
let a: i32 = 1;
*a
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_vector_type_annotation_normalizes() {
    let src = r#"
vector Point i32 [
    X,
    Y,
    Z,
]

fn main() i32 {
    let p: Point = Point [X: 1, Y: 2, Z: 3];
    p.X
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_rejects_duplicate_type_name_struct_vector() {
    let src = r#"
struct Point { x: i32 }
vector Point i32 [ X, Y, Z ]
fn main() i32 { 0 }
"#;
    let (structs, enums, fns, vectors) = parse(src);
    let r = collect_sigs(&structs, &enums, &vectors, &fns);
    assert!(r.is_err());
}

#[test]
fn typecheck_vector_add_sub_ok_same_type() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 3, Y: 4];
    let s = u + v;
    let d = s - u;
    d.X + d.Y
}
"#;
    assert!(check_all(src).is_ok());
}

#[test]
fn typecheck_vector_mul_rejected() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 3, Y: 4];
    u * v
}
"#;
    assert!(check_all(src).is_err());
}

#[test]
fn typecheck_vector_add_rejects_different_vector_types() {
    let src = r#"
vector A i32 [ X, Y ]
vector B i32 [ X, Y ]

fn main() i32 {
    let a = A [X: 1, Y: 2];
    let b = B [X: 3, Y: 4];
    a + b
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "expected mismatch, got {r:?}");
}

#[test]
fn typecheck_vector_float_add_sub_ok() {
    let src = r#"
vector Vf f64 [ X, Y ]

fn main() i32 {
    let u = Vf [X: 1.0, Y: 2.0];
    let v = Vf [X: 3.0, Y: 4.0];
    let w = u + v;
    let _z = w - u;
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_vector_div_rejected() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 3, Y: 4];
    u / v
}
"#;
    assert!(check_all(src).is_err());
}
