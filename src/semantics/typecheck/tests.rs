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
        include_str!("../../../examples/tests/ok_impl_methods.nia"),
        include_str!("../../../examples/tests/ok_quant_scope.nia"),
        include_str!("../../../examples/tests/ok_gpu_scope.nia"),
        include_str!("../../../examples/tests/ok_print_primitives.nia"),
        include_str!("../../../examples/tests/ok_pointers.nia"),
        include_str!("../../../examples/tests/ok_nested_if.nia"),
        include_str!("../../../examples/tests/ok_tuple_named_mix.nia"),
        include_str!("../../../examples/tests/ok_array.nia"),
        include_str!("../../../examples/tests/ok_array_index.nia"),
        include_str!("../../../examples/tests/ok_array_index_store.nia"),
        include_str!("../../../examples/tests/ok_array_reverse.nia"),
        include_str!("../../../examples/tests/ok_array_len.nia"),
        include_str!("../../../examples/tests/ok_array_to_vec.nia"),
        include_str!("../../../examples/tests/ok_vector_to_array.nia"),
        include_str!("../../../examples/tests/ok_array_matrix_conversions.nia"),
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
        include_str!("../../../examples/sample_struct_methods_big.nia"),
        include_str!("../../../examples/sample_matrix_rc.nia"),
        include_str!("../../../examples/sample_matrix_arith.nia"),
        include_str!("../../../examples/sample_matrix_det.nia"),
        include_str!("../../../examples/sample_matrix_vector.nia"),
        include_str!("../../../examples/sample_list.nia"),
        include_str!("../../../examples/sample_dft_list.nia"),
    ];
    for src in ok_files {
        let r = check_all(src);
        assert!(r.is_ok(), "{r:?}");
    }
}

