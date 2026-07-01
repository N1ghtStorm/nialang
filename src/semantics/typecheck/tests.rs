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
        include_str!("../../../examples/tests/ok_if_else.nia"),
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
        include_str!("../../../examples/tests/ok_break_continue.nia"),
        include_str!("../../../examples/tests/ok_compound_assign.nia"),
        include_str!("../../../examples/tests/ok_bitwise.nia"),
        include_str!("../../../examples/tests/ok_atomic_bool.nia"),
        include_str!("../../../examples/tests/ok_atomic_ptr.nia"),
        include_str!("../../../examples/tests/ok_atomic_int.nia"),
        include_str!("../../../examples/tests/ok_atomic_narrow_int.nia"),
        include_str!("../../../examples/tests/ok_atomic_i128.nia"),
        include_str!("../../../examples/tests/ok_threads_minimal.nia"),
        include_str!("../../../examples/tests/ok_spawn_move_closure.nia"),
        include_str!("../../../examples/tests/ok_send_sync_struct.nia"),
        include_str!("../../../examples/tests/ok_arc_basic.nia"),
        include_str!("../../../examples/tests/ok_mutex_basic.nia"),
        include_str!("../../../examples/tests/ok_rwlock_read_write.nia"),
        include_str!("../../../examples/tests/ok_condvar_wait_notify.nia"),
        include_str!("../../../examples/sample_send_sync.nia"),
        include_str!("../../../examples/sample_arc.nia"),
        include_str!("../../../examples/sample_threads_closure.nia"),
        include_str!("../../../examples/sample_mutex.nia"),
        include_str!("../../../examples/sample_rwlock.nia"),
        include_str!("../../../examples/sample_condvar.nia"),
        include_str!("../../../examples/tests/ok_option_result.nia"),
        include_str!("../../../examples/tests/ok_floats.nia"),
        include_str!("../../../examples/tests/ok_string.nia"),
        include_str!("../../../examples/sample_struct_methods_big.nia"),
        include_str!("../../../examples/sample_atomic_ptr.nia"),
        include_str!("../../../examples/sample_atomic_int.nia"),
        include_str!("../../../examples/sample_atomic_narrow_int.nia"),
        include_str!("../../../examples/sample_atomic_i128.nia"),
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
fn typecheck_rejects_bitwise_non_integer_operands() {
    for expr in ["true & false", "1.0 | 2.0", "~false", "1.0 << 2.0"] {
        let src = format!(
            r#"
fn main() i32 {{
    let value = {expr};
    0
}}
"#
        );
        let err = check_all(&src).expect_err("bitwise expression must require integers");
        assert!(err.contains("non-integer"), "{expr}: {err}");
    }
}

#[test]
fn typecheck_u32_primitive_ops() {
    let src = r#"
fn main() i32 {
    let a: u32 = 10;
    let b: u32 = 3;
    let c: u32 = ((a + b) - 1) * 2;
    let d: u32 = c / b;
    let e: u32 = d % b;
    let f: u32 = (e << 1) >> 1;
    println(f);
    0
}
"#;
    check_all(src).expect("u32 primitive operations should typecheck");
}

#[test]
fn typecheck_builtin_ordering_enum() {
    let src = r#"
fn main() i32 {
    let load_order: Ordering = Ordering::Acquire;
    let store_order: Ordering = Ordering::Release;
    0
}
"#;
    check_all(src).expect("builtin Ordering should typecheck");
}

#[test]
fn typecheck_rejects_user_defined_ordering_type() {
    let src = r#"
enum Ordering {
    Acquire,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("Ordering is reserved");
    assert!(err.contains("reserved"), "{err}");
}

#[test]
fn typecheck_rejects_reserved_atomic_names() {
    let type_src = r#"
struct AtomicBool {
    value: bool,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(type_src).expect_err("AtomicBool is reserved");
    assert!(err.contains("reserved"), "{err}");

    let fn_src = r#"
fn atomic_bool(value: bool) bool {
    value
}

fn main() i32 {
    0
}
"#;
    let err = check_all(fn_src).expect_err("atomic_bool is reserved");
    assert!(err.contains("reserved"), "{err}");

    let atomic_ptr_type_src = r#"
struct AtomicPtr {
    value: i32,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(atomic_ptr_type_src).expect_err("AtomicPtr is reserved");
    assert!(err.contains("reserved"), "{err}");

    let atomic_ptr_fn_src = r#"
fn atomic_ptr(value: &i32) i32 {
    *value
}

fn main() i32 {
    0
}
"#;
    let err = check_all(atomic_ptr_fn_src).expect_err("atomic_ptr is reserved");
    assert!(err.contains("reserved"), "{err}");
}

#[test]
fn atomic_ordering_literal_helpers_validate_rules() {
    let parsed =
        parse_ordering_literal(&Expr::Ident("Ordering::SeqCst".into())).expect("literal ordering");
    assert_eq!(parsed, crate::nia_std::AtomicOrdering::SeqCst);
    assert!(parse_ordering_literal(&Expr::Ident("other::SeqCst".into())).is_err());
    assert!(parse_ordering_literal(&Expr::Ident("SeqCst".into())).is_err());

    use crate::nia_std::AtomicOrdering::{AcqRel, Acquire, Relaxed, Release, SeqCst};
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::Load, Acquire).is_ok());
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::Load, Release).is_err());
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::Store, Release).is_ok());
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::Store, Acquire).is_err());
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::ReadModifyWrite, AcqRel).is_ok());
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::Fence, SeqCst).is_ok());
    assert!(check_atomic_ordering_for_op(AtomicOrderingUse::Fence, Relaxed).is_err());

    assert!(check_compare_exchange_orderings(AcqRel, Acquire).is_ok());
    assert!(check_compare_exchange_orderings(Release, Acquire).is_err());
    assert!(check_compare_exchange_orderings(SeqCst, SeqCst).is_ok());
}

#[test]
fn atomic_lvalue_receiver_helper_accepts_initial_receiver_set() {
    assert!(check_atomic_lvalue_receiver(&Expr::Ident("cell".into())).is_ok());
    assert!(check_atomic_lvalue_receiver(&Expr::Deref(Box::new(Expr::Ident("p".into())))).is_ok());
    assert!(
        check_atomic_lvalue_receiver(&Expr::Field(
            Box::new(Expr::Ident("owner".into())),
            "cell".into()
        ))
        .is_err()
    );
}

#[test]
fn typecheck_atomic_bool_methods() {
    let src = include_str!("../../../examples/tests/ok_atomic_bool.nia");
    check_all(src).expect("AtomicBool methods should typecheck");
}

#[test]
fn typecheck_atomic_bool_rejects_plain_reads_and_assignment() {
    let plain_read = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let snapshot = ready;
    0
}
"#;
    let err = check_all(plain_read).expect_err("atomic plain read should fail");
    assert!(err.contains("cannot be read directly"), "{err}");

    let assignment = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    ready = atomic_bool(true);
    0
}
"#;
    let err = check_all(assignment).expect_err("atomic assignment should fail");
    assert!(err.contains("cannot assign to atomic value"), "{err}");

    let direct_deref = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let p = &ready;
    let snapshot = *p;
    0
}
"#;
    let err = check_all(direct_deref).expect_err("atomic pointer deref read should fail");
    assert!(err.contains("cannot be read directly"), "{err}");
}

#[test]
fn typecheck_atomic_bool_rejects_invalid_orderings() {
    let bad_load = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let value = ready.load(Ordering::Release);
    0
}
"#;
    let err = check_all(bad_load).expect_err("load release should fail");
    assert!(err.contains("not a valid ordering"), "{err}");

    let bad_store = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    ready.store(true, Ordering::Acquire);
    0
}
"#;
    let err = check_all(bad_store).expect_err("store acquire should fail");
    assert!(err.contains("not a valid ordering"), "{err}");

    let bad_cmpxchg = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let ok = ready.compare_exchange(false, true, Ordering::Release, Ordering::Acquire);
    0
}
"#;
    let err = check_all(bad_cmpxchg).expect_err("bad cmpxchg failure ordering should fail");
    assert!(err.contains("stronger than success"), "{err}");
}

