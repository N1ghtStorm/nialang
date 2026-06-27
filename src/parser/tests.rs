use super::*;

/// Shared parser assertion helper for fixtures and inline snippets.
fn parse_ok(src: &str) {
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn parse_fixture_minimal() {
    parse_ok(include_str!("../../examples/tests/ok_minimal.nia"));
}

#[test]
fn parse_fixture_if_return() {
    parse_ok(include_str!("../../examples/tests/ok_if_return.nia"));
}

#[test]
fn parse_fixture_tuple_struct() {
    parse_ok(include_str!("../../examples/tests/ok_tuple_struct.nia"));
}

#[test]
fn parse_fixture_named_struct() {
    parse_ok(include_str!("../../examples/tests/ok_struct_named.nia"));
}

#[test]
fn parse_struct_enum_and_vector_abilities() {
    let src = r#"
struct BoxI32 has deref, drop {
    ptr: &i32,
}

struct Pair has copy, clone (i32, i32)

enum Maybe has copy, clone, drop {
    Some(i32),
    None,
}

vector Point i32 [ X, Y ] has copy, clone, drop

fn main() i32 { 0 }
"#;
    let toks = tokenize(src);
    let (structs, enums, _fns, vectors) = Parser::new(toks).parse_file().unwrap();

    assert_eq!(structs[0].abilities, vec![Ability::Deref, Ability::Drop]);
    assert_eq!(structs[1].abilities, vec![Ability::Copy, Ability::Clone]);
    assert_eq!(
        enums[0].abilities,
        vec![Ability::Copy, Ability::Clone, Ability::Drop]
    );
    assert_eq!(
        vectors[0].abilities,
        vec![Ability::Copy, Ability::Clone, Ability::Drop]
    );
}

#[test]
fn parse_rejects_duplicate_ability() {
    let toks = tokenize("struct Bad has copy, copy { x: i32 }");
    let err = Parser::new(toks)
        .parse_file()
        .expect_err("duplicate ability should be rejected");
    assert!(err.contains("duplicate ability"), "{err}");
}

#[test]
fn parse_clone_keyword_method_call() {
    let src = r#"
struct Token has clone {
    id: i32,
}

fn main() i32 {
    let token = Token { id: 1 };
    let cloned = token.clone();
    cloned.id
}
"#;
    let toks = tokenize(src);
    let (_structs, _enums, fns, _vectors) = Parser::new(toks).parse_file().unwrap();
    let Stmt::Let {
        init: Some(init), ..
    } = &fns[0].body.stmts[1]
    else {
        panic!("expected clone let statement");
    };
    let Expr::MethodCall { name, args, .. } = init else {
        panic!("expected clone method call");
    };
    assert_eq!(name, "clone");
    assert!(args.is_empty());
}

#[test]
fn parse_drop_keyword_call() {
    let src = r#"
fn main() {
    drop(value);
}
"#;
    let toks = tokenize(src);
    let (_structs, _enums, fns, _vectors) = Parser::new(toks).parse_file().unwrap();
    let Stmt::Expr(Expr::Call { name, args }) = &fns[0].body.stmts[0] else {
        panic!("expected drop call expression statement");
    };
    assert_eq!(name, "drop");
    assert_eq!(args.len(), 1);
}

#[test]
fn parse_uninitialized_typed_let() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

fn main() i32 {
    let h: FileHandle;
    0
}
"#;
    let toks = tokenize(src);
    let (_structs, _enums, fns, _vectors) = Parser::new(toks).parse_file().unwrap();
    let Stmt::Let {
        name,
        ty: Some(Ty::Struct(ty_name)),
        init: None,
    } = &fns[0].body.stmts[0]
    else {
        panic!("expected uninitialized typed let");
    };
    assert_eq!(name, "h");
    assert_eq!(ty_name, "FileHandle");
}

#[test]
fn parse_fixture_impl_methods() {
    parse_ok(include_str!("../../examples/tests/ok_impl_methods.nia"));
}

#[test]
fn parse_sample_struct_methods_big() {
    parse_ok(include_str!("../../examples/sample_struct_methods_big.nia"));
}

#[test]
fn parse_fixture_quant_scope() {
    parse_ok(include_str!("../../examples/tests/ok_quant_scope.nia"));
}

#[test]
fn parse_fixture_gpu_scope() {
    parse_ok(include_str!("../../examples/tests/ok_gpu_scope.nia"));
}

