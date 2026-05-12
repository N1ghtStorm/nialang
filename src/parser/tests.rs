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
fn parse_fixture_enum_match() {
    parse_ok(include_str!("../../examples/tests/ok_enum_match.nia"));
}

#[test]
fn parse_fixture_enum_payload_match() {
    parse_ok(include_str!("../../examples/tests/ok_enum_payload_match.nia"));
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
    parse_ok(include_str!("../../examples/tests/ok_array_index_store.nia"));
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