#[test]
fn typecheck_atomic_bool_allows_pointer_receiver() {
    let src = r#"
fn observe(p: &AtomicBool) bool {
    (*p).load(Ordering::Acquire)
}

fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let value = observe(&ready);
    println(value);
    0
}
"#;
    check_all(src).expect("AtomicBool pointer receiver should typecheck");
}

#[test]
fn typecheck_atomic_bool_rejects_by_value_signatures_and_fields() {
    let param = r#"
fn bad(cell: AtomicBool) bool {
    cell.load(Ordering::Acquire)
}
"#;
    let err = check_all(param).expect_err("AtomicBool parameter should fail");
    assert!(err.contains("atomic storage by value"), "{err}");

    let ret = r#"
fn bad() AtomicBool {
    atomic_bool(false)
}
"#;
    let err = check_all(ret).expect_err("AtomicBool return should fail");
    assert!(err.contains("atomic storage by value"), "{err}");

    let field = r#"
struct Holder {
    ready: AtomicBool,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(field).expect_err("AtomicBool field should fail for Phase 1");
    assert!(err.contains("atomic storage by value"), "{err}");
}

#[test]
fn typecheck_atomic_ptr_methods() {
    let src = include_str!("../../../examples/tests/ok_atomic_ptr.nia");
    check_all(src).expect("AtomicPtr methods should typecheck");
}

#[test]
fn typecheck_atomic_ptr_rejects_plain_reads_and_assignment() {
    let plain_read = r#"
fn main() i32 {
    let value: i32 = 1;
    let slot: AtomicPtr[i32] = atomic_ptr(&value);
    let snapshot = slot;
    0
}
"#;
    let err = check_all(plain_read).expect_err("atomic pointer plain read should fail");
    assert!(err.contains("cannot be read directly"), "{err}");

    let assignment = r#"
fn main() i32 {
    let a: i32 = 1;
    let b: i32 = 2;
    let slot: AtomicPtr[i32] = atomic_ptr(&a);
    slot = atomic_ptr(&b);
    0
}
"#;
    let err = check_all(assignment).expect_err("atomic pointer assignment should fail");
    assert!(err.contains("cannot assign to atomic value"), "{err}");

    let direct_deref = r#"
fn main() i32 {
    let value: i32 = 1;
    let slot: AtomicPtr[i32] = atomic_ptr(&value);
    let p = &slot;
    let snapshot = *p;
    0
}
"#;
    let err = check_all(direct_deref).expect_err("atomic pointer deref read should fail");
    assert!(err.contains("cannot be read directly"), "{err}");
}

#[test]
fn typecheck_atomic_ptr_rejects_invalid_orderings() {
    let bad_load = r#"
fn main() i32 {
    let value: i32 = 1;
    let slot: AtomicPtr[i32] = atomic_ptr(&value);
    let current = slot.load(Ordering::Release);
    0
}
"#;
    let err = check_all(bad_load).expect_err("AtomicPtr load release should fail");
    assert!(err.contains("not a valid ordering"), "{err}");

    let bad_store = r#"
fn main() i32 {
    let a: i32 = 1;
    let b: i32 = 2;
    let slot: AtomicPtr[i32] = atomic_ptr(&a);
    slot.store(&b, Ordering::Acquire);
    0
}
"#;
    let err = check_all(bad_store).expect_err("AtomicPtr store acquire should fail");
    assert!(err.contains("not a valid ordering"), "{err}");

    let bad_cmpxchg = r#"
fn main() i32 {
    let a: i32 = 1;
    let b: i32 = 2;
    let slot: AtomicPtr[i32] = atomic_ptr(&a);
    let ok = slot.compare_exchange(&a, &b, Ordering::Release, Ordering::Acquire);
    0
}
"#;
    let err = check_all(bad_cmpxchg).expect_err("AtomicPtr bad cmpxchg should fail");
    assert!(err.contains("stronger than success"), "{err}");
}

#[test]
fn typecheck_atomic_ptr_allows_pointer_receiver() {
    let src = r#"
fn observe(p: &AtomicPtr[i32]) &i32 {
    (*p).load(Ordering::Acquire)
}

fn main() i32 {
    let value: i32 = 1;
    let slot: AtomicPtr[i32] = atomic_ptr(&value);
    let current = observe(&slot);
    println(*current);
    0
}
"#;
    check_all(src).expect("AtomicPtr pointer receiver should typecheck");
}

#[test]
fn typecheck_atomic_ptr_rejects_by_value_signatures_fields_and_bad_pointees() {
    let param = r#"
fn bad(cell: AtomicPtr[i32]) &i32 {
    cell.load(Ordering::Acquire)
}
"#;
    let err = check_all(param).expect_err("AtomicPtr parameter should fail");
    assert!(err.contains("atomic storage by value"), "{err}");

    let ret = r#"
fn bad(value: &i32) AtomicPtr[i32] {
    atomic_ptr(value)
}
"#;
    let err = check_all(ret).expect_err("AtomicPtr return should fail");
    assert!(err.contains("atomic storage by value"), "{err}");

    let field = r#"
struct Holder {
    slot: AtomicPtr[i32],
}

fn main() i32 {
    0
}
"#;
    let err = check_all(field).expect_err("AtomicPtr field should fail");
    assert!(err.contains("atomic storage by value"), "{err}");

    let unit = r#"
fn bad(cell: &AtomicPtr[()]) {
}
"#;
    let err = check_all(unit).expect_err("AtomicPtr unit pointee should fail");
    assert!(err.contains("AtomicPtr[()]"), "{err}");

    let quantum = r#"
fn bad(cell: &AtomicPtr[qubit]) {
}
"#;
    let err = check_all(quantum).expect_err("AtomicPtr quantum pointee should fail");
    assert!(err.contains("cannot point to quantum types"), "{err}");
}

#[test]
fn typecheck_atomic_ptr_rejects_non_pointer_constructor_and_bool_fetch_ops() {
    let non_ptr = r#"
fn main() i32 {
    let slot = atomic_ptr(1);
    0
}
"#;
    let err = check_all(non_ptr).expect_err("atomic_ptr requires pointer argument");
    assert!(err.contains("expects `&T`"), "{err}");

    let fetch = r#"
fn main() i32 {
    let value: i32 = 1;
    let slot: AtomicPtr[i32] = atomic_ptr(&value);
    let old = slot.fetch_xor(&value, Ordering::AcqRel);
    0
}
"#;
    let err = check_all(fetch).expect_err("AtomicPtr does not support bool fetch ops");
    assert!(err.contains("not supported for `AtomicPtr[T]`"), "{err}");
}

#[test]
fn typecheck_atomic_int_methods() {
    let src = include_str!("../../../examples/tests/ok_atomic_int.nia");
    check_all(src).expect("integer atomic methods should typecheck");
}

#[test]
fn typecheck_atomic_int_rejects_plain_reads_assignment_and_by_value() {
    let plain_read = r#"
fn main() i32 {
    let counter: AtomicI32 = atomic_i32(1);
    let snapshot = counter;
    0
}
"#;
    let err = check_all(plain_read).expect_err("atomic integer plain read should fail");
    assert!(err.contains("cannot be read directly"), "{err}");

    let assignment = r#"
fn main() i32 {
    let counter: AtomicI32 = atomic_i32(1);
    counter = atomic_i32(2);
    0
}
"#;
    let err = check_all(assignment).expect_err("atomic integer assignment should fail");
    assert!(err.contains("cannot assign to atomic value"), "{err}");

    let param = r#"
fn bad(counter: AtomicU32) u32 {
    counter.load(Ordering::Acquire)
}
"#;
    let err = check_all(param).expect_err("atomic integer parameter should fail");
    assert!(err.contains("atomic storage by value"), "{err}");

    let field = r#"
struct Holder {
    counter: AtomicI64,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(field).expect_err("atomic integer field should fail");
    assert!(err.contains("atomic storage by value"), "{err}");
}

#[test]
fn typecheck_atomic_int_rejects_invalid_orderings_types_and_bool_add() {
    let bad_load = r#"
fn main() i32 {
    let counter: AtomicI32 = atomic_i32(1);
    let value = counter.load(Ordering::Release);
    0
}
"#;
    let err = check_all(bad_load).expect_err("AtomicI32 load release should fail");
    assert!(err.contains("not a valid ordering"), "{err}");

    let bad_store = r#"
fn main() i32 {
    let counter: AtomicI32 = atomic_i32(1);
    counter.store(2, Ordering::Acquire);
    0
}
"#;
    let err = check_all(bad_store).expect_err("AtomicI32 store acquire should fail");
    assert!(err.contains("not a valid ordering"), "{err}");

    let bad_cmpxchg = r#"
fn main() i32 {
    let counter: AtomicI32 = atomic_i32(1);
    let ok = counter.compare_exchange(1, 2, Ordering::Release, Ordering::Acquire);
    0
}
"#;
    let err = check_all(bad_cmpxchg).expect_err("AtomicI32 bad cmpxchg should fail");
    assert!(err.contains("stronger than success"), "{err}");

    let bad_ctor = r#"
fn main() i32 {
    let counter = atomic_i32(true);
    0
}
"#;
    let err = check_all(bad_ctor).expect_err("atomic_i32 bool argument should fail");
    assert!(err.contains("cannot satisfy I32"), "{err}");

    let bool_add = r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let old = ready.fetch_add(true, Ordering::AcqRel);
    0
}
"#;
    let err = check_all(bool_add).expect_err("AtomicBool fetch_add should fail");
    assert!(err.contains("not supported for `AtomicBool`"), "{err}");
}

#[test]
fn typecheck_atomic_narrow_int_methods() {
    let src = include_str!("../../../examples/tests/ok_atomic_narrow_int.nia");
    check_all(src).expect("narrow integer atomic methods should typecheck");
}

#[test]
fn typecheck_atomic_narrow_int_rejects_bad_constructor_and_by_value() {
    let bad_ctor = r#"
fn main() i32 {
    let cell = atomic_u8(true);
    0
}
"#;
    let err = check_all(bad_ctor).expect_err("atomic_u8 bool argument should fail");
    assert!(err.contains("cannot satisfy U8"), "{err}");

    let param = r#"
fn bad(cell: AtomicU64) u64 {
    cell.load(Ordering::Acquire)
}
"#;
    let err = check_all(param).expect_err("AtomicU64 parameter should fail");
    assert!(err.contains("atomic storage by value"), "{err}");
}

#[test]
fn typecheck_threads_minimal_surface() {
    let src = include_str!("../../../examples/tests/ok_threads_minimal.nia");
    check_all(src).expect("minimal thread spawn/join should typecheck");
}

#[test]
fn typecheck_threads_spawn_move_closure_surface() {
    let src = include_str!("../../../examples/tests/ok_spawn_move_closure.nia");
    check_all(src).expect("spawn move closure should typecheck");
}

#[test]
fn typecheck_threads_reject_unknown_spawn_target() {
    let src = r#"
fn main() i32 {
    let t = spawn missing_worker;
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("unknown spawn target should fail");
    assert!(err.contains("top-level function"), "{err}");
}

#[test]
fn typecheck_threads_join_consumes_handle() {
    let src = r#"
fn worker() {
}

fn main() i32 {
    let t = spawn worker;
    join(t);
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("join should consume Thread");
    assert!(err.contains("use of moved local `t`"), "{err}");
}

#[test]
fn typecheck_threads_reject_wrong_spawn_signature() {
    let src = r#"
fn worker(x: i32) {
    println(x);
}

fn main() i32 {
    let t = spawn worker;
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("spawn target must be fn() -> ()");
    assert!(err.contains("fn() -> ()"), "{err}");
}

#[test]
fn typecheck_threads_reject_non_send_spawn_capture() {
    let src = r#"
fn worker() {
}

fn main() i32 {
    let handle: Thread = spawn worker;
    let t: Thread = spawn move || {
        drop(handle);
    };
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("Thread capture is not send");
    assert!(err.contains("capture `handle` requires `send`"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred"),
        "{err}"
    );
}

#[test]
fn typecheck_threads_reject_spawn_closure_wrong_return() {
    let src = r#"
fn main() i32 {
    let t: Thread = spawn move || 1;
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("spawn closure must return unit");
    assert!(err.contains("cannot satisfy `()`"), "{err}");
}

#[test]
fn typecheck_threads_reject_function_value_with_non_send_env() {
    let src = r#"
fn worker() {
}

fn main() i32 {
    let handle: Thread = spawn worker;
    let f: fn() -> () = move || {
        let handle_ref = &handle;
    };
    let t: Thread = spawn move || {
        f();
    };
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("function value env is not send");
    assert!(err.contains("capture `f` requires `send`"), "{err}");
    assert!(
        err.contains("function value may own a non-`send` closure environment"),
        "{err}"
    );
}

#[test]
fn typecheck_allows_non_capturing_closure_values() {
    let src = r#"
fn apply(x: i32, f: fn(i32) -> i32) i32 {
    f(x)
}

fn main() i32 {
    let add1: fn(i32) -> i32 = |x| x + 1;
    apply(41, add1)
}
"#;
    check_all(src).expect("closures should typecheck");
}

#[test]
fn typecheck_allows_unit_return_function_values() {
    let src = r#"
fn main() i32 {
    let print_i32: fn(i32) -> () = |x| println(x);
    print_i32(7);
    0
}
"#;
    check_all(src).expect("unit-return closure should typecheck");
}

#[test]
fn typecheck_allows_copy_closure_captures() {
    let src = r#"
fn main() i32 {
    let base: i32 = 10;
    let add_base: fn(i32) -> i32 = |x| x + base;
    add_base(1)
}
"#;
    check_all(src).expect("copy captures should typecheck");
}

#[test]
fn typecheck_allows_nested_copy_closure_captures() {
    let src = r#"
fn main() i32 {
    let base: i32 = 10;
    let outer: fn(i32) -> i32 = |x| {
        let inner: fn(i32) -> i32 = |y| y + base;
        inner(x)
    };
    outer(32)
}
"#;
    check_all(src).expect("nested copy captures should typecheck");
}

#[test]
fn typecheck_rejects_non_copy_closure_capture() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let handle = FileHandle { fd: 3 };
    let read_fd: fn() -> i32 = || handle.fd;
    read_fd()
}
"#;
    let err = check_all(src).expect_err("non-copy captures are delayed");
    assert!(
        err.contains("closure capture `handle` requires `copy`"),
        "{err}"
    );
}

#[test]
fn typecheck_allows_move_closure_capture() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let handle = FileHandle { fd: 3 };
    let read_fd: fn() -> i32 = move || handle.fd;
    read_fd()
}
"#;
    check_all(src).expect("move closure should capture a droppable non-copy value");
}

#[test]
fn typecheck_rejects_use_after_move_closure_capture() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let handle = FileHandle { fd: 3 };
    let read_fd: fn() -> i32 = move || handle.fd;
    handle.fd
}
"#;
    let err = check_all(src).expect_err("move capture should move the source local");
    assert!(err.contains("use of moved local `handle`"), "{err}");
    assert!(err.contains("captured by a `move` closure"), "{err}");
    assert!(
        err.contains("struct `FileHandle` does not declare `copy`"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_moving_out_of_captured_closure_env() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let handle = FileHandle { fd: 3 };
    let consume: fn() -> () = move || {
        drop(handle);
    };
    consume();
    0
}
"#;
    let err = check_all(src).expect_err("captured env values are not FnOnce captures yet");
    assert!(
        err.contains("cannot move captured variable `handle` out of a closure environment"),
        "{err}"
    );
}

#[test]
fn typecheck_allows_explicit_drop_of_function_value() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let handle = FileHandle { fd: 3 };
    let read_fd: fn() -> i32 = move || handle.fd;
    drop(read_fd);
    0
}
"#;
    check_all(src).expect("function values own optional closure environments");
}

#[test]
fn typecheck_allows_copying_top_level_function_value() {
    let src = r#"
fn add1(x: i32) i32 {
    x + 1
}

fn main() i32 {
    let f: fn(i32) -> i32 = add1;
    let g: fn(i32) -> i32 = f;
    f(1) + g(2)
}
"#;
    check_all(src).expect("top-level function values are copyable");
}

#[test]
fn typecheck_allows_copying_non_capturing_closure_value() {
    let src = r#"
fn main() i32 {
    let f: fn(i32) -> i32 = |x| x + 1;
    let g: fn(i32) -> i32 = f;
    f(1) + g(2)
}
"#;
    check_all(src).expect("non-capturing closure values are copyable");
}

#[test]
fn typecheck_rejects_copying_capturing_closure_value() {
    let src = r#"
fn main() i32 {
    let base: i32 = 10;
    let f: fn(i32) -> i32 = |x| x + base;
    let g: fn(i32) -> i32 = f;
    f(1) + g(2)
}
"#;
    let err = check_all(src).expect_err("capturing closure values are move-only unless cloned");
    assert!(err.contains("use of moved local `f`"), "{err}");
    assert!(err.contains("bound into another local"), "{err}");
    assert!(
        err.contains("function value may own a closure environment"),
        "{err}"
    );
    assert!(err.contains("use `.clone()`"), "{err}");
}

#[test]
fn typecheck_allows_clone_of_cloneable_capturing_closure_value() {
    let src = r#"
fn main() i32 {
    let base: i32 = 10;
    let f: fn(i32) -> i32 = |x| x + base;
    let g: fn(i32) -> i32 = f.clone();
    f(1) + g(2)
}
"#;
    check_all(src).expect("capturing closure with cloneable env should clone");
}

#[test]
fn typecheck_rejects_clone_of_non_cloneable_capturing_closure_value() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let handle = FileHandle { fd: 3 };
    let f: fn() -> i32 = move || handle.fd;
    let g: fn() -> i32 = f.clone();
    g()
}
"#;
    let err = check_all(src).expect_err("drop-only capture should not be cloneable");
    assert!(
        err.contains("function value clone requires a cloneable closure environment"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_assignment_to_captured_variable() {
    let src = r#"
fn main() i32 {
    let base: i32 = 10;
    let set_base: fn() -> () = || {
        base = 20;
    };
    set_base();
    0
}
"#;
    let err = check_all(src).expect_err("captured variables are read-only for now");
    assert!(
        err.contains("assignment to captured variable `base`"),
        "{err}"
    );
}

#[test]
fn typecheck_logical_not_requires_bool() {
    check_all(
        r#"
fn main() i32 {
    let value: bool = !false;
    if !value {
        return 1
    }
    0
}
"#,
    )
    .expect("logical not should accept bool operands");

    let err = check_all(
        r#"
fn main() i32 {
    let value = !1;
    0
}
"#,
    )
    .expect_err("logical not must reject integer operands");
    assert!(err.contains("bool"), "{err}");
}

#[test]
fn typecheck_rejects_remainder_by_zero() {
    let err = check_all(
        r#"
fn main() i32 {
    let value = 10 % 0;
    value
}
"#,
    )
    .expect_err("remainder by zero must be rejected");
    assert!(err.contains("remainder by zero"), "{err}");
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
fn typecheck_option_and_result_types_in_signatures() {
    let src = r#"
fn pass_opt(x: Option[i32]) Option[i32] {
    x
}

fn pass_result(x: Result[Option[i32], string]) Result[Option[i32], string] {
    x
}

fn main() i32 {
    0
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_option_and_result_constructors_and_match() {
    let src = r#"
fn make_some() Option[i32] {
    Some(42)
}

fn make_none() Option[i32] {
    None
}

fn unwrap_or_zero(x: Option[i32]) i32 {
    match x {
        Some(n) => n,
        None => 0,
    }
}

fn make_ok() Result[i32, string] {
    Ok(7)
}

fn make_err() Result[i32, string] {
    Err("bad")
}

fn unwrap_result(x: Result[i32, string]) i32 {
    match x {
        Ok(n) => n,
        Err(e) => 0,
    }
}

fn main() i32 {
    unwrap_or_zero(make_some()) + unwrap_or_zero(make_none()) + unwrap_result(make_ok()) + unwrap_result(make_err())
}
"#;
    let r = check_all(src);
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn typecheck_option_result_reject_non_exhaustive_match() {
    let src = r#"
fn unwrap_or_zero(x: Option[i32]) i32 {
    match x {
        Some(n) => n,
    }
}
"#;
    let err = check_all(src).expect_err("non-exhaustive Option match should fail");
    assert!(err.contains("non-exhaustive match"), "{err}");
}

#[test]
fn typecheck_result_constructors_need_expected_type() {
    let src = r#"
fn main() i32 {
    let x = Ok(1);
    0
}
"#;
    let err = check_all(src).expect_err("Ok without Result hint should fail");
    assert!(err.contains("expected Result[T, E]"), "{err}");
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
fn typecheck_allows_q_read_result_as_bool_inside_quant() {
    let src = r#"
fn main() i32 {
    quant {
        let q = qubit();
        let r = q_measure(q);
        let b: bool = q_read(r);
        q_record(b);
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
    for gate in ["I", "Y", "Z", "S", "Sdg", "T", "Tdg"] {
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
    for gate in ["I", "Y", "Z", "S", "Sdg", "T", "Tdg"] {
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
    for gate in ["CNOT", "CZ", "SWAP", "CH", "CY", "CS", "CSdg", "CT", "CTdg"] {
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
    for gate in ["CNOT", "CZ", "SWAP", "CH", "CY", "CS", "CSdg", "CT", "CTdg"] {
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
    for gate in ["CNOT", "CZ", "SWAP", "CH", "CY", "CS", "CSdg", "CT", "CTdg"] {
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
fn typecheck_rejects_three_qubit_gates_outside_quant() {
    for gate in ["CCNOT", "CCZ", "CSWAP"] {
        let src = format!(
            r#"
fn main() i32 {{
    {gate}(0, 1, 2);
    0
}}
"#
        );
        let err = check_all(&src).expect_err("three-qubit gate must be quant-only");
        assert!(err.contains("only allowed inside `quant`"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_three_qubit_gates_non_qubit_argument() {
    for gate in ["CCNOT", "CCZ", "CSWAP"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(q, q, 0);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("three-qubit gate expects qubits");
        assert!(err.contains("cannot satisfy Qubit"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_three_qubit_gates_wrong_arity() {
    for gate in ["CCNOT", "CCZ", "CSWAP"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(q, q);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("three-qubit gate expects three arguments");
        assert!(err.contains("expects exactly 3 arguments"), "{gate}: {err}");
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
fn typecheck_rejects_controlled_rotation_gates_outside_quant() {
    for gate in ["CRx", "CRy", "CRz", "CR1"] {
        let src = format!(
            r#"
fn main() i32 {{
    {gate}(1.0, 0, 1);
    0
}}
"#
        );
        let err = check_all(&src).expect_err("controlled rotation gate must be quant-only");
        assert!(err.contains("only allowed inside `quant`"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_controlled_rotation_gates_non_f64_angle() {
    for gate in ["CRx", "CRy", "CRz", "CR1"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        let theta: i32 = 1;
        {gate}(theta, q, q);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("controlled rotation gate expects an f64 angle");
        assert!(err.contains("expects an f64 angle"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_controlled_rotation_gates_non_qubit_argument() {
    for gate in ["CRx", "CRy", "CRz", "CR1"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(1.0, q, 0);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("controlled rotation gate expects qubits");
        assert!(err.contains("cannot satisfy Qubit"), "{gate}: {err}");
    }
}

#[test]
fn typecheck_rejects_controlled_rotation_gates_wrong_arity() {
    for gate in ["CRx", "CRy", "CRz", "CR1"] {
        let src = format!(
            r#"
fn main() i32 {{
    quant {{
        let q = qubit();
        {gate}(1.0, q);
    }}
    0
}}
"#
        );
        let err = check_all(&src).expect_err("controlled rotation gate expects three arguments");
        assert!(err.contains("expects exactly 3 arguments"), "{gate}: {err}");
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
    let err = check_all(src).expect_err("q_record expects a result or bool");
    assert!(err.contains("expects a result or bool argument"), "{err}");
}

#[test]
fn typecheck_rejects_q_read_outside_quant() {
    let src = r#"
fn main() i32 {
    q_read(false);
    0
}
"#;
    let err = check_all(src).expect_err("q_read must be quant-only");
    assert!(err.contains("only allowed inside `quant`"), "{err}");
}

#[test]
fn typecheck_accepts_crypto_merkle_builtins() {
    let src = r#"
fn main() i32 {
    let data: [[u8; 3]; 2] = [[1, 2, 3], [4, 5, 6]];
    let root = merkle_root_from_data(data);
    let left = merkle_leaf_hash(data[0]);
    let right = merkle_leaf_hash(data[1]);
    let expected = merkle_node_hash(left, right);
    let proof: [[u8; 32]; 1] = [right];
    let ok = digest_eq(root, expected);
    let verified = merkle_verify(root, left, 0, proof);
    if ok {
        if verified {
            return 0
        }
    }
    1
}
"#;
    check_all(src).expect("crypto builtins typecheck");
}

#[test]
fn typecheck_rejects_empty_merkle_root() {
    let src = r#"
fn main() i32 {
    let leaves: [[u8; 32]; 0] = [];
    let root = merkle_root(leaves);
    println(root);
    0
}
"#;
    let err = check_all(src).expect_err("empty merkle root must be rejected");
    assert!(err.contains("expects at least one digest"), "{err}");
}

#[test]
fn typecheck_rejects_merkle_verify_bad_proof_type() {
    let src = r#"
fn main() i32 {
    let root: [u8; 32] = [
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ];
    let leaf: [u8; 32] = root;
    let proof: [[u8; 31]; 1] = [[
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0
    ]];
    merkle_verify(root, leaf, 0, proof);
    0
}
"#;
    let err = check_all(src).expect_err("proof must be digest array");
    assert!(err.contains("expected [[u8; 32]; N]"), "{err}");
}

#[test]
fn typecheck_rejects_q_read_non_result_argument() {
    let src = r#"
fn main() i32 {
    quant {
        q_read(false);
    }
    0
}
"#;
    let err = check_all(src).expect_err("q_read expects a result");
    assert!(err.contains("cannot satisfy Result"), "{err}");
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
fn typecheck_allows_qubit_array_inside_quant() {
    let src = r#"
fn main() i32 {
    quant {
        let qs: [qubit; 4] = [qubit(), qubit(), qubit(), qubit()];
        H(qs[0]);
        for i in 0..4 {
            H(qs[i]);
        }
        CNOT(qs[0], qs[1]);
        CR1(PI / 2.0, qs[1], qs[0]);
    }
    0
}
"#;
    check_all(src).expect("qubit array inside quant should typecheck");
}

#[test]
fn typecheck_rejects_qubit_array_escape_from_quant() {
    let src = r#"
fn main() i32 {
    let qs = quant {
        [qubit(), qubit()]
    };
    0
}
"#;
    let err = check_all(src).expect_err("qubit array must not escape quant");
    assert!(err.contains("cannot return quantum type"), "{err}");
}

#[test]
fn typecheck_allows_quant_fn_qubit_array_parameter() {
    let src = r#"
quant fn qft4(qs: [qubit; 4]) {
    H(qs[0]);
}

fn main() i32 {
    quant {
        let qs: [qubit; 4] = [qubit(), qubit(), qubit(), qubit()];
        qft4(qs);
    }
    0
}
"#;
    check_all(src).expect("quant fn with qubit array parameter should typecheck");
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
fn typecheck_accepts_phase2_ability_declarations() {
    let src = r#"
struct Token has copy, clone, send, sync {
    id: i32,
}

struct Handle has deref, drop {
    ptr: &i32,
}

impl Handle {
    fn deref(&self) &i32 {
        self.ptr
    }

    fn drop(self) {
    }
}

enum Maybe has copy, clone, drop, send, sync {
    Some(i32),
    None,
}

vector Point i32 [X, Y] has copy, clone, drop, send, sync

fn main() i32 {
    0
}
"#;
    check_all(src).expect("phase 2 ability declarations");
}

#[test]
fn typecheck_accepts_send_sync_declarations() {
    let src = r#"
struct Counter has send, sync, drop {
    hits: i32,
    label: i32,
}

enum Job has send, sync, drop {
    Run(i32),
    Idle,
}

vector Point i32 [X, Y] has copy, clone, drop, send, sync

fn main() i32 {
    0
}
"#;
    check_all(src).expect("send/sync declarations with valid fields");
}

#[test]
fn typecheck_rejects_struct_send_with_thread_field() {
    let src = r#"
struct Bundle has send, drop {
    worker: Thread,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("Thread field blocks send");
    assert!(err.contains("field `worker` does not support it"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_struct_sync_with_thread_field() {
    let src = r#"
struct Bundle has sync, drop {
    worker: Thread,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("Thread field blocks sync");
    assert!(err.contains("field `worker` does not support it"), "{err}");
    assert!(
        err.contains("Thread handles cannot be shared across threads"),
        "{err}"
    );
}

#[test]
fn typecheck_arc_rejects_non_send_sync_inner() {
    let src = r#"
fn worker() {
}

fn main() i32 {
    let shared: Arc[Thread] = arc_new(spawn worker);
    0
}
"#;
    let err = check_all(src).expect_err("Arc[Thread] must fail send/sync bounds");
    assert!(err.contains("requires inner type to be `send`"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_mutex_surface() {
    let src = include_str!("../../../examples/tests/ok_mutex_basic.nia");
    check_all(src).expect("Mutex lock/try_lock/deref/drop should typecheck");
}

#[test]
fn typecheck_mutex_rejects_non_send_inner() {
    let src = r#"
fn worker() {
}

fn main() i32 {
    let m: Mutex[Thread] = mutex_new(spawn worker);
    0
}
"#;
    let err = check_all(src).expect_err("Mutex[Thread] must fail send bound");
    assert!(err.contains("requires inner type to be `send`"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_mutex_guard_is_not_send() {
    let src = r#"
fn main() i32 {
    let m: Mutex[i32] = mutex_new(0);
    let guard: MutexGuard[i32] = m.lock();
    let t: Thread = spawn move || {
        println(*guard);
    };
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("MutexGuard capture must fail send bound");
    assert!(err.contains("capture `guard` requires `send`"), "{err}");
    assert!(
        err.contains("MutexGuard values cannot be transferred"),
        "{err}"
    );
}

#[test]
fn typecheck_rwlock_surface() {
    let src = include_str!("../../../examples/tests/ok_rwlock_read_write.nia");
    check_all(src).expect("RwLock read/write/try guards should typecheck");
}

#[test]
fn typecheck_rwlock_rejects_non_send_inner() {
    let src = r#"
fn worker() {
}

fn main() i32 {
    let rw: RwLock[Thread] = rwlock_new(spawn worker);
    0
}
"#;
    let err = check_all(src).expect_err("RwLock[Thread] must fail send bound");
    assert!(err.contains("requires inner type to be `send`"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_rwlock_guards_are_not_send() {
    let src = r#"
fn main() i32 {
    let rw: RwLock[i32] = rwlock_new(0);
    let guard: RwLockReadGuard[i32] = rw.read();
    let t: Thread = spawn move || {
        println(*guard);
    };
    join(t);
    0
}
"#;
    let err = check_all(src).expect_err("RwLockReadGuard capture must fail send bound");
    assert!(err.contains("capture `guard` requires `send`"), "{err}");
    assert!(
        err.contains("RwLock guard values cannot be transferred"),
        "{err}"
    );
}

#[test]
fn typecheck_rwlock_read_guard_is_read_only() {
    let src = r#"
fn main() i32 {
    let rw: RwLock[i32] = rwlock_new(0);
    let guard: RwLockReadGuard[i32] = rw.read();
    *guard = 1;
    drop(guard);
    drop(rw);
    0
}
"#;
    let err = check_all(src).expect_err("assignment through read guard must fail");
    assert!(
        err.contains("cannot assign through `RwLockReadGuard` dereference"),
        "{err}"
    );
}

#[test]
fn typecheck_condvar_surface() {
    let src = include_str!("../../../examples/tests/ok_condvar_wait_notify.nia");
    check_all(src).expect("Condvar wait/notify surface should typecheck");
}

#[test]
fn typecheck_condvar_wait_requires_mutex_guard() {
    let src = r#"
fn main() i32 {
    let rw: RwLock[i32] = rwlock_new(0);
    let cv: Condvar = condvar_new();
    let guard: RwLockReadGuard[i32] = rw.read();
    cv.wait(guard);
    0
}
"#;
    let err = check_all(src).expect_err("Condvar wait must reject non-mutex guard");
    assert!(err.contains("expects a MutexGuard[T]"), "{err}");
}

#[test]
fn typecheck_condvar_is_send_sync_for_arc() {
    let src = r#"
fn main() i32 {
    let cv: Arc[Condvar] = arc_new(condvar_new());
    let moved = cv.clone();
    let t: Thread = spawn move || {
        (*moved).notify_all();
    };
    join(t);
    drop(cv);
    0
}
"#;
    check_all(src).expect("Arc[Condvar] should be send + sync");
}

#[test]
fn typecheck_arc_is_move_only_but_clone_keeps_source_usable() {
    let ok = r#"
fn main() i32 {
    let shared: Arc[i32] = arc_new(1);
    let cloned = shared.clone();
    println(*shared);
    println(*cloned);
    drop(shared);
    drop(cloned);
    0
}
"#;
    check_all(ok).expect("Arc clone should be shallow and keep source usable");

    let moved = r#"
fn main() i32 {
    let shared: Arc[i32] = arc_new(1);
    let moved = shared;
    println(*shared);
    drop(moved);
    0
}
"#;
    let err = check_all(moved).expect_err("Arc assignment should move the source");
    assert!(err.contains("use of moved local `shared`"), "{err}");
    assert!(
        err.contains("Arc values are shared runtime owners"),
        "{err}"
    );
}

#[test]
fn typecheck_arc_deref_is_read_only() {
    let src = r#"
fn main() i32 {
    let shared: Arc[i32] = arc_new(1);
    *shared = 2;
    drop(shared);
    0
}
"#;
    let err = check_all(src).expect_err("Arc deref assignment should fail");
    assert!(
        err.contains("cannot assign through `Arc` dereference"),
        "{err}"
    );
}

#[test]
fn typecheck_arc_allows_atomic_inner_via_atomic_methods() {
    let src = r#"
fn main() i32 {
    let shared: Arc[AtomicI32] = arc_new(atomic_i32(0));
    let old = (*shared).fetch_add(1, Ordering::AcqRel);
    let now = (*shared).load(Ordering::Acquire);
    println(old + now);
    drop(shared);
    0
}
"#;
    check_all(src).expect("Arc[AtomicI32] should be usable through atomic methods");
}

#[test]
fn typecheck_rejects_enum_send_with_thread_payload() {
    let src = r#"
enum WorkerState has send, drop {
    Running(Thread),
    Idle,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("enum variant Thread blocks send");
    assert!(err.contains("variant `Running` field `0`"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_vector_send_with_thread_element() {
    let src = r#"
vector Handle Thread [X, Y] has drop, send

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("Thread axis blocks vector send");
    assert!(err.contains("element type does not support it"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_struct_send_when_nested_field_is_not_send() {
    let src = r#"
struct Inner has drop {
    worker: Thread,
}

struct Outer has send, drop {
    inner: Inner,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("nested Thread blocks outer send");
    assert!(err.contains("field `inner` does not support it"), "{err}");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
}

#[test]
fn typecheck_copy_types_implicitly_satisfy_send_sync_declarations() {
    let src = r#"
struct Point has copy, clone, drop, send, sync {
    x: i32,
    y: i32,
}

fn main() i32 {
    0
}
"#;
    check_all(src).expect("copy struct with send/sync declaration");
}

#[test]
fn typecheck_atomic_types_are_send_and_sync() {
    let structs = HashMap::new();
    let enums = HashMap::new();
    let vectors = HashMap::new();
    for ty in [
        Ty::AtomicBool,
        Ty::AtomicI32,
        Ty::AtomicUsize,
        Ty::AtomicPtr(Box::new(Ty::I32)),
    ] {
        assert!(
            has_formal_ability(&ty, Ability::Send, &structs, &enums, &vectors),
            "{ty:?} should be send"
        );
        assert!(
            has_formal_ability(&ty, Ability::Sync, &structs, &enums, &vectors),
            "{ty:?} should be sync"
        );
    }
    assert!(!has_formal_ability(
        &Ty::Thread,
        Ability::Send,
        &structs,
        &enums,
        &vectors
    ));
    assert!(!has_formal_ability(
        &Ty::Thread,
        Ability::Sync,
        &structs,
        &enums,
        &vectors
    ));
}

#[test]
fn typecheck_rejects_copy_without_clone_ability() {
    let src = r#"
struct Bad has copy {
    x: i32,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("copy implies clone");
    assert!(err.contains("missing required `clone` ability"), "{err}");
}

#[test]
fn typecheck_rejects_struct_ability_when_field_lacks_it() {
    let src = r#"
struct Inner {
    x: i32,
}

struct Bad has clone {
    inner: Inner,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("field lacks clone");
    assert!(err.contains("field `inner` does not support it"), "{err}");
    assert!(
        err.contains("struct `Inner` does not declare `clone`"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_enum_ability_with_variant_payload_diagnostic() {
    let src = r#"
struct Inner {
    x: i32,
}

enum Bad has clone {
    Full(Inner),
    Empty,
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("variant payload lacks clone");
    assert!(
        err.contains("variant `Full` field `0` does not support it"),
        "{err}"
    );
    assert!(
        err.contains("struct `Inner` does not declare `clone`"),
        "{err}"
    );
}

#[test]
fn typecheck_allows_derived_drop_for_runtime_handle_field() {
    let src = r#"
struct MatrixOwner has drop {
    m: f64[],
}

fn main() i32 {
    0
}
"#;
    check_all(src).expect("runtime handles can now participate in derived drop");
}

#[test]
fn typecheck_allows_derived_struct_drop_for_language_drop_fields() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

struct Pair has drop {
    first: FileHandle,
    second: FileHandle,
}

fn main() i32 {
    let pair = Pair {
        first: FileHandle { fd: 1 },
        second: FileHandle { fd: 2 }
    };
    drop(pair);
    0
}
"#;
    check_all(src).expect("derived struct drop should accept language-drop fields");
}

#[test]
fn typecheck_allows_derived_enum_drop_and_explicit_drop() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

enum Slot has drop {
    Full(FileHandle),
    Empty,
}

fn main() i32 {
    let slot = Slot::Full(FileHandle { fd: 7 });
    drop(slot);
    0
}
"#;
    check_all(src).expect("derived enum drop should be language-level drop");
}

#[test]
fn typecheck_rejects_deref_without_pointer_return() {
    let src = r#"
struct BoxI32 has deref {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) i32 {
        1
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("deref must return pointer");
    assert!(err.contains("custom deref must return"), "{err}");
}

#[test]
fn typecheck_rejects_enum_and_vector_deref_abilities() {
    let enum_src = r#"
enum Bad has deref {
    Unit,
}

fn main() i32 {
    0
}
"#;
    let enum_err = check_all(enum_src).expect_err("enum deref rejected");
    assert!(
        enum_err.contains("enum deref is not supported yet"),
        "{enum_err}"
    );

    let vector_src = r#"
vector Bad i32 [X] has deref

fn main() i32 {
    0
}
"#;
    let vector_err = check_all(vector_src).expect_err("vector deref rejected");
    assert!(
        vector_err.contains("vector deref is not supported yet"),
        "{vector_err}"
    );
}

#[test]
fn typecheck_rejects_use_after_move() {
    let src = r#"
struct Token {
    id: i32,
}

fn main() i32 {
    let a = Token { id: 1 };
    let b = a;
    a.id + b.id
}
"#;
    let err = check_all(src).expect_err("non-copy local should move");
    assert!(err.contains("use of moved local `a`"), "{err}");
    assert!(err.contains("bound into another local"), "{err}");
    assert!(
        err.contains("struct `Token` does not declare `copy`"),
        "{err}"
    );
}

#[test]
fn typecheck_copy_ability_keeps_local_available_after_assignment() {
    let src = r#"
struct Token has copy, clone {
    id: i32,
}

fn main() i32 {
    let a = Token { id: 1 };
    let b = a;
    a.id + b.id
}
"#;
    check_all(src).expect("copy values should not move on assignment");
}

#[test]
fn typecheck_allows_moving_same_value_in_if_else_branches() {
    let src = r#"
struct Token {
    id: i32,
}

fn take(t: Token) {
    println(t.id);
}

fn main(flag: bool) i32 {
    let token = Token { id: 1 };
    if flag {
        take(token);
    } else {
        take(token);
    }
    0
}
"#;
    check_all(src).expect("each if/else path should start from the same move state");
}

#[test]
fn typecheck_rejects_function_argument_use_after_move() {
    let src = r#"
struct Token {
    id: i32,
}

fn take(t: Token) i32 {
    t.id
}

fn main() i32 {
    let a = Token { id: 1 };
    let first = take(a);
    let second = take(a);
    first + second
}
"#;
    let err = check_all(src).expect_err("by-value argument should move");
    assert!(err.contains("use of moved local `a`"), "{err}");
    assert!(err.contains("passed by value"), "{err}");
    assert!(
        err.contains("struct `Token` does not declare `copy`"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_returned_local_use_after_move() {
    let src = r#"
struct Token {
    id: i32,
}

fn consume(t: Token) Token {
    return t
    t
}

fn main() i32 {
    let value = consume(Token { id: 1 });
    value.id
}
"#;
    let err = check_all(src).expect_err("returning a non-copy local should move it");
    assert!(err.contains("use of moved local `t`"), "{err}");
    assert!(err.contains("returned from the function"), "{err}");
    assert!(
        err.contains("struct `Token` does not declare `copy`"),
        "{err}"
    );
}

#[test]
fn typecheck_rejects_partial_move_out_of_struct() {
    let src = r#"
struct Token {
    id: i32,
}

struct Boxed {
    token: Token,
}

fn main() i32 {
    let boxed = Boxed { token: Token { id: 1 } };
    let token = boxed.token;
    token.id
}
"#;
    let err = check_all(src).expect_err("partial moves are not supported yet");
    assert!(err.contains("partial moves are not supported yet"), "{err}");
}

#[test]
fn typecheck_rejects_indexed_move() {
    let src = r#"
struct Token {
    id: i32,
}

fn main() i32 {
    let tokens: [Token; 1] = [Token { id: 1 }];
    let token = tokens[0];
    token.id
}
"#;
    let err = check_all(src).expect_err("indexed moves are not supported yet");
    assert!(err.contains("indexed moves are not supported yet"), "{err}");
}

#[test]
fn typecheck_allows_clone_method_for_clone_struct_without_moving_source() {
    let src = r#"
struct Token has clone {
    id: i32,
}

fn main() i32 {
    let token = Token { id: 7 };
    let cloned = token.clone();
    token.id + cloned.id
}
"#;
    check_all(src).expect("clone method should borrow source");
}

#[test]
fn typecheck_allows_clone_method_for_array_of_clone_values() {
    let src = r#"
struct Token has clone {
    id: i32,
}

fn main() i32 {
    let tokens: [Token; 2] = [Token { id: 1 }, Token { id: 2 }];
    let cloned = tokens.clone();
    tokens[0].id + cloned[1].id
}
"#;
    check_all(src).expect("arrays of clone values should support clone method");
}

#[test]
fn typecheck_allows_custom_clone_for_struct_with_non_clone_field() {
    let src = r#"
struct Handle {
    fd: i32,
}

struct Token has clone {
    handle: Handle,
}

impl Token {
    fn clone(&self) Token {
        Token {
            handle: Handle { fd: self.handle.fd + 1 }
        }
    }
}

fn main() i32 {
    let token = Token { handle: Handle { fd: 7 } };
    let cloned = token.clone();
    token.handle.fd + cloned.handle.fd
}
"#;
    check_all(src).expect("custom clone should override structural clone");
}

#[test]
fn typecheck_rejects_custom_clone_without_clone_ability() {
    let src = r#"
struct Token {
    id: i32,
}

impl Token {
    fn clone(&self) Token {
        Token { id: self.id }
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("custom clone requires clone ability");
    assert!(err.contains("does not declare `clone` ability"), "{err}");
}

#[test]
fn typecheck_rejects_custom_clone_with_by_value_self() {
    let src = r#"
struct Token has clone {
    id: i32,
}

impl Token {
    fn clone(self) Token {
        Token { id: self.id }
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("custom clone must borrow self");
    assert!(err.contains("custom clone must have signature"), "{err}");
}

#[test]
fn typecheck_rejects_custom_clone_with_wrong_return_type() {
    let src = r#"
struct Token has clone {
    id: i32,
}

impl Token {
    fn clone(&self) i32 {
        self.id
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("custom clone must return owner");
    assert!(err.contains("custom clone must return `Token`"), "{err}");
}

#[test]
fn typecheck_rejects_direct_recursive_custom_clone() {
    let src = r#"
struct Token has clone {
    id: i32,
}

impl Token {
    fn clone(&self) Token {
        self.clone()
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("direct recursive clone should be rejected");
    assert!(err.contains("recursively calls `self.clone()`"), "{err}");
}

#[test]
fn typecheck_rejects_clone_method_without_clone_ability() {
    let src = r#"
struct Token {
    id: i32,
}

fn main() i32 {
    let token = Token { id: 7 };
    let cloned = token.clone();
    cloned.id
}
"#;
    let err = check_all(src).expect_err("clone method requires clone ability");
    assert!(err.contains("requires receiver type"), "{err}");
    assert!(err.contains("support `clone`"), "{err}");
}

#[test]
fn typecheck_rejects_clone_method_arguments() {
    let src = r#"
struct Token has clone {
    id: i32,
}

fn main() i32 {
    let token = Token { id: 7 };
    let cloned = token.clone(1);
    cloned.id
}
"#;
    let err = check_all(src).expect_err("clone method takes no arguments");
    assert!(err.contains("method `clone`: expected 0 args"), "{err}");
}

#[test]
fn typecheck_allows_clone_method_for_primitives_and_runtime_handles() {
    for src in [
        r#"
fn main() i32 {
    let cloned = 1.clone();
    cloned
}
"#,
        r#"
fn main() i32 {
    let m: f64[] = matrix([[1.0]]);
    let cloned = m.clone();
    0
}
"#,
        r#"
fn main() i32 {
    let v: f64<> = <1.0, 2.0>;
    let cloned = v.clone();
    0
}
"#,
        r#"
fn main() i32 {
    let xs: List[i32] = list_new[i32]();
    let cloned = xs.clone();
    0
}
"#,
    ] {
        check_all(src).expect("primitive/runtime clone should typecheck");
    }
}

#[test]
fn typecheck_allows_derived_clone_and_drop_for_runtime_owner_fields() {
    let src = r#"
struct Owner has clone, drop {
    m: f64[],
}

fn main() i32 {
    let owner = Owner { m: matrix([[1.0]]) };
    let cloned = owner.clone();
    drop(cloned);
    drop(owner);
    0
}
"#;
    check_all(src).expect("runtime owner fields should support derived clone/drop");
}

#[test]
fn typecheck_allows_custom_deref_without_moving_source() {
    let src = r#"
struct BoxI32 has deref {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) &i32 {
        self.ptr
    }
}

fn main() i32 {
    let x = 21;
    let b = BoxI32 { ptr: &x };
    let first = *b;
    let second = *b;
    first + second
}
"#;
    check_all(src).expect("custom deref should borrow source");
}

#[test]
fn typecheck_rejects_custom_deref_without_deref_ability() {
    let src = r#"
struct BoxI32 {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) &i32 {
        self.ptr
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("custom deref requires deref ability");
    assert!(err.contains("does not declare `deref` ability"), "{err}");
}

#[test]
fn typecheck_rejects_direct_deref_method_call() {
    let src = r#"
struct BoxI32 has deref {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) &i32 {
        self.ptr
    }
}

fn main() i32 {
    let x = 1;
    let b = BoxI32 { ptr: &x };
    b.deref();
    0
}
"#;
    let err = check_all(src).expect_err("direct deref method call is not supported");
    assert!(err.contains("use `*x`"), "{err}");
}

#[test]
fn typecheck_deref_does_not_grant_clone_or_drop() {
    for src in [
        r#"
struct BoxI32 has deref {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) &i32 {
        self.ptr
    }
}

fn main() i32 {
    let x = 1;
    let b = BoxI32 { ptr: &x };
    let cloned = b.clone();
    *cloned
}
"#,
        r#"
struct BoxI32 has deref {
    ptr: &i32,
}

impl BoxI32 {
    fn deref(&self) &i32 {
        self.ptr
    }
}

fn main() i32 {
    let x = 1;
    let b = BoxI32 { ptr: &x };
    drop(b);
    0
}
"#,
    ] {
        let err = check_all(src).expect_err("deref should not imply other abilities");
        assert!(
            err.contains("support `clone`") || err.contains("support `drop`"),
            "{err}"
        );
    }
}

#[test]
fn typecheck_rejects_moving_non_copy_value_out_through_deref() {
    let src = r#"
struct Token {
    id: i32,
}

struct BoxToken has deref {
    ptr: &Token,
}

impl BoxToken {
    fn deref(&self) &Token {
        self.ptr
    }
}

fn main() i32 {
    let token = Token { id: 7 };
    let b = BoxToken { ptr: &token };
    let moved = *b;
    moved.id
}
"#;
    let err = check_all(src).expect_err("deref should not move non-copy targets out");
    assert!(err.contains("cannot move out through dereference"), "{err}");
}

#[test]
fn typecheck_allows_custom_drop_and_explicit_drop() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
        println(self.fd);
    }
}

fn main() i32 {
    let h = FileHandle { fd: 3 };
    drop(h);
    0
}
"#;
    check_all(src).expect("explicit drop should accept custom-drop structs");
}

#[test]
fn typecheck_rejects_custom_drop_without_drop_ability() {
    let src = r#"
struct FileHandle {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("custom drop requires drop ability");
    assert!(err.contains("does not declare `drop` ability"), "{err}");
}

#[test]
fn typecheck_rejects_direct_drop_method_call() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h = FileHandle { fd: 3 };
    h.drop();
    0
}
"#;
    let err = check_all(src).expect_err("direct drop method call is not supported");
    assert!(err.contains("use `drop(x)`"), "{err}");
}

#[test]
fn typecheck_explicit_drop_moves_local() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h = FileHandle { fd: 3 };
    drop(h);
    h.fd
}
"#;
    let err = check_all(src).expect_err("drop should move the local");
    assert!(err.contains("use of moved local `h`"), "{err}");
    assert!(err.contains("passed to `drop(...)`"), "{err}");
    assert!(
        err.contains("struct `FileHandle` does not declare `copy`"),
        "{err}"
    );
}

#[test]
fn typecheck_allows_language_drop_for_primitives_and_runtime_handles() {
    for src in [
        r#"
fn main() i32 {
    let n = 1;
    drop(n);
    0
}
"#,
        r#"
fn main() i32 {
    let m: f64[] = matrix([[1.0]]);
    drop(m);
    0
}
"#,
        r#"
fn main() i32 {
    let v: f64<> = <1.0>;
    drop(v);
    0
}
"#,
        r#"
fn main() i32 {
    let xs: List[i32] = list_new[i32]();
    drop(xs);
    0
}
"#,
    ] {
        check_all(src).expect("primitive/runtime drop should typecheck");
    }
}

#[test]
fn typecheck_rejects_runtime_owner_use_after_move() {
    let src = r#"
fn main() i32 {
    let m: f64[] = matrix([[1.0]]);
    let moved = m;
    matrix_rows(m)
}
"#;
    let err = check_all(src).expect_err("Matrix moves should make the source unavailable");
    assert!(err.contains("use of moved local `m`"), "{err}");
    assert!(err.contains("bound into another local"), "{err}");
    assert!(err.contains("Matrix is a runtime owner"), "{err}");
    assert!(err.contains("use `.clone()`"), "{err}");
}

#[test]
fn typecheck_allows_assigning_uninitialized_custom_drop_local() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h: FileHandle;
    h = FileHandle { fd: 3 };
    h.fd
}
"#;
    check_all(src).expect("assignment should initialize typed local");
}

#[test]
fn typecheck_rejects_use_of_uninitialized_local() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h: FileHandle;
    h.fd
}
"#;
    let err = check_all(src).expect_err("uninitialized local should not be readable");
    assert!(err.contains("use of uninitialized local `h`"), "{err}");
}

#[test]
fn typecheck_rejects_use_of_maybe_initialized_local() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main(flag: bool) i32 {
    let h: FileHandle;
    if flag {
        h = FileHandle { fd: 3 };
    }
    h.fd
}
"#;
    let err = check_all(src).expect_err("maybe-initialized local should not be readable");
    assert!(err.contains("use of maybe-initialized local `h`"), "{err}");
}

#[test]
fn typecheck_allows_conditional_init_for_scope_exit_only() {
    let src = r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main(flag: bool) i32 {
    let h: FileHandle;
    if flag {
        h = FileHandle { fd: 3 };
    }
    0
}
"#;
    check_all(src).expect("maybe-initialized custom-drop local can be left to drop flags");
}

#[test]
fn typecheck_rejects_moving_field_out_inside_custom_drop() {
    let src = r#"
struct Token {
    id: i32,
}

struct FileHandle has drop {
    token: Token,
}

impl FileHandle {
    fn drop(self) {
        let token = self.token;
        println(token.id);
    }
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("custom drop should not move fields out");
    assert!(err.contains("partial moves are not supported yet"), "{err}");
}

#[test]
fn typecheck_reserves_new_single_qubit_gate_function_names() {
    for gate in ["I", "Y", "Z", "S", "Sdg", "T", "Tdg"] {
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
    for gate in ["CNOT", "CZ", "SWAP", "CH", "CY", "CS", "CSdg", "CT", "CTdg"] {
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
fn typecheck_reserves_three_qubit_gate_function_names() {
    for gate in ["CCNOT", "CCZ", "CSWAP"] {
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
        let err = check_all(&src).expect_err("three-qubit gate is a reserved builtin name");
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
fn typecheck_reserves_controlled_rotation_gate_function_names() {
    for gate in ["CRx", "CRy", "CRz", "CR1"] {
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
        let err = check_all(&src).expect_err("controlled rotation gate is a reserved builtin name");
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
fn typecheck_reserves_q_read_function_name() {
    let src = r#"
fn q_read() i32 {
    1
}

fn main() i32 {
    0
}
"#;
    let err = check_all(src).expect_err("q_read is a reserved builtin name");
    assert!(err.contains("function name `q_read` is reserved"), "{err}");
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
fn typecheck_rejects_continue_outside_loop_fixture() {
    let src = include_str!("../../../examples/tests/err_continue_outside_loop.nia");
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
fn typecheck_rejects_send_sync_thread_field_fixture() {
    let src = include_str!("../../../examples/tests/err_send_sync_thread_field.nia");
    let err = check_all(src).expect_err("Thread in send struct should fail");
    assert!(
        err.contains("Thread handles cannot be transferred to another thread"),
        "{err}"
    );
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
    let copied: f64<> = vector_clone(v);
    println(copied);
    vector_drop(copied);
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
