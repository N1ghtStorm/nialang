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

fn emit_qir_runner(src: &str) -> String {
    let (structs, enums, fns, vectors) = Parser::new(tokenize(src)).parse_file().expect("parse");
    let (struct_map, enum_map, vector_map, fn_sigs) =
        collect_sigs(&structs, &enums, &vectors, &fns).expect("sigs");
    for f in &fns {
        check_fn(f, &struct_map, &enum_map, &vector_map, &fn_sigs).expect("typecheck");
    }
    emit_module_for_qir_runner(&structs, &enums, &vectors, &fns, &fn_sigs)
}

#[test]
fn codegen_qir_runner_prints_strings_without_printf() {
    let ll = emit_qir_runner(
        r#"
fn main() i32 {
    let s: string = "hello";
    println(s);
    if s == "hello" {
        return 0
    }
    1
}
"#,
    );
    assert!(
        ll.contains("define i32 @strcmp(ptr %a, ptr %b)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("call ptr @__quantum__rt__string_create(ptr"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("call void @__quantum__rt__message(ptr"),
        "IR:\n{ll}"
    );
    assert!(!ll.contains("@printf"), "IR:\n{ll}");
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
fn codegen_option_and_result_types_lower_as_pointers() {
    let ll = emit(
        r#"
fn pass_opt(x: Option[i32]) Option[i32] {
    x
}

fn pass_result(x: Result[Option[i32], string]) Result[Option[i32], string] {
    x
}

fn main() i32 {
    0
}
"#,
    );
    assert!(
        ll.contains("define internal ptr @pass_opt(ptr %x)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("define internal ptr @pass_result(ptr %x)"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_option_result_constructors_and_match() {
    let ll = emit(
        r#"
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
"#,
    );
    assert!(ll.contains("call ptr @malloc(i64 16)"), "IR:\n{ll}");
    assert!(ll.contains("store i32 1, ptr %"), "IR:\n{ll}");
    assert!(ll.contains("store i32 0, ptr %"), "IR:\n{ll}");
    assert!(ll.contains("getelementptr i8, ptr %"), "IR:\n{ll}");
    assert!(ll.contains("switch i32 %"), "IR:\n{ll}");
    assert!(ll.contains("call void @free(ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_builtin_ordering_enum_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let order: Ordering = Ordering::Acquire;
    0
}
"#,
    );
    assert!(
        ll.contains("%enum.Ordering = type { i32, i8, i8, i8, i8, i8 }"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("insertvalue %enum.Ordering poison, i32 1, 0"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_u32_primitive_ops() {
    let ll = emit(
        r#"
fn main() i32 {
    let a: u32 = 10;
    let b: u32 = 3;
    let div: u32 = a / b;
    let rem: u32 = a % b;
    println(div);
    println(rem);
    0
}
"#,
    );
    assert!(ll.contains("udiv i32"), "IR:\n{ll}");
    assert!(ll.contains("urem i32"), "IR:\n{ll}");
    assert!(ll.contains("@nialang.std.fmt.u32"), "IR:\n{ll}");
}

#[test]
fn codegen_atomic_bool_operations() {
    let ll = emit(
        r#"
fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    ready.store(true, Ordering::Release);
    let loaded = ready.load(Ordering::Acquire);
    let swapped = ready.swap(false, Ordering::AcqRel);
    let old_and = ready.fetch_and(true, Ordering::AcqRel);
    let old_or = ready.fetch_or(false, Ordering::AcqRel);
    let old_xor = ready.fetch_xor(true, Ordering::AcqRel);
    let changed = ready.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire);
    atomic_fence(Ordering::SeqCst);
    println(loaded);
    println(swapped);
    println(old_and);
    println(old_or);
    println(old_xor);
    println(changed);
    0
}
"#,
    );
    assert!(
        ll.contains("store atomic i1 1, ptr %ready.addr release, align 1"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("load atomic i1, ptr %ready.addr acquire, align 1"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xchg ptr %ready.addr, i1 0 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw and ptr %ready.addr, i1 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw or ptr %ready.addr, i1 0 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xor ptr %ready.addr, i1 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("cmpxchg ptr %ready.addr, i1 0, i1 1 acq_rel acquire"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("extractvalue { i1, i1 } %"), "IR:\n{ll}");
    assert!(ll.contains("fence seq_cst"), "IR:\n{ll}");
}

#[test]
fn codegen_atomic_bool_pointer_receiver() {
    let ll = emit(
        r#"
fn observe(p: &AtomicBool) bool {
    (*p).load(Ordering::Acquire)
}

fn main() i32 {
    let ready: AtomicBool = atomic_bool(false);
    let value = observe(&ready);
    println(value);
    0
}
"#,
    );
    assert!(
        ll.contains("define internal i1 @observe(ptr %p)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("load atomic i1, ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_atomic_ptr_operations() {
    let ll = emit(
        r#"
fn main() i32 {
    let a: i32 = 1;
    let b: i32 = 2;
    let slot: AtomicPtr[i32] = atomic_ptr(&a);
    let loaded = slot.load(Ordering::Acquire);
    slot.store(&b, Ordering::Release);
    let swapped = slot.swap(&a, Ordering::AcqRel);
    let changed = slot.compare_exchange(&a, &b, Ordering::AcqRel, Ordering::Acquire);
    println(*loaded);
    println(*swapped);
    println(changed);
    0
}
"#,
    );
    assert!(
        ll.contains("load atomic ptr, ptr %slot.addr acquire, align 8"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("store atomic ptr %b.addr, ptr %slot.addr release, align 8"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("atomic.ptr.swap.loop"), "IR:\n{ll}");
    assert!(ll.contains("cmpxchg ptr %slot.addr, ptr %"), "IR:\n{ll}");
    assert!(
        ll.contains("cmpxchg ptr %slot.addr, ptr %a.addr, ptr %b.addr acq_rel acquire"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("extractvalue { ptr, i1 } %"), "IR:\n{ll}");
}

#[test]
fn codegen_atomic_ptr_pointer_receiver() {
    let ll = emit(
        r#"
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
"#,
    );
    assert!(
        ll.contains("define internal ptr @observe(ptr %p)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("load atomic ptr, ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_atomic_int_operations() {
    let ll = emit(include_str!("../../../examples/tests/ok_atomic_int.nia"));
    assert!(
        ll.contains("store atomic i32 2, ptr %counter.addr release, align 4"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("load atomic i32, ptr %counter.addr acquire, align 4"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xchg ptr %counter.addr, i32 3 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw add ptr %counter.addr, i32 4 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw sub ptr %counter.addr, i32 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw and ptr %counter.addr, i32 7 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw or ptr %counter.addr, i32 8 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xor ptr %counter.addr, i32 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("cmpxchg ptr %counter.addr, i32 14, i32 1 acq_rel acquire"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("extractvalue { i32, i1 } %"), "IR:\n{ll}");
    assert!(
        ll.contains("atomicrmw add ptr %unsigned.addr, i32 5 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw sub ptr %wide.addr, i64 25 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw or ptr %index.addr, i64 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xor ptr %size.addr, i64 3 acq_rel"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_atomic_narrow_int_operations() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_atomic_narrow_int.nia"
    ));
    assert!(
        ll.contains("store atomic i8 2, ptr %tiny.addr release, align 1"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("load atomic i8, ptr %tiny.addr acquire, align 1"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xchg ptr %tiny.addr, i8 3 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw add ptr %tiny.addr, i8 4 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw sub ptr %tiny.addr, i8 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw and ptr %tiny.addr, i8 7 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw or ptr %tiny.addr, i8 8 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xor ptr %tiny.addr, i8 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("cmpxchg ptr %tiny.addr, i8 14, i8 1 acq_rel acquire"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("extractvalue { i8, i1 } %"), "IR:\n{ll}");
    assert!(
        ll.contains("atomicrmw add ptr %byte.addr, i8 5 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw sub ptr %short.addr, i16 25 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw or ptr %ushort.addr, i16 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xor ptr %wideu.addr, i64 3 acq_rel"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_atomic_i128_operations() {
    let ll = emit(include_str!("../../../examples/tests/ok_atomic_i128.nia"));
    assert!(
        ll.contains("store atomic i128 2, ptr %wide.addr release, align 16"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("load atomic i128, ptr %wide.addr acquire, align 16"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xchg ptr %wide.addr, i128 3 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw add ptr %wide.addr, i128 4 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw sub ptr %wide.addr, i128 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw and ptr %wide.addr, i128 7 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw or ptr %wide.addr, i128 8 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("atomicrmw xor ptr %wide.addr, i128 1 acq_rel"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("cmpxchg ptr %wide.addr, i128 14, i128 1 acq_rel acquire"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("extractvalue { i128, i1 } %"), "IR:\n{ll}");
    assert!(
        ll.contains("atomicrmw add ptr %unsigned.addr, i128 5 acq_rel"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_non_capturing_closure_function_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let add1: fn(i32) -> i32 = |x| x + 1;
    add1(41)
}
"#,
    );
    assert!(
        ll.contains("define internal i32 @__nia_closure_0(ptr %_nia_closure_env, i32 %x)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("insertvalue { ptr, ptr, ptr, ptr } poison, ptr @__nia_closure_0, 0"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call i32 %"), "IR:\n{ll}");
}

#[test]
fn codegen_threads_minimal_spawn_join_and_drop() {
    let ll = emit(
        r#"
fn worker() {
    println(1);
}

fn detached_worker() {
    println(2);
}

fn main() i32 {
    let joined: Thread = spawn worker;
    join(joined);

    let detached: Thread = spawn detached_worker;
    0
}
"#,
    );
    assert!(
        ll.contains("declare i32 @pthread_create(ptr, ptr, ptr, ptr)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("define ptr @nialang.thread.entry(ptr %arg)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call i32 @pthread_create(ptr %"), "IR:\n{ll}");
    assert!(ll.contains("pthread_join"), "IR:\n{ll}");
    assert!(ll.contains("call i32 @pthread_detach(ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_capturing_closure_function_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let base: i32 = 10;
    let add_base: fn(i32) -> i32 = |x| x + base;
    add_base(32)
}
"#,
    );
    assert!(
        ll.contains("define internal i32 @__nia_closure_0(ptr %_nia_closure_env, i32 %x)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call ptr @malloc"), "IR:\n{ll}");
    assert!(
        ll.contains("getelementptr inbounds { i32 }, ptr %"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call i32 %"), "IR:\n{ll}");
}

#[test]
fn codegen_move_closure_env_drop_glue() {
    let ll = emit(
        r#"
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
"#,
    );
    assert!(
        ll.contains("define internal void @__nia_closure_0_drop(ptr %env)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call void @FileHandle__drop"), "IR:\n{ll}");
    assert!(ll.contains("call void @free(ptr %env)"), "IR:\n{ll}");
    assert!(
        ll.contains("insertvalue { ptr, ptr, ptr, ptr } %"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_cloneable_capturing_closure_env_clone_glue() {
    let ll = emit(
        r#"
fn main() i32 {
    let base: i32 = 10;
    let add_base: fn(i32) -> i32 = |x| x + base;
    let cloned: fn(i32) -> i32 = add_base.clone();
    add_base(1) + cloned(2)
}
"#,
    );
    assert!(
        ll.contains("define internal ptr @__nia_closure_0_clone(ptr %env)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("extractvalue { ptr, ptr, ptr, ptr } %"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_nested_capturing_closure_function_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let base: i32 = 10;
    let outer: fn(i32) -> i32 = |x| {
        let inner: fn(i32) -> i32 = |y| y + base;
        inner(x)
    };
    outer(32)
}
"#,
    );
    assert!(ll.contains("@__nia_closure_0"), "IR:\n{ll}");
    assert!(ll.contains("@__nia_closure_1"), "IR:\n{ll}");
    assert!(ll.contains("call ptr @malloc"), "IR:\n{ll}");
    assert!(
        ll.contains("getelementptr inbounds { i32 }, ptr %"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_unit_return_closure_function_value() {
    let ll = emit(
        r#"
fn main() i32 {
    let print_i32: fn(i32) -> () = |x| println(x);
    print_i32(7);
    0
}
"#,
    );
    assert!(
        ll.contains("define internal void @__nia_closure_0(ptr %_nia_closure_env, i32 %x)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call void %"), "IR:\n{ll}");
}

#[test]
fn codegen_bitwise_shift_and_remainder_instructions() {
    let ll = emit(include_str!("../../../examples/tests/ok_bitwise.nia"));
    for instruction in [
        " and i32 ",
        " or i32 ",
        " xor i32 ",
        " shl i32 ",
        " ashr i32 ",
        " lshr i8 ",
        " srem i32 ",
        " urem i8 ",
    ] {
        assert!(
            ll.contains(instruction),
            "missing `{instruction}` in IR:\n{ll}"
        );
    }
}

#[test]
fn codegen_crypto_merkle_builtins_call_runtime_helpers() {
    let ll = emit(
        r#"
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
"#,
    );
    for expected in [
        "define void @nialang.crypto.sha256(",
        "call void @nialang.crypto.merkle_root_from_data(",
        "call void @nialang.crypto.merkle_leaf_hash(",
        "call void @nialang.crypto.merkle_node_hash(",
        "call i1 @nialang.crypto.digest_eq(",
        "call i1 @nialang.crypto.merkle_verify(",
    ] {
        assert!(ll.contains(expected), "missing `{expected}` in IR:\n{ll}");
    }
}

#[test]
fn codegen_complex_std_surface() {
    let ll = emit(
        r#"
fn main() f64 {
    let z = complex(1.0, 2.0);
    let w = Complex { re: 3.0, im: 4.0 };
    let q = complex_div(complex_mul(complex_add(z, w), cis(PI)), complex_scale(w, 2.0));
    println(q);
    sin(q.re) + cos(q.im)
}
"#,
    );
    assert!(
        ll.contains("%struct.Complex = type { double, double }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("declare double @sin(double)"), "IR:\n{ll}");
    assert!(ll.contains("declare double @cos(double)"), "IR:\n{ll}");
    assert!(ll.contains("call double @sin(double"), "IR:\n{ll}");
    assert!(ll.contains("call double @cos(double"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue %struct.Complex"), "IR:\n{ll}");
    assert!(ll.contains("extractvalue %struct.Complex"), "IR:\n{ll}");
    assert!(ll.contains("fdiv double"), "IR:\n{ll}");
}

#[test]
fn codegen_clone_method_for_clone_struct_uses_value_copy() {
    let ll = emit(
        r#"
struct Token has clone {
    id: i32,
}

fn main() i32 {
    let token = Token { id: 7 };
    let cloned = token.clone();
    token.id + cloned.id
}
"#,
    );
    assert!(ll.contains("%struct.Token = type { i32 }"), "IR:\n{ll}");
    assert!(ll.contains("load %struct.Token"), "IR:\n{ll}");
    assert!(ll.contains("store %struct.Token"), "IR:\n{ll}");
    assert!(!ll.contains("Token__clone"), "IR:\n{ll}");
}

#[test]
fn codegen_clone_method_for_array_of_clone_structs() {
    let ll = emit(
        r#"
struct Token has clone {
    id: i32,
}

fn main() i32 {
    let tokens: [Token; 2] = [Token { id: 1 }, Token { id: 2 }];
    let cloned = tokens.clone();
    tokens[0].id + cloned[1].id
}
"#,
    );
    assert!(ll.contains("[2 x %struct.Token]"), "IR:\n{ll}");
    assert!(ll.contains("load [2 x %struct.Token]"), "IR:\n{ll}");
    assert!(!ll.contains("Token__clone"), "IR:\n{ll}");
}

#[test]
fn codegen_clone_method_calls_custom_struct_clone() {
    let ll = emit(
        r#"
struct Token has clone {
    id: i32,
}

impl Token {
    fn clone(&self) Token {
        Token { id: self.id + 1 }
    }
}

fn main() i32 {
    let token = Token { id: 7 };
    let cloned = token.clone();
    cloned.id
}
"#,
    );
    assert!(
        ll.contains("define internal %struct.Token @Token__clone(ptr %self)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("call %struct.Token @Token__clone(ptr %"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_custom_deref_loads_through_deref_method() {
    let ll = emit(
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
    let x = 41;
    let b = BoxI32 { ptr: &x };
    *b
}
"#,
    );
    assert!(
        ll.contains("define internal ptr @BoxI32__deref(ptr %self)"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call ptr @BoxI32__deref(ptr %"), "IR:\n{ll}");
    assert!(ll.contains("load i32, ptr %"), "IR:\n{ll}");
}

#[test]
fn codegen_explicit_drop_calls_custom_struct_drop() {
    let ll = emit(
        r#"
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
"#,
    );
    assert!(
        ll.contains("define internal void @FileHandle__drop(%struct.FileHandle %self)"),
        "IR:\n{ll}"
    );
    assert!(
        ll.contains("call void @FileHandle__drop(%struct.FileHandle %"),
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_auto_drops_custom_drop_local_on_scope_exit() {
    let ll = emit(
        r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h = FileHandle { fd: 3 };
    0
}
"#,
    );
    assert!(
        ll.contains("define internal void @FileHandle__drop(%struct.FileHandle %self)"),
        "IR:\n{ll}"
    );
    assert_eq!(
        ll.matches("call void @FileHandle__drop").count(),
        1,
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_auto_drops_custom_drop_local_before_return() {
    let ll = emit(
        r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h = FileHandle { fd: 3 };
    return 7;
    0
}
"#,
    );
    let drop_pos = ll.find("call void @FileHandle__drop").expect("drop call");
    let ret_pos = ll.find("ret i32 7").expect("return");
    assert!(drop_pos < ret_pos, "IR:\n{ll}");
}

#[test]
fn codegen_auto_drops_loop_body_local_before_break() {
    let ll = emit(
        r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    loop {
        let h = FileHandle { fd: 3 };
        break;
    }
    0
}
"#,
    );
    assert!(ll.contains("call void @FileHandle__drop"), "IR:\n{ll}");
    let drop_pos = ll.find("call void @FileHandle__drop").expect("drop call");
    let exit_branch_pos = ll[drop_pos..]
        .find("br label %loop.exit")
        .map(|idx| drop_pos + idx)
        .expect("break branch");
    assert!(drop_pos < exit_branch_pos, "IR:\n{ll}");
}

#[test]
fn codegen_conditional_custom_drop_init_uses_drop_flag() {
    let ll = emit(
        r#"
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
"#,
    );
    assert!(ll.contains("%h.drop = alloca i1"), "IR:\n{ll}");
    assert!(ll.contains("store i1 false, ptr %h.drop"), "IR:\n{ll}");
    assert!(ll.contains("store i1 true, ptr %h.drop"), "IR:\n{ll}");
    assert!(ll.contains("call void @FileHandle__drop"), "IR:\n{ll}");
}

#[test]
fn codegen_overwrite_drops_old_custom_drop_value() {
    let ll = emit(
        r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

fn main() i32 {
    let h = FileHandle { fd: 3 };
    h = FileHandle { fd: 4 };
    0
}
"#,
    );
    assert!(
        ll.matches("call void @FileHandle__drop").count() >= 2,
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_derived_struct_drop_drops_language_drop_fields() {
    let ll = emit(
        r#"
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
    0
}
"#,
    );
    assert!(!ll.contains("@Pair__drop"), "IR:\n{ll}");
    assert!(ll.contains("%pair.drop = alloca i1"), "IR:\n{ll}");
    assert_eq!(
        ll.matches("call void @FileHandle__drop").count(),
        2,
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_derived_enum_drop_switches_on_active_payload() {
    let ll = emit(
        r#"
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
    0
}
"#,
    );
    assert!(ll.contains("%slot.drop = alloca i1"), "IR:\n{ll}");
    assert!(ll.contains("switch i32"), "IR:\n{ll}");
    assert_eq!(
        ll.matches("call void @FileHandle__drop").count(),
        1,
        "IR:\n{ll}"
    );
}

#[test]
fn codegen_custom_drop_runs_before_language_drop_fields() {
    let ll = emit(
        r#"
struct FileHandle has drop {
    fd: i32,
}

impl FileHandle {
    fn drop(self) {
    }
}

struct Owner has drop {
    handle: FileHandle,
}

impl Owner {
    fn drop(self) {
        println(99);
    }
}

fn main() i32 {
    let owner = Owner {
        handle: FileHandle { fd: 3 }
    };
    0
}
"#,
    );
    let owner_drop = ll.find("call void @Owner__drop").expect("owner drop call");
    let field_drop = ll
        .find("call void @FileHandle__drop")
        .expect("field drop call");
    assert!(owner_drop < field_drop, "IR:\n{ll}");
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
fn codegen_heap_anon_vector_uses_unique_header_and_len() {
    let src = r#"
fn main() i32 {
    let v: f64<> = <1.0, 2.0, 3.0>;
    println(vector_len(v));
    println(len(v));
    let copied: f64<> = vector_clone(v);
    vector_set(copied, 1, 9.0);
    println(vector_get(v, 1));
    println(vector_get(copied, 1));
    println(v);
    vector_drop(copied);
    vector_drop(v);
    0
}
"#;
    let ll = emit(src);
    assert!(
        ll.contains("getelementptr inbounds { ptr, i64 }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call ptr @malloc(i64 16)"), "IR:\n{ll}");
    assert!(ll.contains("heap.vector.clone.loop"), "IR:\n{ll}");
    assert!(ll.contains("println.heap.vector.cond"), "IR:\n{ll}");
}

#[test]
fn codegen_list_alloc_len_capacity_and_push() {
    let src = r#"
fn main() i32 {
    let bytes = list_new[u8]();
    bytes.push(10);
    bytes.push(20);
    println(bytes.get(1));

    let zs = list_with_capacity[Complex](2);
    zs.push(complex(1.0, 0.0));
    println(zs.get(0));

    bytes.len() + bytes.capacity() + zs.len() + zs.capacity()
}
"#;
    let ll = emit(src);
    assert!(
        ll.contains("getelementptr inbounds { ptr, i64, i64 }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call ptr @malloc(i64 24)"), "IR:\n{ll}");
    assert!(ll.contains("call ptr @realloc(ptr"), "IR:\n{ll}");
    assert!(ll.contains("store i8"), "IR:\n{ll}");
    assert!(ll.contains("store %struct.Complex"), "IR:\n{ll}");
    assert!(ll.contains("list.get.bounds.ok"), "IR:\n{ll}");
    assert!(ll.contains("trunc i64"), "IR:\n{ll}");
    assert!(ll.contains("list.push.grow"), "IR:\n{ll}");
}

#[test]
fn codegen_matrix_clone_method_and_language_drop_deep_copy_and_free() {
    let src = r#"
fn main() i32 {
    let m: f64[] = matrix([[1.0, 2.0]]);
    let shared = m.clone();
    drop(shared);
    drop(m);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("matrix.clone.cond"), "IR:\n{ll}");
    assert!(
        ll.contains("getelementptr inbounds { ptr, i64, i64 }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call void @free(ptr"), "IR:\n{ll}");
}

#[test]
fn codegen_heap_vector_clone_method_and_language_drop_deep_copy_and_free() {
    let src = r#"
fn main() i32 {
    let v: i32<> = <1, 2, 3>;
    let shared = v.clone();
    drop(shared);
    drop(v);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("heap.vector.clone.loop"), "IR:\n{ll}");
    assert!(
        ll.contains("getelementptr inbounds { ptr, i64 }"),
        "IR:\n{ll}"
    );
    assert!(ll.contains("call void @free(ptr"), "IR:\n{ll}");
}

#[test]
fn codegen_list_clone_and_language_drop_allocate_and_free() {
    let src = r#"
struct Token has clone, drop {
    id: i32,
}

impl Token {
    fn drop(self) {
    }
}

fn main() i32 {
    let xs = list_new[Token]();
    xs.push(Token { id: 1 });
    xs.push(Token { id: 2 });
    let ys = xs.clone();
    drop(ys);
    drop(xs);
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("list.clone.loop"), "IR:\n{ll}");
    assert!(ll.contains("list.drop.loop"), "IR:\n{ll}");
    assert!(
        ll.matches("call ptr @malloc(i64 24)").count() >= 2,
        "IR:\n{ll}"
    );
    assert!(ll.contains("call void @free(ptr"), "IR:\n{ll}");
}

#[test]
fn codegen_derived_struct_drop_drops_runtime_handle_field() {
    let src = r#"
struct Owner has drop {
    m: f64[],
}

fn main() i32 {
    let owner = Owner { m: matrix([[1.0]]) };
    0
}
"#;
    let ll = emit(src);
    assert!(ll.contains("%owner.drop = alloca i1"), "IR:\n{ll}");
    assert!(ll.contains("call void @free(ptr"), "IR:\n{ll}");
}

#[test]
fn codegen_derived_struct_clone_clones_runtime_handle_field() {
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
    let ll = emit(src);
    assert!(ll.contains("extractvalue %struct.Owner"), "IR:\n{ll}");
    assert!(ll.contains("matrix.clone.cond"), "IR:\n{ll}");
    assert!(ll.contains("call void @free(ptr"), "IR:\n{ll}");
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
fn codegen_array_matrix_conversions_copy_between_array_and_matrix_storage() {
    let ll = emit(include_str!(
        "../../../examples/tests/ok_array_matrix_conversions.nia"
    ));
    assert!(ll.contains("call ptr @malloc(i64 24)"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [3 x i32]"), "IR:\n{ll}");
    assert!(ll.contains("insertvalue [2 x [3 x i32]]"), "IR:\n{ll}");
    assert!(ll.contains("getelementptr inbounds i32"), "IR:\n{ll}");
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
fn codegen_logical_not_emits_bool_xor() {
    let ll = emit(
        r#"
fn main() i32 {
    let value: bool = !false;
    if !value {
        return 1
    }
    0
}
"#,
    );
    assert!(ll.contains("xor i1"), "IR:\n{ll}");
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
fn codegen_matrix_box_helpers_present() {
    let ll = emit(include_str!("../../../examples/sample_matrix_rc.nia"));
    assert!(ll.contains("call ptr @malloc"), "IR:\n{ll}");
    assert!(ll.contains("println.matrix.row.cond"), "IR:\n{ll}");
    assert!(ll.contains("println.matrix.col.cond"), "IR:\n{ll}");
    assert!(
        ll.contains("getelementptr inbounds { ptr, i64, i64 }"),
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