#[test]
fn parse_quant_expression_with_tail() {
    parse_ok(
        r#"
fn main() i32 {
    let y = quant {
        let local = 41;
        local
    };
    y
}
"#,
    );
}

#[test]
fn parse_gpu_scope_and_expression_with_tail() {
    parse_ok(
        r#"
fn main() i32 {
    gpu {
        println(1);
    }
    let y = gpu {
        let local = 41;
        local
    };
    y
}
"#,
    );
}

#[test]
fn parse_extern_fn_marker() {
    let src = r#"
extern fn helper(x: i32) i32 {
    x + 1
}

fn main() i32 {
    helper(41)
}
"#;
    let toks = tokenize(src);
    let (_, _, fns, _) = Parser::new(toks).parse_file().expect("parse");
    assert!(fns[0].is_extern);
    assert_eq!(fns[0].name, "helper");
    assert!(!fns[1].is_extern);
}

#[test]
fn parse_quant_fn_marker() {
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
    let toks = tokenize(src);
    let (_, _, fns, _) = Parser::new(toks).parse_file().expect("parse");
    assert!(fns[0].is_quantum);
    assert!(!fns[0].is_extern);
    assert_eq!(fns[0].name, "prepare");
    assert!(!fns[1].is_quantum);
}

#[test]
fn parse_inline_module_qualifies_items() {
    let src = r#"
pub mod math {
    pub struct Pair { value: i32 }

    pub fn inc(x: i32) i32 {
        x + 1
    }
}

fn main() i32 {
    math::inc(41)
}
"#;
    let toks = tokenize(src);
    let (structs, _, fns, _) = Parser::new(toks).parse_file().expect("parse module");
    assert!(structs.iter().any(|s| s.name == "math::Pair"));
    assert!(fns.iter().any(|f| f.name == "math::inc"));
    assert!(fns.iter().any(|f| f.name == "main"));
}

#[test]
fn parse_rejects_extern_method_marker() {
    let src = r#"
struct Counter { value: i32 }

impl Counter {
    extern fn get(self) i32 {
        self.value
    }
}

fn main() i32 {
    let c = Counter { value: 7 };
    c.get()
}
"#;
    let toks = tokenize(src);
    let err = Parser::new(toks).parse_file().expect_err("extern method");
    assert!(err.contains("extern methods are not supported"), "{err}");
}

#[test]
fn parse_rejects_impl_method_without_self() {
    let src = r#"
struct Point { x: i32, y: i32 }

impl Point {
    fn sum(p: Point) i32 {
        p.x + p.y
    }
}
"#;
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_err());
}

#[test]
fn parse_rejects_mut_self() {
    let src = r#"
struct Point { x: i32, y: i32 }

impl Point {
    fn sum(mut self) i32 {
        self.x + self.y
    }
}
"#;
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_err());
}

#[test]
fn parse_fixture_pointers() {
    parse_ok(include_str!("../../examples/tests/ok_pointers.nia"));
}

#[test]
fn parse_fixture_nested_if() {
    parse_ok(include_str!("../../examples/tests/ok_nested_if.nia"));
}

#[test]
fn parse_fixture_tuple_named_mix() {
    parse_ok(include_str!("../../examples/tests/ok_tuple_named_mix.nia"));
}

#[test]
fn parse_fixture_print_array() {
    parse_ok(include_str!("../../examples/tests/ok_print_array.nia"));
}

#[test]
fn parse_fixture_print_structs() {
    parse_ok(include_str!("../../examples/tests/ok_print_structs.nia"));
}

#[test]
fn parse_fixture_alloc_heap() {
    parse_ok(include_str!("../../examples/tests/ok_alloc_heap.nia"));
}

#[test]
fn parse_fixture_for_range() {
    parse_ok(include_str!("../../examples/tests/ok_for_range.nia"));
}

#[test]
fn parse_fixture_while() {
    parse_ok(include_str!("../../examples/tests/ok_while.nia"));
}

#[test]
fn parse_fixture_loop() {
    parse_ok(include_str!("../../examples/tests/ok_loop.nia"));
}

#[test]
fn parse_fixture_compound_assign() {
    parse_ok(include_str!("../../examples/tests/ok_compound_assign.nia"));
}

#[test]
fn parse_fixture_bitwise() {
    parse_ok(include_str!("../../examples/tests/ok_bitwise.nia"));
}