#[test]
fn typecheck_extern_fn_allows_c_abi_scalars_and_pointers() {
    let src = r#"
extern fn add(a: i32, b: i32) i32 {
    a + b
}

extern fn store(p: &i32, v: i32) {
    *p = v;
}

fn main() i32 {
    add(1, 2)
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_extern_fn_rejects_non_c_abi_param_type() {
    let src = r#"
struct Pair { x: i32, y: i32 }

extern fn bad(p: Pair) i32 {
    p.x
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("non-C-ABI extern param");
    assert!(err.contains("non-C-ABI type"), "{err}");
}

#[test]
fn typecheck_extern_fn_rejects_non_c_abi_return_type() {
    let src = r#"
extern fn bad() i32<3> {
    <1, 2, 3>
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("non-C-ABI extern return");
    assert!(err.contains("return type is non-C-ABI"), "{err}");
}

#[test]
fn typecheck_complex_std_surface() {
    let src = r#"
fn main() f64 {
    let z: Complex = complex(1.0, 2.0);
    let w = Complex { re: 3, im: 4 };
    let sum = complex_add(z, w);
    let product = complex_mul(sum, cis(PI));
    let scaled = complex_scale(product, 0.5);
    let ratio = complex_div(scaled, complex(1.0, -1.0));
    sin(PI) + cos(0.0) + ratio.re + complex_sub(sum, z).im
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_list_surface() {
    let src = r#"
fn main() i32 {
    let bytes: List[u8] = list_new[u8]();
    bytes.push(10);
    bytes.push(20);
    let first: u8 = bytes.get(0);
    println(first);

    let zs = list_with_capacity[Complex](2);
    zs.push(complex(1.0, 0.0));
    zs.push(cis(PI));
    let z: Complex = zs.get(1);
    println(z);

    bytes.len() + bytes.capacity() + zs.len() + zs.capacity()
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_list_rejects_missing_type_arg() {
    let src = r#"
fn main() i32 {
    let xs = list_new();
    xs.len()
}
"#;
    let err = check_all(src).expect_err("list_new requires a type argument");
    assert!(err.contains("requires a type argument"), "{err}");
}

#[test]
fn typecheck_list_rejects_wrong_push_type() {
    let src = r#"
fn main() i32 {
    let xs = list_new[u8]();
    xs.push(true);
    xs.len()
}
"#;
    let err = check_all(src).expect_err("push type mismatch");
    assert!(err.contains("cannot satisfy"), "{err}");
}

#[test]
fn typecheck_list_rejects_wrong_get_index_type() {
    let src = r#"
fn main() i32 {
    let xs = list_new[u8]();
    xs.get(true)
}
"#;
    let err = check_all(src).expect_err("get index type mismatch");
    assert!(err.contains("bool literal cannot satisfy I32"), "{err}");
}

#[test]
fn typecheck_list_methods_reject_pointer_receivers() {
    let src = r#"
fn main() i32 {
    let xs = list_new[u8]();
    let p = &xs;
    p.len()
}
"#;
    let err = check_all(src).expect_err("list methods only accept List[T]");
    assert!(err.contains("unknown method `len`"), "{err}");
}

#[test]
fn typecheck_quant_scope_does_not_leak_bindings() {
    let src = r#"
fn main() i32 {
    quant {
        let hidden = 1;
        println(hidden);
    }
    hidden
}
"#;
    let err = check_all(src).expect_err("quant-local binding must not leak");
    assert!(err.contains("unknown variable `hidden`"), "{err}");
}

#[test]
fn typecheck_quant_expression_uses_tail_type_and_scoped_bindings() {
    let src = r#"
fn main() i32 {
    let x = 1;
    let y = quant {
        let local = 41;
        x + local
    };
    y
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_allows_qubit_creation_inside_quant() {
    let src = r#"
fn main() i32 {
    quant {
        let a = qubit();
        let b: qubit = qubit();
        H(a);
        CNOT(a, b);
        let ar = q_measure(a);
        let br: result = q_measure(b);
        q_record(ar);
        q_record(br);
    }
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_allows_quant_fn_called_inside_quant() {
    let src = r#"
quant fn prepare() {
    let q = qubit();
    H(q);
    let r = q_measure(q);
    q_record(r);
}

fn main() i32 {
    quant {
        prepare();
    }
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_rejects_quant_fn_call_outside_quant() {
    let src = r#"
quant fn prepare() {
    let q = qubit();
    H(q);
}

fn main() i32 {
    prepare();
    0
}
"#;
    let err = check_all(src).expect_err("quant fn call must be quant-only");
    assert!(
        err.contains("quantum function `prepare` can only be called inside `quant`"),
        "{err}"
    );
}

#[test]
fn typecheck_allows_quant_fn_qubit_parameter() {
    let src = r#"
quant fn prepare(q: qubit) {
    H(q);
}

fn main() i32 {
    quant {
        let q = qubit();
        prepare(q);
    }
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_rejects_h_outside_quant() {
    let src = r#"
fn main() i32 {
    H(0);
    0
}
"#;
    let err = check_all(src).expect_err("H must be quant-only");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_rejects_h_non_qubit_argument() {
    let src = r#"
fn main() i32 {
    quant {
        H(0);
    }
    0
}
"#;
    let err = check_all(src).expect_err("H expects a qubit");
    assert!(err.contains("cannot satisfy Qubit"), "{err}");
}

#[test]
fn typecheck_rejects_x_outside_quant() {
    let src = r#"
fn main() i32 {
    X(0);
    0
}
"#;
    let err = check_all(src).expect_err("X must be quant-only");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_rejects_x_non_qubit_argument() {
    let src = r#"
fn main() i32 {
    quant {
        X(0);
    }
    0
}
"#;
    let err = check_all(src).expect_err("X expects a qubit");
    assert!(err.contains("cannot satisfy Qubit"), "{err}");
}

#[test]
fn typecheck_rejects_new_single_qubit_gates_outside_quant() {
    for gate in ["Y", "Z", "S", "T"] {
        let src = format!(
            r#"
fn main() i32 {{
    {gate}(0);
    0
}}
"#
        );
        let err = check_all(&src).expect_err("single-qubit gate must be quant-only");
        assert!(err.contains("only allowed inside `quant`"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_new_single_qubit_gates_non_qubit_argument() {
    for gate in ["Y", "Z", "S", "T"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        {gate}(0);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("single-qubit gate expects a qubit");
        assert!(err.contains("cannot satisfy Qubit"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_two_qubit_gates_outside_quant() {
    for gate in ["CNOT", "CZ", "SWAP"] {
        let src = format!(
            r#"
fn main() i32 {{
    {gate}(0, 1);
    0
}}
"#
        );
        let err = check_all(&src).expect_err("two-qubit gate must be quant-only");
        assert!(err.contains("only allowed inside `quant`"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_two_qubit_gates_non_qubit_argument() {
    for gate in ["CNOT", "CZ", "SWAP"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(q, 0);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("two-qubit gate expects qubits");
        assert!(err.contains("cannot satisfy Qubit"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_two_qubit_gates_wrong_arity() {
    for gate in ["CNOT", "CZ", "SWAP"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(q);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("two-qubit gate expects two arguments");
        assert!(err.contains("expects exactly 2 arguments"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_rotation_gates_outside_quant() {
    for gate in ["Rx", "Ry", "Rz", "R1"] {
        let src = format!(
            r#"
fn main() i32 {{
    {gate}(1.0, 0);
    0
}}
"#
        );
        let err = check_all(&src).expect_err("rotation gate must be quant-only");
        assert!(err.contains("only allowed inside `quant`"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_rotation_gates_non_f64_angle() {
    for gate in ["Rx", "Ry", "Rz", "R1"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        let theta: i32 = 1;
        {gate}(theta, q);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("rotation gate expects an f64 angle");
        assert!(err.contains("expects an f64 angle"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_rotation_gates_non_qubit_argument() {
    for gate in ["Rx", "Ry", "Rz", "R1"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        {gate}(1.0, 0);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("rotation gate expects a qubit");
        assert!(err.contains("cannot satisfy Qubit"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_rotation_gates_wrong_arity() {
    for gate in ["Rx", "Ry", "Rz", "R1"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(1.0, q, q);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("rotation gate expects two arguments");
        assert!(err.contains("expects exactly 2 arguments"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_q_measure_outside_quant() {
    let src = r#"
fn main() i32 {
    q_measure(0);
    0
}
"#;
    let err = check_all(src).expect_err("q_measure must be quant-only");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_rejects_q_measure_non_qubit_argument() {
    let src = r#"
fn main() i32 {
    quant {
        q_measure(0);
    }
    0
}
"#;
    let err = check_all(src).expect_err("q_measure expects a qubit");
    assert!(err.contains("cannot satisfy Qubit"), "{err}");
}

#[test]
fn typecheck_rejects_q_record_outside_quant() {
    let src = r#"
fn main() i32 {
    q_record(0);
    0
}
"#;
    let err = check_all(src).expect_err("q_record must be quant-only");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_rejects_q_record_non_result_argument() {
    let src = r#"
fn main() i32 {
    quant {
        let q = qubit();
        q_record(q);
    }
    0
}
"#;
    let err = check_all(src).expect_err("q_record expects a result");
    assert!(err.contains("expects a result argument"), "{err}");
}

#[test]
fn typecheck_rejects_qubit_creation_outside_quant() {
    let src = r#"
fn main() i32 {
    let q = qubit();
    0
}
"#;
    let err = check_all(src).expect_err("qubit() must be quant-only");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_rejects_qubit_creation_inside_gpu() {
    let src = r#"
fn main() i32 {
    gpu {
        let q = qubit();
    }
    0
}
"#;
    let err = check_all(src).expect_err("gpu is not a quantum scope");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_rejects_qubit_type_annotation_outside_quant() {
    let src = r#"
fn main() i32 {
    let q: qubit = 0;
    0
}
"#;
    let err = check_all(src).expect_err("qubit type annotation must be quant-only");
    assert!(err.contains("cannot use quantum types"), "{err}");
}

#[test]
fn typecheck_rejects_result_type_annotation_outside_quant() {
    let src = r#"
fn main() i32 {
    let r: result = 0;
    0
}
"#;
    let err = check_all(src).expect_err("result type annotation must be quant-only");
    assert!(err.contains("cannot use quantum types"), "{err}");
}

#[test]
fn typecheck_rejects_qubit_escape_from_quant_expression() {
    let src = r#"
fn main() i32 {
    let q = quant {
        qubit()
    };
    0
}
"#;
    let err = check_all(src).expect_err("qubit must not escape quant expression");
    assert!(err.contains("cannot return quantum type `qubit`"), "{err}");
}

#[test]
fn typecheck_reserves_qubit_function_name() {
    let src = r#"
fn qubit() i32 {
    1
}

fn main() i32 {
    qubit()
}
"#;
    let err = check_all(src).expect_err("qubit is a reserved builtin name");
    assert!(err.contains("function name `qubit` is reserved"), "{err}");
}

#[test]
fn typecheck_reserves_h_function_name() {
    let src = r#"
fn H() i32 {
    1
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("H is a reserved builtin name");
    assert!(err.contains("function name `H` is reserved"), "{err}");
}

#[test]
fn typecheck_reserves_x_function_name() {
    let src = r#"
fn X() i32 {
    1
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("X is a reserved builtin name");
    assert!(err.contains("function name `X` is reserved"), "{err}");
}

#[test]
fn typecheck_reserves_new_single_qubit_gate_function_names() {
    for gate in ["Y", "Z", "S", "T"] {
        let src = format!(
            r#"
fn {gate}() i32 {{
    1
}}

fn main() i32 {{
    0
}}
"#
        );
        let err = check_all(&src).expect_err("single-qubit gate is a reserved builtin name");
        assert!(
            err.contains(&format!("function name `{gate}` is reserved")),
            "{gate}: {err}"
        );
    }
}

#[test]
fn typecheck_reserves_two_qubit_gate_function_names() {
    for gate in ["CNOT", "CZ", "SWAP"] {
        let src = format!(
            r#"
fn {gate}() i32 {{
    1
}}

fn main() i32 {{
    0
}}
"#
        );
        let err = check_all(&src).expect_err("two-qubit gate is a reserved builtin name");
        assert!(
            err.contains(&format!("function name `{gate}` is reserved")),
            "{gate}: {err}"
        );
    }
}

#[test]
fn typecheck_reserves_rotation_gate_function_names() {
    for gate in ["Rx", "Ry", "Rz", "R1"] {
        let src = format!(
            r#"
fn {gate}() i32 {{
    1
}}

fn main() i32 {{
    0
}}
"#
        );
        let err = check_all(&src).expect_err("rotation gate is a reserved builtin name");
        assert!(
            err.contains(&format!("function name `{gate}` is reserved")),
            "{gate}: {err}"
        );
    }
}

#[test]
fn typecheck_reserves_q_measure_function_name() {
    let src = r#"
fn q_measure() i32 {
    1
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("q_measure is a reserved builtin name");
    assert!(
        err.contains("function name `q_measure` is reserved"),
        "{err}"
    );
}

#[test]
fn typecheck_reserves_q_record_function_name() {
    let src = r#"
fn q_record() i32 {
    1
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("q_record is a reserved builtin name");
    assert!(
        err.contains("function name `q_record` is reserved"),
        "{err}"
    );
}

#[test]
fn typecheck_gpu_scope_and_expression_match_quant_behavior() {
    let src = r#"
fn main() i32 {
    let x = 1;
    gpu {
        let hidden = x + 1;
        println(hidden);
    }
    let y = gpu {
        let local = 41;
        x + local
    };
    y
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
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
    let m: i32[] = matrix([
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
    let m: i32[] = matrix([
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
fn typecheck_array_to_vec_method_ok() {
    let src = r#"
fn main() i32 {
    let ints: i32<4> = [1, 2, 3, 4].to_vec();
    let floats: f32<2> = [1.0, 2.0].to_vec();
    println(ints);
    println(floats);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_array_to_vec_rejects_non_numeric_elements() {
    let src = r#"
fn main() i32 {
    let xs = [true, false].to_vec();
    0
}
"#;
    let err = check_all(src).expect_err("non-numeric to_vec");
    assert!(
        err.contains("method `to_vec` array elements must be numeric"),
        "{err}"
    );
}

#[test]
fn typecheck_array_to_vec_rejects_args() {
    let src = r#"
fn main() i32 {
    [1, 2, 3].to_vec(1)
}
"#;
    let err = check_all(src).expect_err("to_vec args");
    assert!(err.contains("method `to_vec`: expected 0 args"), "{err}");
}

#[test]
fn typecheck_vector_to_array_method_ok() {
    let src = r#"
fn main() i32 {
    let floats: [f64; 3] = <1.0, 2.0, 3.0>.to_array();
    let ints: [i32; 4] = <1, 2, 3, 4>.to_array();
    println(floats);
    println(ints);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_vector_to_array_rejects_non_numeric_elements() {
    let src = r#"
fn main() i32 {
    let xs = <true, false>.to_array();
    0
}
"#;
    let err = check_all(src).expect_err("non-numeric to_array");
    assert!(
        err.contains("method `to_array` vector elements must be numeric"),
        "{err}"
    );
}

#[test]
fn typecheck_vector_to_array_rejects_heap_vectors() {
    let src = r#"
fn main() i32 {
    let xs: i32<> = <1, 2, 3>;
    xs.to_array()
}
"#;
    let err = check_all(src).expect_err("heap to_array");
    assert!(
        err.contains("method `to_array` is only supported for fixed-size anonymous vectors"),
        "{err}"
    );
}

#[test]
fn typecheck_vector_to_array_rejects_args() {
    let src = r#"
fn main() i32 {
    <1, 2, 3>.to_array(1)
}
"#;
    let err = check_all(src).expect_err("to_array args");
    assert!(err.contains("method `to_array`: expected 0 args"), "{err}");
}

#[test]
fn typecheck_array_matrix_conversion_methods_ok() {
    let src = r#"
fn main() i32 {
    let rows = [
        [1, 2, 3],
        [4, 5, 6],
    ];
    let m: i32[] = rows.to_matrix();
    let back: [[i32; 3]; 2] = m.to_array();
    matrix_drop(m);
    back[0][0]
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_array_to_matrix_rejects_flat_array() {
    let src = r#"
fn main() i32 {
    [1, 2, 3].to_matrix()
}
"#;
    let err = check_all(src).expect_err("flat array to_matrix");
    assert!(
        err.contains("method `to_matrix` expects an array of arrays"),
        "{err}"
    );
}

#[test]
fn typecheck_array_to_matrix_rejects_non_numeric_cells() {
    let src = r#"
fn main() i32 {
    [[true, false]].to_matrix()
}
"#;
    let err = check_all(src).expect_err("non-numeric to_matrix");
    assert!(
        err.contains("method `to_matrix` cells must be numeric"),
        "{err}"
    );
}

#[test]
fn typecheck_matrix_to_array_rejects_unknown_shape() {
    let src = r#"
fn f(m: i32[]) i32 {
    let back = m.to_array();
    0
}
"#;
    let err = check_all(src).expect_err("unknown matrix shape to_array");
    assert!(
        err.contains("method `to_array` needs a Matrix with a known shape"),
        "{err}"
    );
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
    let m: i32[] = matrix([
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: i32[] = matrix([
        [10, 20],
        [30, 40],
    ]);
    let c: i32[] = a + b;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: f64[] = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: i32[] = a + b;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: i32[] = matrix([
        [10, 20],
        [30, 40],
    ]);
    let c: i32[] = a - b;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: f64[] = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: i32[] = a - b;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: i32[] = matrix([
        [10, 20],
        [30, 40],
    ]);
    let c: i32[] = a * b;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: f64[] = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: i32[] = a * b;
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
    let a: i32[] = matrix([
        [1, 2, 3],
        [4, 5, 6],
    ]);
    let b: i32[] = matrix([
        [7, 8],
        [9, 10],
        [11, 12],
    ]);
    let c: i32[] = a @ b;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let b: f64[] = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let c: i32[] = a @ b;
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
    let m: i32[] = matrix([
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
    let m: i32[] = matrix([
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
    let c: i32[] = outer(a, b);
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
    let c: i32[] = outer(a, b);
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
    let c: i32[] = outer(a, 3);
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
    let m: i32[] = matrix([
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let right: i32[] = a * 3;
    let left: i32[] = 2 * a;
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
    let a: f64[] = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let scaled: f64[] = a * 2.0;
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
    let a: i32[] = matrix([
        [1, 2],
        [3, 4],
    ]);
    let scaled: f64[] = a * 2.0;
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
    let a: f64[] = matrix([
        [1.0, 2.0],
        [3.0, 4.0],
    ]);
    let scaled: f64[] = a * 2;
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
    let m: i32[] = matrix([
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
    let m: f64[] = matrix([
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
    let m: i32[] = matrix([
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
    let m: i32[] = matrix([
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
fn typecheck_anon_vector_type_annotation_ok() {
    let src = r#"
fn main() i32 {
    let a: i64<3> = <1, 2, 3>;
    let b: i64<3> = <4, 5, 6>;
    let dot: i64 = a @ b;
    println(a + b);
    println(dot);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_anon_vector_type_annotation_rejects_length_mismatch() {
    let src = r#"
fn main() i32 {
    let a: i32<3> = <1, 2>;
    println(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
}

#[test]
fn typecheck_heap_anon_vector_builtins_ok() {
    let src = r#"
fn main() i32 {
    let v: f64<> = <1.0, 2.0, 3.0>;
    println(vector_len(v));
    println(len(v));
    println(vector_get(v, 1));
    vector_set(v, 2, 9.0);
    let shared: f64<> = vector_clone(v);
    println(vector_refcount(v));
    println(shared);
    vector_drop(shared);
    vector_drop(v);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_heap_anon_vector_arithmetic_ok() {
    let src = r#"
fn main() i32 {
    let a: i32<> = <1, 2, 3>;
    let b: i32<> = <4, 5, 6>;
    let c: i32<> = a + b;
    let d: i32<> = c * 2;
    let dot: i32 = d @ <7, 8, 9>;
    println(dot);
    vector_drop(d);
    vector_drop(c);
    vector_drop(b);
    vector_drop(a);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_heap_anon_vector_rejects_static_length_annotation() {
    let src = r#"
fn main() i32 {
    let v: i32<> = <1, 2, 3>;
    let bad: i32<3> = v;
    println(bad);
    vector_drop(v);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_err(), "{r:?}");
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
    let m: i32[] = outer(<1, 2, 3>, <4, 5>);
    println(m);
    matrix_drop(m);
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}
