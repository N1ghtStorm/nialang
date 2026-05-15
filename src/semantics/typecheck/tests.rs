use super::*;
use crate::{
    ast::VectorDef,
    parser::{tokenize, Parser},
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
        include_str!("../../../examples/tests/ok_impl_methods.nia"),
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
        include_str!("../../../examples/tests/ok_string.nia"),
        include_str!("../../../examples/sample_matrix_rc.nia"),
        include_str!("../../../examples/sample_matrix_arith.nia"),
        include_str!("../../../examples/sample_matrix_det.nia"),
        include_str!("../../../examples/sample_matrix_vector.nia"),
    ];
    for src in ok_files {
        let r = check_all(src);
        assert!(r.is_ok(), "{r:?}");
    }
}

#[test]
fn typecheck_rejects_unknown_method() {
    let src = r#"
struct Point { x: i32, y: i32 }

fn main() i32 {
    let p = Point { x: 2, y: 3 };
    p.missing()
}
"#;
    let err = check_all(src).expect_err("unknown method");
    assert!(err.contains("unknown method `missing`"), "{err}");
}

#[test]
fn typecheck_matrix_det_method_ok() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let d: i32 = m.det();
    d
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_det_method_rejects_args() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    m.det(1)
}
"#;
    let err = check_all(src).expect_err("det method args");
    assert!(err.contains("method `det`: expected 0 args"), "{err}");
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
fn typecheck_matrix_accepts_nested_numeric_arrays() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    matrix_drop(m);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_add_ok_same_cell_type() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [10, 20],
        [30, 40],
    ]);
    let c: Matrix = a + b;
    println(c);
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_add_rejects_different_cell_types() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: Matrix = a + b;
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_sub_ok_same_cell_type() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [10, 20],
        [30, 40],
    ]);
    let c: Matrix = a - b;
    println(c);
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_sub_rejects_different_cell_types() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: Matrix = a - b;
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_mul_ok_same_cell_type() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [10, 20],
        [30, 40],
    ]);
    let c: Matrix = a * b;
    println(c);
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_mul_rejects_different_cell_types() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: Matrix = a * b;
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_matmul_ok_same_cell_type() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2, 3],
        [4, 5, 6],
    ]);
    let b: Matrix = matrix([
        [7, 8],
        [9, 10],
        [11, 12],
    ]);
    let c: Matrix = a @ b;
    println(c);
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_matmul_rejects_different_cell_types() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: Matrix = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: Matrix = a @ b;
    matrix_drop(c);
    matrix_drop(b);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_vector_products_ok_named_and_anon() {
    let src = r#"
vector Vec2i i32 [X, Y]
vector Vec3i i32 [A, B, C]

fn main() i32 {
    let m: Matrix = matrix([
        [1, 2, 3],
        [4, 5, 6],
    ]);
    let v3 = Vec3i [A: 7, B: 8, C: 9];
    let v2 = Vec2i [X: 10, Y: 20];
    let mv_named: Vec2i = m @ v3;
    let vm_named: Vec3i = v2 @ m;
    println(mv_named);
    println(vm_named);
    println(m @ <7, 8, 9>);
    println(<10, 20> @ m);
    matrix_drop(m);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_vector_rejects_static_shape_mismatch() {
    let src = r#"
vector Vec3i i32 [A, B, C]

fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let v = Vec3i [A: 7, B: 8, C: 9];
    let out = m @ v;
    println(out);
    matrix_drop(m);
    0
}
"#;
    let err = check_all(src).expect_err("matrix-vector shape mismatch");
    assert!(err.contains("Matrix-vector shape mismatch"), "{err}");
}

#[test]
fn typecheck_outer_ok_same_numeric_element_type() {
    let src = r#"
vector Vec3i i32 [X, Y, Z]
vector Vec2i i32 [U, V]

fn main() i32 {
    let a = Vec3i [X: 1, Y: 2, Z: 3];
    let b = Vec2i [U: 4, V: 5];
    let c: Matrix = outer(a, b);
    println(c);
    matrix_drop(c);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_outer_rejects_different_element_types() {
    let src = r#"
vector Vec2i i32 [X, Y]
vector Vec2f f64 [X, Y]

fn main() i32 {
    let a = Vec2i [X: 1, Y: 2];
    let b = Vec2f [X: 1.0, Y: 2.0];
    let c: Matrix = outer(a, b);
    matrix_drop(c);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_outer_rejects_non_vector_argument() {
    let src = r#"
vector Vec2i i32 [X, Y]

fn main() i32 {
    let a = Vec2i [X: 1, Y: 2];
    let c: Matrix = outer(a, 3);
    matrix_drop(c);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_def_call_is_not_builtin() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    def(m)
}
"#;
    let err = check_all(src).expect_err("def is not a builtin");
    assert!(err.contains("unknown function `def`"), "{err}");
}

#[test]
fn typecheck_matrix_scalar_mul_ok_same_cell_type_both_orders() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let right: Matrix = a * 3;
    let left: Matrix = 2 * a;
    println(right);
    println(left);
    matrix_drop(left);
    matrix_drop(right);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_scalar_mul_float_ok() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let scaled: Matrix = a * 2.0;
    println(scaled);
    matrix_drop(scaled);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_matrix_scalar_mul_rejects_different_cell_type() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1, 2],
        [3, 4],
    ]);
    let scaled: Matrix = a * 2.0;
    matrix_drop(scaled);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_float_scalar_mul_rejects_int_literal() {
    let src = r#"
fn main() i32 {
    let a: Matrix = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let scaled: Matrix = a * 2;
    matrix_drop(scaled);
    matrix_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_rejects_mixed_numeric_cell_types() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3.5, 4.5],
    ]);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_rejects_int_literal_inside_float_matrix() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1.0, 2],
        [3.0, 4.0],
    ]);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_rejects_bool_cells() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, true],
        [3, 4],
    ]);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_matrix_rejects_ragged_rows() {
    let src = r#"
fn main() i32 {
    let m: Matrix = matrix([
        [1, 2],
        [3, 4, 5],
    ]);
    0
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
fn typecheck_vector_scalar_mul_ok_both_orders() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let v = V2 [X: 2, Y: 3];
    let a = v * 4;
    let b = 10 * v;
    a.X + a.Y + b.X + b.Y
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_vector_scalar_mul_wrong_scalar_ty_rejected() {
    let src = r#"
vector V2 i64 [ X, Y ]

fn main() i32 {
    let v = V2 [X: 1, Y: 2];
    let k: i32 = 4;
    v * k
}
"#;
    assert!(check_all(src).is_err());
}

#[test]
fn typecheck_vector_mul_ok_same_type() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 3, Y: 4];
    let p = u * v;
    p.X + p.Y
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_vector_mul_rejects_different_vector_types() {
    let src = r#"
vector A i32 [ X, Y ]
vector B i32 [ X, Y ]

fn main() i32 {
    let a = A [X: 1, Y: 2];
    let b = B [X: 3, Y: 4];
    a * b
}
"#;
    assert!(check_all(src).is_err());
}

#[test]
fn typecheck_vector_dot_ok_same_type() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 3, Y: 4];
    let d = u @ v;
    d
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_vector_dot_float_ok() {
    let src = r#"
vector Vf f64 [ X, Y ]

fn main() f64 {
    let u = Vf [X: 1.0, Y: 2.0];
    let v = Vf [X: 3.0, Y: 4.0];
    u @ v
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_vector_dot_rejects_non_vector_left() {
    let src = r#"
vector V2 i32 [ X, Y ]

fn main() i32 {
    let v = V2 [X: 1, Y: 2];
    3 @ v
}
"#;
    assert!(check_all(src).is_err());
}

#[test]
fn typecheck_vector_dot_rejects_different_vector_types() {
    let src = r#"
vector A i32 [ X, Y ]
vector B i32 [ X, Y ]

fn main() i32 {
    let a = A [X: 1, Y: 2];
    let b = B [X: 3, Y: 4];
    a @ b
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

#[test]
fn typecheck_anon_vector_arithmetic_ok() {
    let src = r#"
fn main() i32 {
    let a = <1, 2, 3>;
    let b = <4, 5, 6>;
    let sum = a + b;
    let diff = b - a;
    let prod = a * b;
    let scaled = a * 3;
    let left_scaled = 2 * b;
    let dot: i32 = a @ b;
    println(sum);
    println(diff);
    println(prod);
    println(scaled);
    println(left_scaled);
    dot
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_anon_vector_float_arithmetic_ok() {
    let src = r#"
fn main() i32 {
    let a = <1.0, 2.0, 3.0>;
    let b = <4.0, 5.0, 6.0>;
    println(a + b);
    println(a * 2.0);
    println(a @ b);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_anon_vector_rejects_different_lengths() {
    let src = r#"
fn main() i32 {
    let a = <1, 2, 3>;
    let b = <4, 5>;
    let c = a + b;
    println(c);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_anon_vector_rejects_different_element_types() {
    let src = r#"
fn main() i32 {
    let a = <1, 2, 3>;
    let b = <1.0, 2.0, 3.0>;
    let c = a * b;
    println(c);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_anon_vector_outer_ok() {
    let src = r#"
fn main() i32 {
    let m: Matrix = outer(<1, 2, 3>, <4, 5>);
    println(m);
    matrix_drop(m);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}