#[test]
fn parse_fixture_ptr_write() {
    parse_ok(include_str!("../../examples/tests/ok_ptr_write.nia"));
}

#[test]
fn parse_fixture_ptr_array_write() {
    parse_ok(include_str!("../../examples/tests/ok_ptr_array_write.nia"));
}

#[test]
fn parse_fixture_readme_arrays() {
    parse_ok(include_str!("../../examples/tests/ok_readme_arrays.nia"));
}

#[test]
fn parse_fixture_readme_enums() {
    parse_ok(include_str!("../../examples/tests/ok_readme_enums.nia"));
}

#[test]
fn parse_fixture_readme_pointers() {
    parse_ok(include_str!("../../examples/tests/ok_readme_pointers.nia"));
}

#[test]
fn parse_fixture_matrix_rc() {
    parse_ok(include_str!("../../examples/sample_matrix_rc.nia"));
}

#[test]
fn parse_fixture_matrix_arith() {
    parse_ok(include_str!("../../examples/sample_matrix_arith.nia"));
}

#[test]
fn parse_fixture_matrix_det() {
    parse_ok(include_str!("../../examples/sample_matrix_det.nia"));
}

#[test]
fn parse_fixture_matrix_vector() {
    parse_ok(include_str!("../../examples/sample_matrix_vector.nia"));
}

#[test]
fn parse_fixture_list() {
    parse_ok(include_str!("../../examples/sample_list.nia"));
}

#[test]
fn parse_fixture_dft_list() {
    parse_ok(include_str!("../../examples/sample_dft_list.nia"));
}

#[test]
fn parse_fixture_anon_vector() {
    parse_ok(include_str!("../../examples/sample_anon_vector.nia"));
}

#[test]
fn parse_anon_vector_type_annotation() {
    let src = r#"
fn main(v: i32<6>) i32 {
    let pair: f64<2> = <1.0, 2.0>;
    0
}
"#;
    let toks = tokenize(src);
    let (_, _, fns, _) = Parser::new(toks).parse_file().expect("parse");
    assert_eq!(fns[0].params[0].1, Ty::AnonVector(Box::new(Ty::I32), 6));
    match &fns[0].body.stmts[0] {
        Stmt::Let { ty: Some(ty), .. } => {
            assert_eq!(ty, &Ty::AnonVector(Box::new(Ty::F64), 2));
        }
        other => panic!("expected typed let, got {other:?}"),
    }
}

#[test]
fn parse_heap_anon_vector_type_annotation() {
    let src = r#"
fn main(v: f64<>) i32 {
    let heap: i32<> = <1, 2, 3>;
    0
}
"#;
    let toks = tokenize(src);
    let (_, _, fns, _) = Parser::new(toks).parse_file().expect("parse");
    assert_eq!(fns[0].params[0].1, Ty::HeapVector(Box::new(Ty::F64)));
    match &fns[0].body.stmts[0] {
        Stmt::Let { ty: Some(ty), .. } => {
            assert_eq!(ty, &Ty::HeapVector(Box::new(Ty::I32)));
        }
        other => panic!("expected typed let, got {other:?}"),
    }
}

#[test]
fn parse_list_type_and_generic_constructors() {
    let src = r#"
fn main() i32 {
    let bytes: List[u8] = list_new[u8]();
    let zs = list_with_capacity[Complex](10);
    bytes.push(1);
    zs.len()
}
"#;
    let toks = tokenize(src);
    let (_, _, fns, _) = Parser::new(toks).parse_file().expect("parse");
    match &fns[0].body.stmts[0] {
        Stmt::Let {
            ty: Some(ty),
            init: Some(init),
            ..
        } => {
            assert_eq!(ty, &Ty::List(Box::new(Ty::U8)));
            assert!(matches!(
                init,
                Expr::GenericCall { name, ty_args, args }
                    if name == "list_new" && ty_args == &vec![Ty::U8] && args.is_empty()
            ));
        }
        other => panic!("expected typed list let, got {other:?}"),
    }
}

#[test]
fn parse_rejects_zero_anon_vector_type_length() {
    let src = r#"
fn main(v: i32<0>) i32 {
    0
}
"#;
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_err());
}

#[test]
fn parse_fixture_enum_match() {
    parse_ok(include_str!("../../examples/tests/ok_enum_match.nia"));
}

#[test]
fn parse_fixture_enum_payload_match() {
    parse_ok(include_str!(
        "../../examples/tests/ok_enum_payload_match.nia"
    ));
}

#[test]
fn parse_fixture_print_enum() {
    parse_ok(include_str!("../../examples/tests/ok_print_enum.nia"));
}

#[test]
fn parse_fixture_ok_floats() {
    parse_ok(include_str!("../../examples/tests/ok_floats.nia"));
}

#[test]
fn parse_float_fn_param_and_return() {
    let src = r#"
fn scale(x: f32, k: f64) f64 {
    1.0
}
fn main() i32 {
    0
}
"#;
    parse_ok(src);
}

#[test]
fn parse_numeric_literals_with_digit_separators() {
    parse_ok(
        r#"
fn main() i32 {
    let count: i32 = 1_000;
    let ratio: f64 = 3.141_592;
    let scale: f64 = 1.0e1_0;
    count
}
"#,
    );
}

#[test]
fn parse_array_type_and_literal() {
    parse_ok(include_str!("../../examples/tests/ok_array.nia"));
}

#[test]
fn parse_vector_decl_brackets() {
    let src = r#"
vector Point i32 [ X, Y, Z ]
fn main() i32 { 0 }
"#;
    parse_ok(src);
}

#[test]
fn parse_vector_decl_braces_legacy() {
    let src = r#"
vector Point i32 { X, Y, Z }
fn main() i32 { 0 }
"#;
    parse_ok(src);
}

#[test]
fn parse_array_index_expression() {
    let src = r#"
fn main() i32 {
let arr: [u8; 3] = [1, 2, 3];
let x: u8 = arr[1];
0
}
"#;
    parse_ok(src);
}

#[test]
fn parse_fixture_array_index_store() {
    parse_ok(include_str!(
        "../../examples/tests/ok_array_index_store.nia"
    ));
}

#[test]
fn parse_fixture_array_reverse() {
    parse_ok(include_str!("../../examples/tests/ok_array_reverse.nia"));
}

#[test]
fn parse_fixture_array_len() {
    parse_ok(include_str!("../../examples/tests/ok_array_len.nia"));
}

#[test]
fn parse_vector_dot_expression() {
    let src = r#"
vector V2 i32 [ X, Y ]
fn main() i32 {
    let u = V2 [X: 1, Y: 2];
    let v = V2 [X: 3, Y: 4];
    u @ v
}
"#;
    parse_ok(src);
}

#[test]
fn parse_comparison_expression() {
    let src = r#"
fn main() i32 {
    let a: i32 = 3;
    let b: i32 = 4;
    if a < b {
        1
    }
    0
}
"#;
    parse_ok(src);
}

#[test]
fn parse_logical_not_expression() {
    let src = r#"
fn main() i32 {
    let ready: bool = !!false;
    if !ready {
        return 1
    }
    0
}
"#;
    parse_ok(src);
}

#[test]
fn parse_function_type_and_closure_literals() {
    let src = r#"
fn main() i32 {
    let add1: fn(i32) -> i32 = |x| x + 1;
    let print_i32: fn(i32) -> () = |x| println(x);
    add1(41)
}
"#;
    parse_ok(src);
}

#[test]
fn parse_move_closure_literal() {
    let src = r#"
fn main() i32 {
    let base: i32 = 10;
    let add_base: fn(i32) -> i32 = move |x| x + base;
    add_base(1)
}
"#;
    parse_ok(src);
}

#[test]
fn parse_inline_if_return_bool() {
    let src = r#"
fn bar(foo: bool) i32 {
if foo {
    return 1
}
0
}
"#;
    parse_ok(src);
}

#[test]
fn parse_tuple_struct_and_index_field() {
    let src = r#"
struct Foo (u8, i32, u8, u128)
fn main() i32 {
let f = Foo(1, 2, 3, 4);
f.1
}
"#;
    parse_ok(src);
}

#[test]
fn parse_rejects_bad_tuple_struct() {
    let src = "struct Foo (u8, i32";
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_err());
}

#[test]
fn parse_rejects_missing_struct_colon() {
    let src = "struct A { x i32 }";
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_err());
}

#[test]
fn parse_rejects_unclosed_block() {
    let src = "fn main() i32 { let a = 1;";
    let toks = tokenize(src);
    let r = Parser::new(toks).parse_file();
    assert!(r.is_err());
}
