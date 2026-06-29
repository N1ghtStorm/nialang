use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;
use std::rc::Rc;

use crate::ast::{
    Ability, Block, EnumDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt, StructDef, Ty,
    VectorDef, method_symbol,
};
use crate::nia_std::{
    ALLOC, ARC_NEW, ARC_TYPE, ATOMIC_BOOL, ATOMIC_FENCE, ATOMIC_I8, ATOMIC_I8_TYPE, ATOMIC_I16,
    ATOMIC_I16_TYPE, ATOMIC_I32, ATOMIC_I32_TYPE, ATOMIC_I64, ATOMIC_I64_TYPE, ATOMIC_I128,
    ATOMIC_I128_TYPE, ATOMIC_ISIZE, ATOMIC_ISIZE_TYPE, ATOMIC_PTR, ATOMIC_U8, ATOMIC_U8_TYPE,
    ATOMIC_U16, ATOMIC_U16_TYPE, ATOMIC_U32, ATOMIC_U32_TYPE, ATOMIC_U64, ATOMIC_U64_TYPE,
    ATOMIC_U128, ATOMIC_U128_TYPE, ATOMIC_USIZE, ATOMIC_USIZE_TYPE, AtomicOrdering, CIS,
    COMPLEX_ADD, COMPLEX_DIV, COMPLEX_MUL, COMPLEX_NEW, COMPLEX_SCALE, COMPLEX_SUB, COS, DEALLOC,
    DIGEST_EQ, GATE_CCNOT, GATE_CCZ, GATE_CH, GATE_CNOT, GATE_CR1, GATE_CRX, GATE_CRY, GATE_CRZ,
    GATE_CS, GATE_CSDG, GATE_CSWAP, GATE_CT, GATE_CTDG, GATE_CY, GATE_CZ, GATE_H, GATE_I, GATE_R1,
    GATE_RX, GATE_RY, GATE_RZ, GATE_S, GATE_SDG, GATE_SWAP, GATE_T, GATE_TDG, GATE_X, GATE_Y,
    GATE_Z, JOIN, LEN, LIST_CAPACITY, LIST_GET, LIST_LEN, LIST_NEW, LIST_PUSH, LIST_WITH_CAPACITY,
    MATRIX_CLONE, MATRIX_COLS, MATRIX_DROP, MATRIX_GET, MATRIX_LEN, MATRIX_NEW, MATRIX_ROWS,
    MATRIX_SET, MEASURE, MERKLE_LEAF_HASH, MERKLE_NODE_HASH, MERKLE_ROOT, MERKLE_ROOT_FROM_DATA,
    MERKLE_VERIFY, MUTEX_LOCK, MUTEX_NEW, MUTEX_TRY_LOCK, MUTEX_TYPE, OPTION_NONE, OPTION_SOME,
    OPTION_TYPE, OUTER, PI, PRINTLN, QUBIT, READ, REALLOC, RECORD, RESULT_ERR, RESULT_OK,
    RESULT_TYPE, SHA256, SIN, THREAD_TYPE, TO_ARRAY, TO_MATRIX, TO_VEC, VECTOR_CLONE, VECTOR_DROP,
    VECTOR_GET, VECTOR_LEN, VECTOR_SET,
};
use crate::semantics::typecheck::{FnSig, closure_capture_names};

const CLONE_METHOD: &str = "clone";
const DROP_METHOD: &str = "drop";
const DEREF_METHOD: &str = "deref";
const ATOMIC_LOAD_METHOD: &str = "load";
const ATOMIC_STORE_METHOD: &str = "store";
const ATOMIC_SWAP_METHOD: &str = "swap";
const ATOMIC_COMPARE_EXCHANGE_METHOD: &str = "compare_exchange";
const ATOMIC_FETCH_ADD_METHOD: &str = "fetch_add";
const ATOMIC_FETCH_SUB_METHOD: &str = "fetch_sub";
const ATOMIC_FETCH_AND_METHOD: &str = "fetch_and";
const ATOMIC_FETCH_OR_METHOD: &str = "fetch_or";
const ATOMIC_FETCH_XOR_METHOD: &str = "fetch_xor";
const CLOSURE_ENV_PARAM: &str = "\0nia.closure.env";

/// Walks all functions and collects UTF-8 string literal payloads for module-level globals.
fn collect_string_literals_expr(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::String(s) => {
            out.insert(s.clone());
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Ident(_)
        | Expr::Spawn { .. }
        | Expr::EnumVariant { .. } => {}
        Expr::SpawnClosure { closure } => collect_string_literals_expr(closure, out),
        Expr::Neg(inner)
        | Expr::Not(inner)
        | Expr::BitNot(inner)
        | Expr::AddrOf(inner)
        | Expr::Deref(inner)
        | Expr::Field(inner, _) => collect_string_literals_expr(inner, out),
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::VecDot(a, b)
        | Expr::Div(a, b)
        | Expr::Rem(a, b)
        | Expr::BitAnd(a, b)
        | Expr::BitOr(a, b)
        | Expr::BitXor(a, b)
        | Expr::Shl(a, b)
        | Expr::Shr(a, b)
        | Expr::Eq(a, b)
        | Expr::Ne(a, b)
        | Expr::Lt(a, b)
        | Expr::Le(a, b)
        | Expr::Gt(a, b)
        | Expr::Ge(a, b) => {
            collect_string_literals_expr(a, out);
            collect_string_literals_expr(b, out);
        }
        Expr::Call { args, .. } | Expr::GenericCall { args, .. } => {
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::CallExpr { callee, args } => {
            collect_string_literals_expr(callee, out);
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::Closure { body, .. } => {
            collect_string_literals_block(body, out);
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_string_literals_expr(receiver, out);
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::StructLit { fields, .. } | Expr::VectorLit { fields, .. } => {
            for (_, fe) in fields {
                collect_string_literals_expr(fe, out);
            }
        }
        Expr::AnonVectorLit(elems) => {
            for elem in elems {
                collect_string_literals_expr(elem, out);
            }
        }
        Expr::ArrayLit(elems) => {
            for elem in elems {
                collect_string_literals_expr(elem, out);
            }
        }
        Expr::EnumTuple { args, .. } => {
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::EnumStruct { fields, .. } => {
            for (_, fe) in fields {
                collect_string_literals_expr(fe, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_string_literals_expr(scrutinee, out);
            for (_, ae) in arms {
                collect_string_literals_expr(ae, out);
            }
        }
        Expr::Quant { body } => collect_string_literals_block(body, out),
        Expr::Gpu { body } => collect_string_literals_block(body, out),
        Expr::Index(a, i) => {
            collect_string_literals_expr(a, out);
            collect_string_literals_expr(i, out);
        }
    }
}

fn collect_string_literals_block(b: &Block, out: &mut BTreeSet<String>) {
    for st in &b.stmts {
        collect_string_literals_stmt(st, out);
    }
    if let Some(e) = &b.tail {
        collect_string_literals_expr(e, out);
    }
}

fn collect_string_literals_stmt(st: &Stmt, out: &mut BTreeSet<String>) {
    match st {
        Stmt::Let {
            init: Some(init), ..
        } => collect_string_literals_expr(init, out),
        Stmt::Let { init: None, .. } => {}
        Stmt::Expr(e) | Stmt::Return(e) => collect_string_literals_expr(e, out),
        Stmt::Assign { target, value } => {
            collect_string_literals_expr(target, out);
            collect_string_literals_expr(value, out);
        }
        Stmt::If { cond, then_block } => {
            collect_string_literals_expr(cond, out);
            collect_string_literals_block(then_block, out);
        }
        Stmt::While { cond, body } => {
            collect_string_literals_expr(cond, out);
            collect_string_literals_block(body, out);
        }
        Stmt::Loop { body } => collect_string_literals_block(body, out),
        Stmt::For {
            start, end, body, ..
        } => {
            collect_string_literals_expr(start, out);
            collect_string_literals_expr(end, out);
            collect_string_literals_block(body, out);
        }
        Stmt::Quant { body } => collect_string_literals_block(body, out),
        Stmt::Gpu { body } => collect_string_literals_block(body, out),
        Stmt::Break => {}
    }
}

/// Returns `(literal -> @symbol)` and LLVM IR for private string globals (NUL-terminated).
fn build_string_literal_section(fns: &[FnDef]) -> (HashMap<String, String>, String) {
    let mut set = BTreeSet::new();
    for f in fns {
        collect_string_literals_block(&f.body, &mut set);
    }
    let mut map = HashMap::new();
    let mut ir = String::new();
    for (i, s) in set.into_iter().enumerate() {
        let sym = format!("nialang.strlit.{i}");
        map.insert(s.clone(), sym.clone());
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        let lit = llvm_c_escape(&bytes);
        let _ = writeln!(
            ir,
            "@{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1",
            sym,
            bytes.len(),
            lit
        );
    }
    if !ir.is_empty() {
        ir.push('\n');
    }
    (map, ir)
}

/// Emits complete textual LLVM module from already-validated AST.
///
/// ## Preconditions
/// Input program is expected to be successfully typechecked; codegen uses that
/// invariant and contains `unreachable!` branches for impossible typed states.
///
/// ## Emission order
/// 1. Target header.
/// 2. Std prelude (`printf` declaration and text/format globals).
/// 3. Struct LLVM type declarations.
/// 4. Function definitions.
pub fn emit_module(
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
) -> String {
    emit_module_with_mode(structs, enums, vectors, fns, fn_sigs, CodegenMode::Default)
}

pub fn emit_module_for_qir_runner(
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
) -> String {
    emit_module_with_mode(
        structs,
        enums,
        vectors,
        fns,
        fn_sigs,
        CodegenMode::QirRunner,
    )
}

fn emit_module_with_mode(
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    mode: CodegenMode,
) -> String {
    let mut all_structs = crate::nia_std::builtin_structs();
    all_structs.extend_from_slice(structs);
    let mut all_enums = crate::nia_std::builtin_enums();
    all_enums.extend_from_slice(enums);

    let mut out = String::new();
    out.push_str("; generated by nialang\n");
    out.push_str("target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\"\n");
    out.push_str("target triple = \"unknown-unknown-unknown\"\n\n");
    if mode == CodegenMode::Default {
        out.push_str(&crate::nia_std::llvm_prelude());
    } else {
        out.push_str(qir_runner_prelude());
    }
    let (str_lit_syms, str_lit_ir) = build_string_literal_section(fns);
    out.push_str(&str_lit_ir);
    out.push_str(&emit_struct_print_constants(&all_structs));
    out.push_str(&emit_vector_print_constants(vectors));
    out.push_str(&emit_enum_print_constants(&all_enums));

    for s in &all_structs {
        out.push_str(&struct_type_decl(s));
        out.push('\n');
    }
    for v in vectors {
        // Vector literals are lowered as `%struct.<name>` aggregates.
        // Emit matching concrete LLVM type declarations for all vectors.
        out.push_str(&vector_type_decl(v, &all_structs));
        out.push('\n');
    }
    for e in &all_enums {
        out.push_str(&enum_type_decl(e, &all_structs));
        out.push('\n');
    }
    if !all_structs.is_empty() || !all_enums.is_empty() {
        out.push('\n');
    }

    let closures = Rc::new(RefCell::new(ClosureState::default()));
    for f in fns {
        out.push_str(&emit_fn(
            f,
            &all_structs,
            &all_enums,
            vectors,
            fn_sigs,
            &str_lit_syms,
            mode,
            closures.clone(),
        ));
        out.push('\n');
    }
    let closure_defs = std::mem::take(&mut closures.borrow_mut().defs);
    for def in closure_defs {
        out.push_str(&def);
        out.push('\n');
    }
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CodegenMode {
    Default,
    QirRunner,
}

#[derive(Default)]
struct ClosureState {
    next_id: u32,
    defs: Vec<String>,
    wrappers: BTreeSet<String>,
}

fn qir_runner_prelude() -> &'static str {
    r#"declare ptr @__quantum__rt__string_create(ptr)
declare void @__quantum__rt__message(ptr)
declare void @__quantum__rt__bool_record_output(i1, ptr)
declare void @__quantum__rt__int_record_output(i64, ptr)
declare void @__quantum__rt__double_record_output(double, ptr)
declare ptr @__quantum__rt__memory_allocate(i64)
declare ptr @__quantum__rt__qubit_allocate()
declare void @__quantum__rt__qubit_release(ptr)
declare void @__quantum__qis__mz__body(ptr, ptr)
declare i1 @__quantum__rt__read_result(ptr)
declare void @__quantum__rt__result_record_output(ptr, ptr)
declare void @__quantum__qis__h__body(ptr)
declare void @__quantum__qis__x__body(ptr)
declare void @__quantum__qis__y__body(ptr)
declare void @__quantum__qis__z__body(ptr)
declare void @__quantum__qis__s__body(ptr)
declare void @__quantum__qis__t__body(ptr)
declare void @__quantum__qis__cnot__body(ptr, ptr)
declare void @__quantum__qis__cz__body(ptr, ptr)
declare void @__quantum__qis__swap__body(ptr, ptr)
declare void @__quantum__qis__rx__body(double, ptr)
declare void @__quantum__qis__ry__body(double, ptr)
declare void @__quantum__qis__rz__body(double, ptr)
declare void @llvm.trap()

define ptr @malloc(i64 %size) {
entry:
  %p = call ptr @__quantum__rt__memory_allocate(i64 %size)
  ret ptr %p
}

define void @free(ptr %ptr) {
entry:
  ret void
}

define ptr @realloc(ptr %ptr, i64 %size) {
entry:
  %p = call ptr @__quantum__rt__memory_allocate(i64 %size)
  ret ptr %p
}

define void @abort() {
entry:
  call void @llvm.trap()
  unreachable
}

define double @sin(double %x) {
entry:
  %x2 = fmul double %x, %x
  %x3 = fmul double %x2, %x
  %x5a = fmul double %x3, %x2
  %x7a = fmul double %x5a, %x2
  %x9a = fmul double %x7a, %x2
  %t1 = fdiv double %x3, 6.00000000000000000e0
  %t2 = fsub double %x, %t1
  %t3 = fdiv double %x5a, 1.20000000000000000e2
  %t4 = fadd double %t2, %t3
  %t5 = fdiv double %x7a, 5.04000000000000000e3
  %t6 = fsub double %t4, %t5
  %t7 = fdiv double %x9a, 3.62880000000000000e5
  %t8 = fadd double %t6, %t7
  ret double %t8
}

define double @cos(double %x) {
entry:
  %x2 = fmul double %x, %x
  %x4a = fmul double %x2, %x2
  %x6a = fmul double %x4a, %x2
  %x8a = fmul double %x6a, %x2
  %t1 = fdiv double %x2, 2.00000000000000000e0
  %t2 = fsub double 1.00000000000000000e0, %t1
  %t3 = fdiv double %x4a, 2.40000000000000000e1
  %t4 = fadd double %t2, %t3
  %t5 = fdiv double %x6a, 7.20000000000000000e2
  %t6 = fsub double %t4, %t5
  %t7 = fdiv double %x8a, 4.03200000000000000e4
  %t8 = fadd double %t6, %t7
  ret double %t8
}

@nialang.std.txt.arr_open = private unnamed_addr constant [2 x i8] c"[\00", align 1
@nialang.std.txt.arr_sep = private unnamed_addr constant [3 x i8] c", \00", align 1
@nialang.std.txt.arr_close = private unnamed_addr constant [2 x i8] c"]\00", align 1
@nialang.std.txt.arr_close_ln = private unnamed_addr constant [3 x i8] c"]\0A\00", align 1
@nialang.std.txt.obj_open = private unnamed_addr constant [2 x i8] c"{\00", align 1
@nialang.std.txt.obj_close = private unnamed_addr constant [2 x i8] c"}\00", align 1
@nialang.std.txt.obj_close_ln = private unnamed_addr constant [3 x i8] c"}\0A\00", align 1
@nialang.std.txt.tuple_open = private unnamed_addr constant [2 x i8] c"(\00", align 1
@nialang.std.txt.tuple_close = private unnamed_addr constant [2 x i8] c")\00", align 1
@nialang.std.txt.tuple_close_ln = private unnamed_addr constant [3 x i8] c")\0A\00", align 1
@nialang.std.txt.anonvec.close = private unnamed_addr constant [3 x i8] c"})\00", align 1
@nialang.std.txt.anonvec.close_ln = private unnamed_addr constant [4 x i8] c"})\0A\00", align 1
@nialang.std.txt.anonvec.open.i8 = private unnamed_addr constant [5 x i8] c"(i8{\00", align 1
@nialang.std.txt.anonvec.open.u8 = private unnamed_addr constant [5 x i8] c"(u8{\00", align 1
@nialang.std.txt.anonvec.open.i16 = private unnamed_addr constant [6 x i8] c"(i16{\00", align 1
@nialang.std.txt.anonvec.open.u16 = private unnamed_addr constant [6 x i8] c"(u16{\00", align 1
@nialang.std.txt.anonvec.open.i32 = private unnamed_addr constant [6 x i8] c"(i32{\00", align 1
@nialang.std.txt.anonvec.open.u32 = private unnamed_addr constant [6 x i8] c"(u32{\00", align 1
@nialang.std.txt.anonvec.open.i64 = private unnamed_addr constant [6 x i8] c"(i64{\00", align 1
@nialang.std.txt.anonvec.open.u64 = private unnamed_addr constant [6 x i8] c"(u64{\00", align 1
@nialang.std.txt.anonvec.open.i128 = private unnamed_addr constant [7 x i8] c"(i128{\00", align 1
@nialang.std.txt.anonvec.open.isize = private unnamed_addr constant [8 x i8] c"(isize{\00", align 1
@nialang.std.txt.anonvec.open.usize = private unnamed_addr constant [8 x i8] c"(usize{\00", align 1
@nialang.std.txt.anonvec.open.u128 = private unnamed_addr constant [7 x i8] c"(u128{\00", align 1
@nialang.std.txt.anonvec.open.f16 = private unnamed_addr constant [6 x i8] c"(f16{\00", align 1
@nialang.std.txt.anonvec.open.f32 = private unnamed_addr constant [6 x i8] c"(f32{\00", align 1
@nialang.std.txt.anonvec.open.f64 = private unnamed_addr constant [6 x i8] c"(f64{\00", align 1
@nialang.std.txt.anonvec.open.bool = private unnamed_addr constant [7 x i8] c"(bool{\00", align 1
@nialang.std.txt.anonvec.open.string = private unnamed_addr constant [9 x i8] c"(string{\00", align 1

define i32 @strcmp(ptr %a, ptr %b) {
entry:
  br label %loop

loop:
  %idx = phi i64 [ 0, %entry ], [ %next, %same_nonzero ]
  %ap = getelementptr i8, ptr %a, i64 %idx
  %bp = getelementptr i8, ptr %b, i64 %idx
  %av = load i8, ptr %ap, align 1
  %bv = load i8, ptr %bp, align 1
  %eq = icmp eq i8 %av, %bv
  br i1 %eq, label %same, label %diff

same:
  %zero = icmp eq i8 %av, 0
  br i1 %zero, label %ret_zero, label %same_nonzero

same_nonzero:
  %next = add i64 %idx, 1
  br label %loop

diff:
  %av32 = zext i8 %av to i32
  %bv32 = zext i8 %bv to i32
  %res = sub i32 %av32, %bv32
  ret i32 %res

ret_zero:
  ret i32 0
}

"#
}

/// Escapes arbitrary bytes into LLVM `c"..."` literal form.
///
/// Printable ASCII is emitted directly, while control/non-ASCII bytes are emitted as `\XX`.
fn llvm_c_escape(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        match b {
            b'\\' => s.push_str("\\5C"),
            b'"' => s.push_str("\\22"),
            0x20..=0x7e => s.push(char::from(b)),
            _ => {
                let _ = write!(s, "\\{:02X}", b);
            }
        }
    }
    s
}

/// Produces unique global symbol name for named-struct JSON key fragment.
fn struct_field_key_symbol(sname: &str, fname: &str) -> String {
    format!(
        "nialang.std.txt.json.key.{}.{}",
        sanitize(sname),
        sanitize(fname)
    )
}

/// Source-like spelling of a type for `println` (vector axis element type).
fn ty_print_label(t: &Ty) -> String {
    match t {
        Ty::I8 => "i8".into(),
        Ty::U8 => "u8".into(),
        Ty::I16 => "i16".into(),
        Ty::U16 => "u16".into(),
        Ty::I32 => "i32".into(),
        Ty::U32 => "u32".into(),
        Ty::I64 => "i64".into(),
        Ty::U64 => "u64".into(),
        Ty::I128 => "i128".into(),
        Ty::Isize => "isize".into(),
        Ty::Usize => "usize".into(),
        Ty::U128 => "u128".into(),
        Ty::Bool => "bool".into(),
        Ty::AtomicBool => "AtomicBool".into(),
        Ty::AtomicI8 => "AtomicI8".into(),
        Ty::AtomicU8 => "AtomicU8".into(),
        Ty::AtomicI16 => "AtomicI16".into(),
        Ty::AtomicU16 => "AtomicU16".into(),
        Ty::AtomicI32 => "AtomicI32".into(),
        Ty::AtomicU32 => "AtomicU32".into(),
        Ty::AtomicI64 => "AtomicI64".into(),
        Ty::AtomicU64 => "AtomicU64".into(),
        Ty::AtomicI128 => "AtomicI128".into(),
        Ty::AtomicU128 => "AtomicU128".into(),
        Ty::AtomicIsize => "AtomicIsize".into(),
        Ty::AtomicUsize => "AtomicUsize".into(),
        Ty::AtomicPtr(elem) => format!("AtomicPtr[{}]", ty_print_label(elem)),
        Ty::Thread => "Thread".into(),
        Ty::Arc(elem) => format!("Arc[{}]", ty_print_label(elem)),
        Ty::Mutex(elem) => format!("Mutex[{}]", ty_print_label(elem)),
        Ty::MutexGuard(elem) => format!("MutexGuard[{}]", ty_print_label(elem)),
        Ty::Option(elem) => format!("Option[{}]", ty_print_label(elem)),
        Ty::ResultType(ok, err) => {
            format!("Result[{}, {}]", ty_print_label(ok), ty_print_label(err))
        }
        Ty::F16 => "f16".into(),
        Ty::F32 => "f32".into(),
        Ty::F64 => "f64".into(),
        Ty::String => "string".into(),
        Ty::Qubit => "qubit".into(),
        Ty::Result => "result".into(),
        Ty::Array(inner, n) => format!("[{}; {}]", ty_print_label(inner), n),
        Ty::Struct(n) => n.clone(),
        Ty::Enum(n) => n.clone(),
        Ty::Ptr(inner) => format!("&{}", ty_print_label(inner)),
        Ty::Unit => "()".into(),
        Ty::Vector(n, inner) => format!("{} {}", n, ty_print_label(inner)),
        Ty::AnonVector(inner, n) => format!("{}<{}>", ty_print_label(inner), n),
        Ty::HeapVector(inner) => format!("{}<>", ty_print_label(inner)),
        Ty::List(inner) => format!("List[{}]", ty_print_label(inner)),
        Ty::Matrix(inner, _) if matches!(inner.as_ref(), Ty::Unit) => "Matrix".into(),
        Ty::Matrix(inner, _) => format!("Matrix<{}>", ty_print_label(inner)),
        Ty::Fn(params, ret) => {
            let params = params
                .iter()
                .map(ty_print_label)
                .collect::<Vec<_>>()
                .join(", ");
            format!("fn({params}) -> {}", ty_print_label(ret))
        }
    }
}

/// LLVM global holding `println` prefix between `(` and `{`: element type + ASCII space (unique per vector decl).
fn vector_print_ty_prefix_symbol(vname: &str) -> String {
    format!("nialang.std.vector.ty.{}", sanitize(vname))
}

fn anon_vector_print_open_symbol(elem_ty: &Ty) -> Option<(&'static str, u32)> {
    match elem_ty {
        Ty::I8 => Some(("nialang.std.txt.anonvec.open.i8", 5)),
        Ty::U8 => Some(("nialang.std.txt.anonvec.open.u8", 5)),
        Ty::I16 => Some(("nialang.std.txt.anonvec.open.i16", 6)),
        Ty::U16 => Some(("nialang.std.txt.anonvec.open.u16", 6)),
        Ty::I32 => Some(("nialang.std.txt.anonvec.open.i32", 6)),
        Ty::U32 => Some(("nialang.std.txt.anonvec.open.u32", 6)),
        Ty::I64 => Some(("nialang.std.txt.anonvec.open.i64", 6)),
        Ty::U64 => Some(("nialang.std.txt.anonvec.open.u64", 6)),
        Ty::I128 => Some(("nialang.std.txt.anonvec.open.i128", 7)),
        Ty::Isize => Some(("nialang.std.txt.anonvec.open.isize", 8)),
        Ty::Usize => Some(("nialang.std.txt.anonvec.open.usize", 8)),
        Ty::U128 => Some(("nialang.std.txt.anonvec.open.u128", 7)),
        Ty::F16 => Some(("nialang.std.txt.anonvec.open.f16", 6)),
        Ty::F32 => Some(("nialang.std.txt.anonvec.open.f32", 6)),
        Ty::F64 => Some(("nialang.std.txt.anonvec.open.f64", 6)),
        Ty::Bool => Some(("nialang.std.txt.anonvec.open.bool", 7)),
        Ty::String => Some(("nialang.std.txt.anonvec.open.string", 9)),
        _ => None,
    }
}

fn enum_variant_prefix_symbol(ename: &str, vname: &str) -> String {
    format!(
        "nialang.std.txt.enumprefix.{}.{}",
        sanitize(ename),
        sanitize(vname)
    )
}

fn enum_struct_field_key_symbol(ename: &str, vname: &str, fname: &str) -> String {
    format!(
        "nialang.std.txt.enumjson.{}.{}.{}",
        sanitize(ename),
        sanitize(vname),
        sanitize(fname)
    )
}

/// Global `Variant: ` strings and JSON keys for enum struct-variant fields (for `println`).
fn emit_enum_print_constants(enums: &[EnumDef]) -> String {
    let mut out = String::new();
    for e in enums {
        for v in &e.variants {
            let prefix = format!("{}: ", v.name);
            let bytes = prefix.as_bytes();
            let sz = bytes.len() + 1;
            let lit = llvm_c_escape(bytes);
            let sym = enum_variant_prefix_symbol(&e.name, &v.name);
            writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                sym, sz, lit
            )
            .unwrap();
            if let EnumVariantFields::Struct(fs) = &v.fields {
                for (fname, _) in fs {
                    let key = format!("\"{}\": ", fname);
                    let bytes = key.as_bytes();
                    let sz = bytes.len() + 1;
                    let lit = llvm_c_escape(bytes);
                    let sym = enum_struct_field_key_symbol(&e.name, &v.name, fname);
                    writeln!(
                        out,
                        "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                        sym, sz, lit
                    )
                    .unwrap();
                }
            }
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Emits global string constants for named-struct JSON key rendering.
fn emit_struct_print_constants(structs: &[StructDef]) -> String {
    let mut out = String::new();
    for s in structs.iter().filter(|s| !s.is_tuple) {
        for (fname, _) in &s.fields {
            let key = format!("\"{}\": ", fname);
            let bytes = key.as_bytes();
            let sz = bytes.len() + 1;
            let lit = llvm_c_escape(bytes);
            let sym = struct_field_key_symbol(&s.name, fname);
            writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                sym, sz, lit
            )
            .unwrap();
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// JSON key fragments for `println` on vector values (same symbol scheme as named structs).
fn emit_vector_print_constants(vectors: &[VectorDef]) -> String {
    let mut out = String::new();
    for v in vectors {
        let ty_prefix = format!("{} ", ty_print_label(&v.ty));
        let bytes = ty_prefix.as_bytes();
        let sz = bytes.len() + 1;
        let lit = llvm_c_escape(bytes);
        let sym = vector_print_ty_prefix_symbol(&v.name);
        writeln!(
            out,
            "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
            sym, sz, lit
        )
        .unwrap();

        for fname in &v.fields {
            let key = format!("\"{fname}\": ");
            let bytes = key.as_bytes();
            let sz = bytes.len() + 1;
            let lit = llvm_c_escape(bytes);
            let sym = struct_field_key_symbol(&v.name, fname);
            writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                sym, sz, lit
            )
            .unwrap();
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Maps high-level nialang type into LLVM IR textual type.
fn fn_value_ll_ty() -> &'static str {
    "{ ptr, ptr, ptr, ptr }"
}

fn pthread_join_symbol() -> &'static str {
    if cfg!(target_os = "macos") {
        "@\"\\01_pthread_join\""
    } else {
        "@pthread_join"
    }
}

fn llvm_ty(t: &Ty, _structs: &[StructDef]) -> String {
    match t {
        Ty::I8 => "i8".into(),
        Ty::U8 => "i8".into(),
        Ty::I16 => "i16".into(),
        Ty::U16 => "i16".into(),
        Ty::I32 => "i32".into(),
        Ty::U32 => "i32".into(),
        Ty::I64 => "i64".into(),
        Ty::U64 => "i64".into(),
        Ty::I128 => "i128".into(),
        Ty::Isize => "i64".into(),
        Ty::Usize => "i64".into(),
        Ty::U128 => "i128".into(),
        Ty::Bool => "i1".into(),
        Ty::AtomicBool => "i1".into(),
        Ty::AtomicI8 | Ty::AtomicU8 => "i8".into(),
        Ty::AtomicI16 | Ty::AtomicU16 => "i16".into(),
        Ty::AtomicI32 | Ty::AtomicU32 => "i32".into(),
        Ty::AtomicI64 | Ty::AtomicU64 | Ty::AtomicIsize | Ty::AtomicUsize => "i64".into(),
        Ty::AtomicI128 | Ty::AtomicU128 => "i128".into(),
        Ty::AtomicPtr(_) => "ptr".into(),
        Ty::Thread => "ptr".into(),
        Ty::Option(_) | Ty::ResultType(_, _) => "ptr".into(),
        Ty::F16 => "half".into(),
        Ty::F32 => "float".into(),
        Ty::F64 => "double".into(),
        Ty::String => "ptr".into(),
        Ty::Qubit => "ptr".into(),
        Ty::Result => "ptr".into(),
        Ty::Array(elem, n) => format!("[{} x {}]", n, llvm_ty(elem, _structs)),
        Ty::Struct(n) => format!("%struct.{}", sanitize(n)),
        Ty::Enum(n) => format!("%enum.{}", sanitize(n)),
        Ty::Ptr(_) => "ptr".into(),
        Ty::Unit => "void".into(),
        Ty::Vector(n, _) => format!("%struct.{}", sanitize(n)),
        Ty::AnonVector(elem, n) => format!("[{} x {}]", n, llvm_ty(elem, _structs)),
        Ty::HeapVector(_) => "ptr".into(),
        Ty::List(_) => "ptr".into(),
        Ty::Arc(_) => "ptr".into(),
        Ty::Mutex(_) => "ptr".into(),
        Ty::MutexGuard(_) => "ptr".into(),
        Ty::Matrix(_, _) => "ptr".into(),
        Ty::Fn(_, _) => fn_value_ll_ty().into(),
    }
}

fn normalize_codegen_ty(t: &Ty) -> Ty {
    match t {
        Ty::Struct(name) if name == QUBIT => Ty::Qubit,
        Ty::Struct(name) if name == "result" => Ty::Result,
        Ty::Struct(name) if name == OPTION_TYPE => {
            panic!("typechecked Option without type argument")
        }
        Ty::Struct(name) if name == RESULT_TYPE => {
            panic!("typechecked Result without type arguments")
        }
        Ty::Struct(name) if name == ARC_TYPE => {
            panic!("typechecked Arc without type argument")
        }
        Ty::Struct(name) if name == MUTEX_TYPE => {
            panic!("typechecked Mutex without type argument")
        }
        Ty::Struct(name) if name == crate::nia_std::ATOMIC_BOOL_TYPE => Ty::AtomicBool,
        Ty::Struct(name) if name == ATOMIC_I8_TYPE => Ty::AtomicI8,
        Ty::Struct(name) if name == ATOMIC_U8_TYPE => Ty::AtomicU8,
        Ty::Struct(name) if name == ATOMIC_I16_TYPE => Ty::AtomicI16,
        Ty::Struct(name) if name == ATOMIC_U16_TYPE => Ty::AtomicU16,
        Ty::Struct(name) if name == ATOMIC_I32_TYPE => Ty::AtomicI32,
        Ty::Struct(name) if name == ATOMIC_U32_TYPE => Ty::AtomicU32,
        Ty::Struct(name) if name == ATOMIC_I64_TYPE => Ty::AtomicI64,
        Ty::Struct(name) if name == ATOMIC_U64_TYPE => Ty::AtomicU64,
        Ty::Struct(name) if name == ATOMIC_I128_TYPE => Ty::AtomicI128,
        Ty::Struct(name) if name == ATOMIC_U128_TYPE => Ty::AtomicU128,
        Ty::Struct(name) if name == ATOMIC_ISIZE_TYPE => Ty::AtomicIsize,
        Ty::Struct(name) if name == ATOMIC_USIZE_TYPE => Ty::AtomicUsize,
        Ty::Struct(name) if name == THREAD_TYPE => Ty::Thread,
        Ty::Array(elem, n) => Ty::Array(Box::new(normalize_codegen_ty(elem)), *n),
        Ty::Ptr(inner) => Ty::Ptr(Box::new(normalize_codegen_ty(inner))),
        Ty::AtomicPtr(inner) => Ty::AtomicPtr(Box::new(normalize_codegen_ty(inner))),
        Ty::Vector(name, inner) => Ty::Vector(name.clone(), Box::new(normalize_codegen_ty(inner))),
        Ty::AnonVector(inner, n) => Ty::AnonVector(Box::new(normalize_codegen_ty(inner)), *n),
        Ty::HeapVector(inner) => Ty::HeapVector(Box::new(normalize_codegen_ty(inner))),
        Ty::List(inner) => Ty::List(Box::new(normalize_codegen_ty(inner))),
        Ty::Arc(inner) => Ty::Arc(Box::new(normalize_codegen_ty(inner))),
        Ty::Mutex(inner) => Ty::Mutex(Box::new(normalize_codegen_ty(inner))),
        Ty::MutexGuard(inner) => Ty::MutexGuard(Box::new(normalize_codegen_ty(inner))),
        Ty::Option(inner) => Ty::Option(Box::new(normalize_codegen_ty(inner))),
        Ty::ResultType(ok, err) => Ty::ResultType(
            Box::new(normalize_codegen_ty(ok)),
            Box::new(normalize_codegen_ty(err)),
        ),
        Ty::Matrix(inner, shape) => Ty::Matrix(Box::new(normalize_codegen_ty(inner)), *shape),
        other => other.clone(),
    }
}

/// Signed integer types use `icmp slt` for `..` range checks; unsigned use `icmp ult`.
fn int_ty_signed(t: &Ty) -> bool {
    matches!(
        t,
        Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize
    )
}

#[allow(dead_code)]
fn llvm_atomic_ordering(ordering: AtomicOrdering) -> &'static str {
    match ordering {
        AtomicOrdering::Relaxed => "monotonic",
        AtomicOrdering::Acquire => "acquire",
        AtomicOrdering::Release => "release",
        AtomicOrdering::AcqRel => "acq_rel",
        AtomicOrdering::SeqCst => "seq_cst",
    }
}

fn atomic_exchange_failure_ordering(ordering: AtomicOrdering) -> AtomicOrdering {
    match ordering {
        AtomicOrdering::Relaxed => AtomicOrdering::Relaxed,
        AtomicOrdering::Acquire => AtomicOrdering::Acquire,
        AtomicOrdering::Release => AtomicOrdering::Relaxed,
        AtomicOrdering::AcqRel => AtomicOrdering::Acquire,
        AtomicOrdering::SeqCst => AtomicOrdering::SeqCst,
    }
}

fn atomic_int_constructor_tys(name: &str) -> Option<(Ty, Ty)> {
    match name {
        ATOMIC_I8 => Some((Ty::AtomicI8, Ty::I8)),
        ATOMIC_U8 => Some((Ty::AtomicU8, Ty::U8)),
        ATOMIC_I16 => Some((Ty::AtomicI16, Ty::I16)),
        ATOMIC_U16 => Some((Ty::AtomicU16, Ty::U16)),
        ATOMIC_I32 => Some((Ty::AtomicI32, Ty::I32)),
        ATOMIC_U32 => Some((Ty::AtomicU32, Ty::U32)),
        ATOMIC_I64 => Some((Ty::AtomicI64, Ty::I64)),
        ATOMIC_U64 => Some((Ty::AtomicU64, Ty::U64)),
        ATOMIC_I128 => Some((Ty::AtomicI128, Ty::I128)),
        ATOMIC_U128 => Some((Ty::AtomicU128, Ty::U128)),
        ATOMIC_ISIZE => Some((Ty::AtomicIsize, Ty::Isize)),
        ATOMIC_USIZE => Some((Ty::AtomicUsize, Ty::Usize)),
        _ => None,
    }
}

fn atomic_int_value_ty(t: &Ty) -> Option<Ty> {
    match t {
        Ty::AtomicI8 => Some(Ty::I8),
        Ty::AtomicU8 => Some(Ty::U8),
        Ty::AtomicI16 => Some(Ty::I16),
        Ty::AtomicU16 => Some(Ty::U16),
        Ty::AtomicI32 => Some(Ty::I32),
        Ty::AtomicU32 => Some(Ty::U32),
        Ty::AtomicI64 => Some(Ty::I64),
        Ty::AtomicU64 => Some(Ty::U64),
        Ty::AtomicI128 => Some(Ty::I128),
        Ty::AtomicU128 => Some(Ty::U128),
        Ty::AtomicIsize => Some(Ty::Isize),
        Ty::AtomicUsize => Some(Ty::Usize),
        _ => None,
    }
}

#[allow(dead_code)]
fn atomic_storage_align_bytes(storage_ty: &Ty) -> usize {
    match storage_ty {
        Ty::Bool | Ty::AtomicBool | Ty::I8 | Ty::U8 | Ty::AtomicI8 | Ty::AtomicU8 => 1,
        Ty::I16 | Ty::U16 | Ty::AtomicI16 | Ty::AtomicU16 => 2,
        Ty::I32 | Ty::U32 | Ty::AtomicI32 | Ty::AtomicU32 => 4,
        Ty::I64
        | Ty::U64
        | Ty::Isize
        | Ty::Usize
        | Ty::Ptr(_)
        | Ty::AtomicI64
        | Ty::AtomicU64
        | Ty::AtomicIsize
        | Ty::AtomicUsize
        | Ty::AtomicPtr(_) => 8,
        Ty::I128 | Ty::U128 | Ty::AtomicI128 | Ty::AtomicU128 => 16,
        other => unreachable!("unsupported atomic storage type: {other:?}"),
    }
}

fn is_atomic_bool_method_name(name: &str) -> bool {
    matches!(
        name,
        ATOMIC_LOAD_METHOD
            | ATOMIC_STORE_METHOD
            | ATOMIC_SWAP_METHOD
            | ATOMIC_COMPARE_EXCHANGE_METHOD
            | ATOMIC_FETCH_AND_METHOD
            | ATOMIC_FETCH_OR_METHOD
            | ATOMIC_FETCH_XOR_METHOD
    )
}

fn is_atomic_ptr_method_name(name: &str) -> bool {
    matches!(
        name,
        ATOMIC_LOAD_METHOD
            | ATOMIC_STORE_METHOD
            | ATOMIC_SWAP_METHOD
            | ATOMIC_COMPARE_EXCHANGE_METHOD
    )
}

fn is_atomic_int_method_name(name: &str) -> bool {
    matches!(
        name,
        ATOMIC_LOAD_METHOD
            | ATOMIC_STORE_METHOD
            | ATOMIC_SWAP_METHOD
            | ATOMIC_COMPARE_EXCHANGE_METHOD
            | ATOMIC_FETCH_ADD_METHOD
            | ATOMIC_FETCH_SUB_METHOD
            | ATOMIC_FETCH_AND_METHOD
            | ATOMIC_FETCH_OR_METHOD
            | ATOMIC_FETCH_XOR_METHOD
    )
}

fn is_atomic_method_name(name: &str) -> bool {
    is_atomic_bool_method_name(name)
        || is_atomic_ptr_method_name(name)
        || is_atomic_int_method_name(name)
}

fn atomic_ordering_from_expr(e: &Expr) -> AtomicOrdering {
    let Expr::Ident(path) = e else {
        unreachable!("typechecked atomic ordering literal")
    };
    crate::nia_std::atomic_ordering_from_path(path).expect("typechecked atomic ordering")
}

fn is_float_ty(t: &Ty) -> bool {
    matches!(t, Ty::F16 | Ty::F32 | Ty::F64)
}

/// Emits LLVM `%struct.Name = type { ... }` declaration.
fn struct_type_decl(s: &StructDef) -> String {
    let parts: Vec<String> = s.fields.iter().map(|(_, t)| llvm_ty(t, &[])).collect();
    format!(
        "%struct.{} = type {{ {} }}",
        sanitize(&s.name),
        parts.join(", ")
    )
}

fn vector_type_decl(v: &VectorDef, structs: &[StructDef]) -> String {
    let elem = llvm_ty(&v.ty, structs);
    let parts = vec![elem; v.fields.len()];
    format!(
        "%struct.{} = type {{ {} }}",
        sanitize(&v.name),
        parts.join(", ")
    )
}

fn enum_variant_payload_ty(v: &crate::ast::EnumVariantDef, structs: &[StructDef]) -> String {
    match &v.fields {
        EnumVariantFields::Unit => "i8".into(),
        EnumVariantFields::Tuple(ts) => {
            if ts.len() == 1 {
                llvm_ty(&ts[0], structs)
            } else {
                let inner = ts
                    .iter()
                    .map(|t| llvm_ty(t, structs))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {inner} }}")
            }
        }
        EnumVariantFields::Struct(fs) => {
            let inner = fs
                .iter()
                .map(|(_, t)| llvm_ty(t, structs))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {inner} }}")
        }
    }
}

fn enum_type_decl(e: &EnumDef, structs: &[StructDef]) -> String {
    let mut parts = vec!["i32".to_string()];
    for v in &e.variants {
        parts.push(enum_variant_payload_ty(v, structs));
    }
    format!(
        "%enum.{} = type {{ {} }}",
        sanitize(&e.name),
        parts.join(", ")
    )
}

/// Sanitizes user-defined names for safe LLVM symbol usage.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn split_variant_path(path: &str) -> Option<(&str, &str)> {
    path.rsplit_once("::")
        .filter(|(enum_name, variant)| !enum_name.is_empty() && !variant.is_empty())
}

/// `root[i][j]...` as a chain ending in a root lvalue expression.
fn collect_array_index_chain(mut e: &Expr) -> Option<(&Expr, Vec<&Expr>)> {
    let mut idxs = Vec::new();
    loop {
        match e {
            Expr::Index(a, i) => {
                idxs.push(i.as_ref());
                e = a.as_ref();
            }
            Expr::Ident(_) | Expr::Deref(_) => {
                idxs.reverse();
                return Some((e, idxs));
            }
            _ => return None,
        }
    }
}

fn method_receiver_owner_ty(t: &Ty) -> &Ty {
    match t {
        Ty::Ptr(inner) => inner.as_ref(),
        _ => t,
    }
}

fn has_declared_ability(abilities: &[Ability], ability: Ability) -> bool {
    abilities.contains(&ability)
}

fn supports_clone_method_ty(
    t: &Ty,
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
) -> bool {
    match t {
        Ty::Unit
        | Ty::Bool
        | Ty::I8
        | Ty::U8
        | Ty::I16
        | Ty::U16
        | Ty::I32
        | Ty::U32
        | Ty::I64
        | Ty::U64
        | Ty::I128
        | Ty::Isize
        | Ty::Usize
        | Ty::U128
        | Ty::F16
        | Ty::F32
        | Ty::F64
        | Ty::String => true,
        Ty::Struct(name) => {
            vectors
                .iter()
                .find(|v| v.name == *name)
                .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Clone))
                || structs
                    .iter()
                    .find(|s| s.name == *name)
                    .is_some_and(|s| has_declared_ability(&s.abilities, Ability::Clone))
        }
        Ty::Enum(name) => enums
            .iter()
            .find(|e| e.name == *name)
            .is_some_and(|e| has_declared_ability(&e.abilities, Ability::Clone)),
        Ty::Vector(name, _) => vectors
            .iter()
            .find(|v| v.name == *name)
            .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Clone)),
        Ty::Array(elem, _) | Ty::AnonVector(elem, _) => {
            supports_clone_method_ty(elem, structs, enums, vectors)
        }
        Ty::HeapVector(elem) | Ty::List(elem) | Ty::Matrix(elem, _) => {
            supports_clone_method_ty(elem, structs, enums, vectors)
        }
        Ty::Arc(_) => true,
        Ty::Fn(_, _) => true,
        _ => false,
    }
}

fn digest_ty() -> Ty {
    Ty::Array(Box::new(Ty::U8), 32)
}

fn matrix_elem_size(t: &Ty) -> usize {
    match t {
        Ty::I8 | Ty::U8 => 1,
        Ty::I16 | Ty::U16 | Ty::F16 => 2,
        Ty::I32 | Ty::U32 | Ty::F32 => 4,
        Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize | Ty::F64 => 8,
        Ty::I128 | Ty::U128 => 16,
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

fn matrix_binop_label(op: &str) -> &'static str {
    match op {
        "+" => "add",
        "-" => "sub",
        "*" => "mul",
        _ => unreachable!("typechecked matrix operator"),
    }
}

fn matrix_int_binop_instruction(op: &str) -> &'static str {
    match op {
        "+" => "add",
        "-" => "sub",
        "*" => "mul",
        _ => unreachable!("typechecked matrix operator"),
    }
}

fn matrix_int_div_instruction(elem_ty: &Ty) -> &'static str {
    match elem_ty {
        Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize => "sdiv",
        Ty::U8 | Ty::U16 | Ty::U32 | Ty::U64 | Ty::Usize | Ty::U128 => "udiv",
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

fn matrix_float_binop_instruction(op: &str) -> &'static str {
    match op {
        "+" => "fadd",
        "-" => "fsub",
        "*" => "fmul",
        "/" => "fdiv",
        _ => unreachable!("typechecked matrix operator"),
    }
}

fn matrix_zero_value(t: &Ty) -> &'static str {
    match t {
        Ty::I8
        | Ty::U8
        | Ty::I16
        | Ty::U16
        | Ty::I32
        | Ty::U32
        | Ty::I64
        | Ty::U64
        | Ty::Isize
        | Ty::Usize
        | Ty::I128
        | Ty::U128 => "0",
        Ty::F16 | Ty::F32 | Ty::F64 => "0.0",
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

fn matrix_one_value(t: &Ty) -> &'static str {
    match t {
        Ty::I8
        | Ty::U8
        | Ty::I16
        | Ty::U16
        | Ty::I32
        | Ty::U32
        | Ty::I64
        | Ty::U64
        | Ty::Isize
        | Ty::Usize
        | Ty::I128
        | Ty::U128 => "1",
        Ty::F16 | Ty::F32 | Ty::F64 => "1.0",
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

struct Gen<'a> {
    structs: &'a [StructDef],
    enums: &'a [EnumDef],
    vectors: &'a [VectorDef],
    fn_sigs: &'a HashMap<String, FnSig>,
    str_lit_syms: &'a HashMap<String, String>,
    tmp: u32,
    lbl: u32,
    out: String,
    mode: CodegenMode,
    qir_next_result: usize,
    terminated: bool,
    /// Labels of `loop.exit` for nested `loop`; `break` branches to the top.
    loop_exit_stack: Vec<String>,
    /// Visible-order index where each active `loop` body scope starts.
    loop_scope_start_stack: Vec<usize>,
    closures: Rc<RefCell<ClosureState>>,
}

impl<'a> Gen<'a> {
    /// Constructs function-level codegen state.
    fn new(
        structs: &'a [StructDef],
        enums: &'a [EnumDef],
        vectors: &'a [VectorDef],
        fn_sigs: &'a HashMap<String, FnSig>,
        str_lit_syms: &'a HashMap<String, String>,
        mode: CodegenMode,
        closures: Rc<RefCell<ClosureState>>,
    ) -> Self {
        Self {
            structs,
            enums,
            vectors,
            fn_sigs,
            str_lit_syms,
            tmp: 0,
            lbl: 0,
            out: String::new(),
            mode,
            qir_next_result: 0,
            terminated: false,
            loop_exit_stack: Vec::new(),
            loop_scope_start_stack: Vec::new(),
            closures,
        }
    }

    /// Allocates fresh SSA temporary id.
    fn fresh(&mut self) -> String {
        let n = self.tmp;
        self.tmp += 1;
        format!("t{n}")
    }

    /// Allocates fresh basic-block label id.
    fn fresh_label(&mut self, prefix: &str) -> String {
        let n = self.lbl;
        self.lbl += 1;
        format!("{prefix}.{n}")
    }

    fn emit_fn_value(&mut self, code: &str, env: &str, drop: &str, clone: &str) -> String {
        let with_code = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} poison, ptr {}, 0",
            with_code,
            fn_value_ll_ty(),
            code
        )
        .unwrap();
        let with_env = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} %{}, ptr {}, 1",
            with_env,
            fn_value_ll_ty(),
            with_code,
            env
        )
        .unwrap();
        let with_drop = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} %{}, ptr {}, 2",
            with_drop,
            fn_value_ll_ty(),
            with_env,
            drop
        )
        .unwrap();
        let with_clone = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} %{}, ptr {}, 3",
            with_clone,
            fn_value_ll_ty(),
            with_drop,
            clone
        )
        .unwrap();
        format!("%{with_clone}")
    }

    fn closure_env_ll_ty(captures: &[(String, Ty)]) -> String {
        if captures.is_empty() {
            return "{}".into();
        }
        let fields = captures
            .iter()
            .map(|(_, ty)| llvm_ty(ty, &[]))
            .collect::<Vec<_>>()
            .join(", ");
        format!("{{ {fields} }}")
    }

    fn emit_method_self_arg(
        &mut self,
        recv_ty: &Ty,
        recv_val: String,
        self_param: &Ty,
    ) -> (Ty, String) {
        if types_match(recv_ty, self_param) {
            (self_param.clone(), recv_val)
        } else if let Ty::Ptr(pointee) = self_param {
            debug_assert!(types_match(recv_ty, pointee));
            let slot = self.fresh();
            writeln!(
                self.out,
                "  %{} = alloca {}",
                slot,
                llvm_ty(pointee, self.structs)
            )
            .unwrap();
            writeln!(
                self.out,
                "  store {} {}, ptr %{}",
                llvm_ty(pointee, self.structs),
                recv_val,
                slot
            )
            .unwrap();
            (self_param.clone(), format!("%{slot}"))
        } else if let Ty::Ptr(pointee) = recv_ty {
            debug_assert!(types_match(pointee, self_param));
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = load {}, ptr {}",
                tmp,
                llvm_ty(self_param, self.structs),
                recv_val
            )
            .unwrap();
            (self_param.clone(), format!("%{tmp}"))
        } else {
            unreachable!("typechecked method receiver")
        }
    }

    fn emit_custom_deref_ptr(&mut self, recv_ty: &Ty, recv_val: String) -> Option<(Ty, String)> {
        if matches!(recv_ty, Ty::Ptr(_)) {
            return None;
        }
        let symbol = method_symbol(method_receiver_owner_ty(recv_ty), DEREF_METHOD);
        let (params, ret) = self
            .fn_sigs
            .get(&symbol)
            .map(|sig| (sig.params.clone(), sig.ret.clone()))?;
        debug_assert_eq!(params.len(), 1);
        let Some(Ty::Ptr(pointee)) = ret else {
            return None;
        };
        let (self_arg_ty, self_arg_val) = self.emit_method_self_arg(recv_ty, recv_val, &params[0]);
        let ret_ty = Ty::Ptr(pointee.clone());
        let tmp = self.fresh();
        writeln!(
            self.out,
            "  %{} = call {} @{}({} {})",
            tmp,
            llvm_ty(&ret_ty, self.structs),
            sanitize(&symbol),
            llvm_ty(&self_arg_ty, self.structs),
            self_arg_val
        )
        .unwrap();
        Some((pointee.as_ref().clone(), format!("%{tmp}")))
    }

    fn emit_language_drop(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        debug_assert_eq!(args.len(), 1);
        let (arg_ty, arg_val) = self.emit_expr(&args[0], locals, None);
        self.emit_drop_value(&arg_ty, arg_val);
        (Ty::Unit, String::new())
    }

    fn has_custom_drop_ty(&self, ty: &Ty) -> bool {
        let Ty::Struct(name) = method_receiver_owner_ty(ty) else {
            return false;
        };
        self.struct_def(name)
            .is_some_and(|s| has_declared_ability(&s.abilities, Ability::Drop))
            && self
                .fn_sigs
                .contains_key(&method_symbol(&Ty::Struct(name.clone()), DROP_METHOD))
    }

    fn is_language_drop_ty(&self, ty: &Ty) -> bool {
        match method_receiver_owner_ty(ty) {
            Ty::Struct(name) if self.vectors.iter().any(|v| v.name == *name) => self
                .vectors
                .iter()
                .find(|v| v.name == *name)
                .is_some_and(|v| {
                    has_declared_ability(&v.abilities, Ability::Drop)
                        && self.is_language_drop_ty(&v.ty)
                }),
            Ty::Struct(name) => self.struct_def(name).is_some_and(|s| {
                has_declared_ability(&s.abilities, Ability::Drop)
                    && (self.has_custom_drop_ty(ty)
                        || s.fields
                            .iter()
                            .any(|(_, field_ty)| self.is_language_drop_ty(field_ty)))
            }),
            Ty::Enum(name) => self
                .enums
                .iter()
                .find(|e| e.name == *name)
                .is_some_and(|e| {
                    has_declared_ability(&e.abilities, Ability::Drop)
                        && e.variants.iter().any(|variant| match &variant.fields {
                            EnumVariantFields::Unit => false,
                            EnumVariantFields::Tuple(fields) => fields
                                .iter()
                                .any(|field_ty| self.is_language_drop_ty(field_ty)),
                            EnumVariantFields::Struct(fields) => fields
                                .iter()
                                .any(|(_, field_ty)| self.is_language_drop_ty(field_ty)),
                        })
                }),
            Ty::Vector(name, elem) => {
                self.vectors
                    .iter()
                    .find(|v| v.name == *name)
                    .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Drop))
                    && self.is_language_drop_ty(elem)
            }
            Ty::Array(elem, _) | Ty::AnonVector(elem, _) => self.is_language_drop_ty(elem),
            Ty::HeapVector(_)
            | Ty::List(_)
            | Ty::Arc(_)
            | Ty::Mutex(_)
            | Ty::MutexGuard(_)
            | Ty::Option(_)
            | Ty::ResultType(_, _)
            | Ty::Matrix(_, _)
            | Ty::Thread => true,
            Ty::Fn(_, _) => true,
            _ => false,
        }
    }

    fn emit_custom_drop_call(&mut self, ty: &Ty, value: String) {
        let symbol = method_symbol(method_receiver_owner_ty(ty), DROP_METHOD);
        let (params, ret) = {
            let sig = self.fn_sigs.get(&symbol).expect("typechecked drop");
            (sig.params.clone(), sig.ret.clone())
        };
        debug_assert_eq!(params.len(), 1);
        let (self_arg_ty, self_arg_val) = self.emit_method_self_arg(ty, value, &params[0]);
        debug_assert!(matches!(ret, None | Some(Ty::Unit)));
        writeln!(
            self.out,
            "  call void @{}({} {})",
            sanitize(&symbol),
            llvm_ty(&self_arg_ty, self.structs),
            self_arg_val
        )
        .unwrap();
    }

    fn emit_matrix_clone_value(&mut self, matrix_ty: &Ty, matrix: &str) -> String {
        let Ty::Matrix(elem_ty, _) = matrix_ty else {
            unreachable!("typechecked matrix clone")
        };
        let rows = self.matrix_load_i64_field(matrix, 1);
        let cols = self.matrix_load_i64_field(matrix, 2);
        let len = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", len, rows, cols).unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        let out = self.emit_matrix_header_alloc(&format!("%{data}"), &rows, &cols);
        let source_data = self.matrix_load_data_ptr(matrix);

        let idx_addr = self.fresh();
        let cond_lbl = self.fresh_label("matrix.clone.cond");
        let body_lbl = self.fresh_label("matrix.clone.body");
        let done_lbl = self.fresh_label("matrix.clone.done");
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            has_item, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let source_cell = self.fresh();
        let out_cell = self.fresh();
        let value = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            source_cell, elem_ll, source_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell, elem_ll, data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            value, elem_ll, source_cell
        )
        .unwrap();
        let cloned = self.emit_clone_value(elem_ty, &format!("%{value}"));
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            elem_ll, cloned, out_cell
        )
        .unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();
        writeln!(self.out, "{}:", done_lbl).unwrap();
        out
    }

    fn emit_matrix_drop_value(&mut self, matrix: &str) {
        let data = self.matrix_load_data_ptr(matrix);
        writeln!(self.out, "  call void @free(ptr {})", data).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", matrix).unwrap();
    }

    fn emit_heap_vector_clone_value(&mut self, vector_ty: &Ty, vector: &str) -> String {
        let Ty::HeapVector(elem_ty) = vector_ty else {
            unreachable!("typechecked heap vector clone")
        };
        let len = self.heap_vector_load_i64_field(vector, 1);
        let (out, out_data) = self.emit_heap_vector_alloc(elem_ty, &len);
        let source_data = self.heap_vector_load_data_ptr(vector);
        let idx_addr = self.fresh();
        let loop_lbl = self.fresh_label("heap.vector.clone.loop");
        let body_lbl = self.fresh_label("heap.vector.clone.body");
        let cont_lbl = self.fresh_label("heap.vector.clone.cont");
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
        writeln!(self.out, "{}:", loop_lbl).unwrap();
        let idx = self.fresh();
        let keep_going = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            keep_going, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            keep_going, body_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", body_lbl).unwrap();
        let source_cell = self.heap_vector_data_cell_ptr(&source_data, &format!("%{idx}"), elem_ty);
        let out_cell = self.heap_vector_data_cell_ptr(&out_data, &format!("%{idx}"), elem_ty);
        let value = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            value,
            llvm_ty(elem_ty, self.structs),
            source_cell
        )
        .unwrap();
        let cloned = self.emit_clone_value(elem_ty, &format!("%{value}"));
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(elem_ty, self.structs),
            cloned,
            out_cell
        )
        .unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        out
    }

    fn emit_drop_linear_elements(
        &mut self,
        data: &str,
        len: &str,
        elem_ty: &Ty,
        label_prefix: &str,
    ) {
        if !self.is_language_drop_ty(elem_ty) {
            return;
        }
        let idx_addr = self.fresh();
        let loop_lbl = self.fresh_label(&format!("{label_prefix}.drop.loop"));
        let body_lbl = self.fresh_label(&format!("{label_prefix}.drop.body"));
        let cont_lbl = self.fresh_label(&format!("{label_prefix}.drop.cont"));
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
        writeln!(self.out, "{}:", loop_lbl).unwrap();
        let idx = self.fresh();
        let keep_going = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            keep_going, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            keep_going, body_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", body_lbl).unwrap();
        let cell = self.heap_vector_data_cell_ptr(data, &format!("%{idx}"), elem_ty);
        let value = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            value,
            llvm_ty(elem_ty, self.structs),
            cell
        )
        .unwrap();
        self.emit_drop_value(elem_ty, format!("%{value}"));
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn emit_heap_vector_drop_value(&mut self, elem_ty: &Ty, vector: &str) {
        let len = self.heap_vector_load_i64_field(vector, 1);
        let data = self.heap_vector_load_data_ptr(vector);
        self.emit_drop_linear_elements(&data, &len, elem_ty, "heap.vector");
        writeln!(self.out, "  call void @free(ptr {})", data).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", vector).unwrap();
    }

    fn emit_list_drop_value(&mut self, elem_ty: &Ty, list: &str) {
        let len = self.list_load_i64_field(list, 1);
        let data = self.list_load_data_ptr(list);
        self.emit_drop_linear_elements(&data, &len, elem_ty, "list");
        writeln!(self.out, "  call void @free(ptr {})", data).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", list).unwrap();
    }

    fn emit_list_clone_value(&mut self, elem_ty: &Ty, list: &str) -> String {
        let len = self.list_load_i64_field(list, 1);
        let out = self.emit_list_alloc(elem_ty, &len);
        let idx_addr = self.fresh();
        let loop_lbl = self.fresh_label("list.clone.loop");
        let body_lbl = self.fresh_label("list.clone.body");
        let cont_lbl = self.fresh_label("list.clone.cont");
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
        writeln!(self.out, "{}:", loop_lbl).unwrap();
        let idx = self.fresh();
        let keep_going = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            keep_going, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            keep_going, body_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", body_lbl).unwrap();
        let data = self.list_load_data_ptr(list);
        let cell = self.heap_vector_data_cell_ptr(&data, &format!("%{idx}"), elem_ty);
        let value = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            value,
            llvm_ty(elem_ty, self.structs),
            cell
        )
        .unwrap();
        let cloned = self.emit_clone_value(elem_ty, &format!("%{value}"));
        self.emit_list_push_value(&out, elem_ty, &cloned);
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        out
    }

    fn emit_clone_array_value(&mut self, elem_ty: &Ty, n: usize, value: &str, ty: &Ty) -> String {
        let arr_ll = llvm_ty(ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for idx in 0..n {
            let field = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                field, arr_ll, value, idx
            )
            .unwrap();
            let cloned = self.emit_clone_value(elem_ty, &format!("%{field}"));
            let next = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next, arr_ll, agg, elem_ll, cloned, idx
            )
            .unwrap();
            agg = format!("%{next}");
        }
        agg
    }

    fn emit_clone_struct_fields(
        &mut self,
        out_ty: &Ty,
        fields: &[(String, Ty)],
        value: &str,
    ) -> String {
        let struct_ll = llvm_ty(out_ty, self.structs);
        let mut agg = "poison".to_string();
        for (idx, (_, field_ty)) in fields.iter().enumerate() {
            let field = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                field, struct_ll, value, idx
            )
            .unwrap();
            let cloned = self.emit_clone_value(field_ty, &format!("%{field}"));
            let next = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next,
                struct_ll,
                agg,
                llvm_ty(field_ty, self.structs),
                cloned,
                idx
            )
            .unwrap();
            agg = format!("%{next}");
        }
        agg
    }

    fn emit_clone_enum_value(&mut self, enum_name: &str, value: &str) -> String {
        let edef = self
            .enums
            .iter()
            .find(|e| e.name == enum_name)
            .cloned()
            .expect("typechecked enum clone");
        let enum_ll = format!("%enum.{}", sanitize(enum_name));
        let out_slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", out_slot, enum_ll).unwrap();
        let tag = self.fresh();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 0",
            tag, enum_ll, value
        )
        .unwrap();
        let cont_lbl = self.fresh_label("clone.enum.cont");
        let default_lbl = self.fresh_label("clone.enum.default");
        let arm_labels = edef
            .variants
            .iter()
            .map(|_| self.fresh_label("clone.enum.arm"))
            .collect::<Vec<_>>();
        writeln!(self.out, "  switch i32 %{}, label %{} [", tag, default_lbl).unwrap();
        for (idx, lbl) in arm_labels.iter().enumerate() {
            writeln!(self.out, "    i32 {}, label %{}", idx, lbl).unwrap();
        }
        writeln!(self.out, "  ]").unwrap();
        writeln!(self.out, "{}:", default_lbl).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", enum_ll, value, out_slot).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        for (idx, (variant, lbl)) in edef.variants.iter().zip(&arm_labels).enumerate() {
            writeln!(self.out, "{}:", lbl).unwrap();
            let with_tag = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} poison, i32 {}, 0",
                with_tag, enum_ll, idx
            )
            .unwrap();
            let payload_idx = idx + 1;
            let payload = match &variant.fields {
                EnumVariantFields::Unit => {
                    let payload_ll = enum_variant_payload_ty(variant, self.structs);
                    (payload_ll, "zeroinitializer".to_string())
                }
                EnumVariantFields::Tuple(ts) if ts.len() == 1 => {
                    let payload_ll = llvm_ty(&ts[0], self.structs);
                    let raw = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        raw, enum_ll, value, payload_idx
                    )
                    .unwrap();
                    let cloned = self.emit_clone_value(&ts[0], &format!("%{raw}"));
                    (payload_ll, cloned)
                }
                EnumVariantFields::Tuple(ts) => {
                    let payload_ll = enum_variant_payload_ty(variant, self.structs);
                    let raw_payload = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        raw_payload, enum_ll, value, payload_idx
                    )
                    .unwrap();
                    let mut agg = "poison".to_string();
                    for (field_idx, field_ty) in ts.iter().enumerate() {
                        let raw = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = extractvalue {} %{}, {}",
                            raw, payload_ll, raw_payload, field_idx
                        )
                        .unwrap();
                        let cloned = self.emit_clone_value(field_ty, &format!("%{raw}"));
                        let next = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = insertvalue {} {}, {} {}, {}",
                            next,
                            payload_ll,
                            agg,
                            llvm_ty(field_ty, self.structs),
                            cloned,
                            field_idx
                        )
                        .unwrap();
                        agg = format!("%{next}");
                    }
                    (payload_ll, agg)
                }
                EnumVariantFields::Struct(fs) => {
                    let payload_ll = enum_variant_payload_ty(variant, self.structs);
                    let raw_payload = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        raw_payload, enum_ll, value, payload_idx
                    )
                    .unwrap();
                    let mut agg = "poison".to_string();
                    for (field_idx, (_, field_ty)) in fs.iter().enumerate() {
                        let raw = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = extractvalue {} %{}, {}",
                            raw, payload_ll, raw_payload, field_idx
                        )
                        .unwrap();
                        let cloned = self.emit_clone_value(field_ty, &format!("%{raw}"));
                        let next = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = insertvalue {} {}, {} {}, {}",
                            next,
                            payload_ll,
                            agg,
                            llvm_ty(field_ty, self.structs),
                            cloned,
                            field_idx
                        )
                        .unwrap();
                        agg = format!("%{next}");
                    }
                    (payload_ll, agg)
                }
            };
            let out = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} %{}, {} {}, {}",
                out, enum_ll, with_tag, payload.0, payload.1, payload_idx
            )
            .unwrap();
            writeln!(self.out, "  store {} %{}, ptr %{}", enum_ll, out, out_slot).unwrap();
            writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        }
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        let out = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", out, enum_ll, out_slot).unwrap();
        format!("%{out}")
    }

    fn emit_clone_value(&mut self, ty: &Ty, value: &str) -> String {
        let symbol = method_symbol(method_receiver_owner_ty(ty), CLONE_METHOD);
        if let Some((params, Some(ret_ty))) = self
            .fn_sigs
            .get(&symbol)
            .map(|sig| (sig.params.clone(), sig.ret.clone()))
        {
            debug_assert_eq!(params.len(), 1);
            let (self_arg_ty, self_arg_val) =
                self.emit_method_self_arg(ty, value.to_string(), &params[0]);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = call {} @{}({} {})",
                tmp,
                llvm_ty(&ret_ty, self.structs),
                sanitize(&symbol),
                llvm_ty(&self_arg_ty, self.structs),
                self_arg_val
            )
            .unwrap();
            return format!("%{tmp}");
        }
        match method_receiver_owner_ty(ty) {
            Ty::Array(elem_ty, n) | Ty::AnonVector(elem_ty, n) => {
                self.emit_clone_array_value(elem_ty, *n, value, ty)
            }
            Ty::HeapVector(_) => self.emit_heap_vector_clone_value(ty, value),
            Ty::List(elem_ty) => self.emit_list_clone_value(elem_ty, value),
            Ty::Arc(elem_ty) => self.emit_arc_clone_value(elem_ty, value),
            Ty::Matrix(_, _) => self.emit_matrix_clone_value(ty, value),
            Ty::Fn(_, _) => self.emit_fn_value_clone(value),
            Ty::Struct(name) if self.vector_def(name).is_some() => {
                let vdef = self.vector_def(name).cloned().expect("checked vector");
                let fields = vdef
                    .fields
                    .iter()
                    .map(|field| (field.clone(), vdef.ty.clone()))
                    .collect::<Vec<_>>();
                self.emit_clone_struct_fields(ty, &fields, value)
            }
            Ty::Struct(name) => {
                let Some(sdef) = self.struct_def(name).cloned() else {
                    return value.to_string();
                };
                self.emit_clone_struct_fields(ty, &sdef.fields, value)
            }
            Ty::Enum(name) => self.emit_clone_enum_value(name, value),
            _ => value.to_string(),
        }
    }

    fn is_copy_like_ty(&self, ty: &Ty) -> bool {
        match ty {
            Ty::Unit
            | Ty::Bool
            | Ty::I8
            | Ty::U8
            | Ty::I16
            | Ty::U16
            | Ty::I32
            | Ty::U32
            | Ty::I64
            | Ty::U64
            | Ty::I128
            | Ty::Isize
            | Ty::Usize
            | Ty::U128
            | Ty::F16
            | Ty::F32
            | Ty::F64
            | Ty::String
            | Ty::Qubit
            | Ty::Result
            | Ty::Ptr(_) => true,
            Ty::AtomicBool
            | Ty::AtomicI8
            | Ty::AtomicU8
            | Ty::AtomicI16
            | Ty::AtomicU16
            | Ty::AtomicI32
            | Ty::AtomicU32
            | Ty::AtomicI64
            | Ty::AtomicU64
            | Ty::AtomicI128
            | Ty::AtomicU128
            | Ty::AtomicIsize
            | Ty::AtomicUsize
            | Ty::AtomicPtr(_)
            | Ty::Thread
            | Ty::Fn(_, _) => false,
            Ty::Array(elem, _) | Ty::AnonVector(elem, _) => self.is_copy_like_ty(elem),
            Ty::HeapVector(_)
            | Ty::List(_)
            | Ty::Arc(_)
            | Ty::Mutex(_)
            | Ty::MutexGuard(_)
            | Ty::Option(_)
            | Ty::ResultType(_, _)
            | Ty::Matrix(_, _) => false,
            Ty::Struct(name) if name == crate::nia_std::COMPLEX_TYPE => true,
            Ty::Struct(name) => {
                self.structs
                    .iter()
                    .find(|s| s.name == *name)
                    .is_some_and(|s| has_declared_ability(&s.abilities, Ability::Copy))
                    || self
                        .vectors
                        .iter()
                        .find(|v| v.name == *name)
                        .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Copy))
            }
            Ty::Enum(name) => self
                .enums
                .iter()
                .find(|e| e.name == *name)
                .is_some_and(|e| has_declared_ability(&e.abilities, Ability::Copy)),
            Ty::Vector(name, elem) => {
                self.vectors
                    .iter()
                    .find(|v| v.name == *name)
                    .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Copy))
                    || self.is_copy_like_ty(elem)
            }
        }
    }

    fn emit_alloc_drop_flag(&mut self, name: &str, initial: bool) -> String {
        let flag = format!("%{}.drop", sanitize(name));
        writeln!(self.out, "  {} = alloca i1", flag).unwrap();
        writeln!(
            self.out,
            "  store i1 {}, ptr {}",
            if initial { "true" } else { "false" },
            flag
        )
        .unwrap();
        flag
    }

    fn emit_set_drop_flag(&mut self, flag: &str, value: bool) {
        writeln!(
            self.out,
            "  store i1 {}, ptr {}",
            if value { "true" } else { "false" },
            flag
        )
        .unwrap();
    }

    fn emit_fn_value_drop(&mut self, value: &str) {
        let drop_fn = self.fresh();
        let has_drop = self.fresh();
        let drop_lbl = self.fresh_label("fn.drop.env");
        let cont_lbl = self.fresh_label("fn.drop.cont");
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 2",
            drop_fn,
            fn_value_ll_ty(),
            value
        )
        .unwrap();
        writeln!(self.out, "  %{} = icmp ne ptr %{}, null", has_drop, drop_fn).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_drop, drop_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", drop_lbl).unwrap();
        let env = self.fresh();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 1",
            env,
            fn_value_ll_ty(),
            value
        )
        .unwrap();
        writeln!(self.out, "  call void %{}(ptr %{})", drop_fn, env).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn emit_fn_value_clone(&mut self, value: &str) -> String {
        let out_slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", out_slot, fn_value_ll_ty()).unwrap();
        let code = self.fresh();
        let env = self.fresh();
        let drop_fn = self.fresh();
        let clone_fn = self.fresh();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 0",
            code,
            fn_value_ll_ty(),
            value
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 1",
            env,
            fn_value_ll_ty(),
            value
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 2",
            drop_fn,
            fn_value_ll_ty(),
            value
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 3",
            clone_fn,
            fn_value_ll_ty(),
            value
        )
        .unwrap();
        let has_clone = self.fresh();
        let clone_lbl = self.fresh_label("fn.clone.env");
        let plain_lbl = self.fresh_label("fn.clone.plain");
        let cont_lbl = self.fresh_label("fn.clone.cont");
        writeln!(
            self.out,
            "  %{} = icmp ne ptr %{}, null",
            has_clone, clone_fn
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_clone, clone_lbl, plain_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", clone_lbl).unwrap();
        let new_env = self.fresh();
        writeln!(
            self.out,
            "  %{} = call ptr %{}(ptr %{})",
            new_env, clone_fn, env
        )
        .unwrap();
        let cloned = self.emit_fn_value(
            &format!("%{code}"),
            &format!("%{new_env}"),
            &format!("%{drop_fn}"),
            &format!("%{clone_fn}"),
        );
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            fn_value_ll_ty(),
            cloned,
            out_slot
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", plain_lbl).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            fn_value_ll_ty(),
            value,
            out_slot
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            out,
            fn_value_ll_ty(),
            out_slot
        )
        .unwrap();
        format!("%{out}")
    }

    fn emit_drop_value(&mut self, ty: &Ty, value: String) {
        match method_receiver_owner_ty(ty) {
            Ty::Array(elem_ty, n) | Ty::AnonVector(elem_ty, n) => {
                if !self.is_language_drop_ty(elem_ty) {
                    return;
                }
                let arr_ll = llvm_ty(ty, self.structs);
                for idx in (0..*n).rev() {
                    let elem = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        elem, arr_ll, value, idx
                    )
                    .unwrap();
                    self.emit_drop_value(elem_ty, format!("%{elem}"));
                }
            }
            Ty::HeapVector(elem_ty) => self.emit_heap_vector_drop_value(elem_ty, &value),
            Ty::List(elem_ty) => self.emit_list_drop_value(elem_ty, &value),
            Ty::Arc(elem_ty) => self.emit_arc_drop_value(elem_ty, &value),
            Ty::Mutex(elem_ty) => self.emit_mutex_drop_value(elem_ty, &value),
            Ty::MutexGuard(elem_ty) => self.emit_mutex_guard_drop_value(elem_ty, &value),
            Ty::Option(elem_ty) => self.emit_option_drop_value(elem_ty, &value),
            Ty::ResultType(ok_ty, err_ty) => self.emit_result_drop_value(ok_ty, err_ty, &value),
            Ty::Matrix(_, _) => self.emit_matrix_drop_value(&value),
            Ty::Thread => self.emit_thread_detach_drop(&value),
            Ty::Vector(name, elem_ty) => {
                let Some(vdef) = self.vector_def(name).cloned() else {
                    return;
                };
                if !self.is_language_drop_ty(elem_ty) {
                    return;
                }
                let vector_ll = llvm_ty(ty, self.structs);
                for idx in (0..vdef.fields.len()).rev() {
                    let field = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        field, vector_ll, value, idx
                    )
                    .unwrap();
                    self.emit_drop_value(elem_ty, format!("%{field}"));
                }
            }
            Ty::Struct(name) if self.vector_def(name).is_some() => {
                let vdef = self.vector_def(name).cloned().expect("checked vector");
                if !self.is_language_drop_ty(&vdef.ty) {
                    return;
                }
                let vector_ll = format!("%struct.{}", sanitize(name));
                for idx in (0..vdef.fields.len()).rev() {
                    let field = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        field, vector_ll, value, idx
                    )
                    .unwrap();
                    self.emit_drop_value(&vdef.ty, format!("%{field}"));
                }
            }
            Ty::Struct(name) => {
                if self.has_custom_drop_ty(ty) {
                    self.emit_custom_drop_call(ty, value.clone());
                }
                self.emit_drop_struct_fields(name, &value);
            }
            Ty::Enum(name) => self.emit_drop_enum_value(name, &value),
            Ty::Fn(_, _) => self.emit_fn_value_drop(&value),
            _ => {}
        }
    }

    fn emit_sum_payload_ptr(&mut self, value: &str) -> String {
        let slot = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr i8, ptr {}, i64 8",
            slot, value
        )
        .unwrap();
        let payload = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr %{}", payload, slot).unwrap();
        format!("%{payload}")
    }

    fn emit_option_drop_value(&mut self, elem_ty: &Ty, value: &str) {
        let tag = self.fresh();
        let drop_payload_lbl = self.fresh_label("option.drop.payload");
        let free_lbl = self.fresh_label("option.drop.free");
        writeln!(self.out, "  %{} = load i32, ptr {}", tag, value).unwrap();
        let is_some = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i32 %{}, 1", is_some, tag).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            is_some, drop_payload_lbl, free_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", drop_payload_lbl).unwrap();
        let payload = self.emit_sum_payload_ptr(value);
        if self.is_language_drop_ty(elem_ty) {
            let loaded = self.fresh();
            writeln!(
                self.out,
                "  %{} = load {}, ptr {}",
                loaded,
                llvm_ty(elem_ty, self.structs),
                payload
            )
            .unwrap();
            self.emit_drop_value(elem_ty, format!("%{loaded}"));
        }
        writeln!(self.out, "  call void @free(ptr {})", payload).unwrap();
        writeln!(self.out, "  br label %{}", free_lbl).unwrap();
        writeln!(self.out, "{}:", free_lbl).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", value).unwrap();
    }

    fn emit_result_drop_value(&mut self, ok_ty: &Ty, err_ty: &Ty, value: &str) {
        let tag = self.fresh();
        let ok_lbl = self.fresh_label("result.drop.ok");
        let err_lbl = self.fresh_label("result.drop.err");
        let free_lbl = self.fresh_label("result.drop.free");
        writeln!(self.out, "  %{} = load i32, ptr {}", tag, value).unwrap();
        writeln!(
            self.out,
            "  switch i32 %{}, label %{} [ i32 0, label %{} i32 1, label %{} ]",
            tag, free_lbl, ok_lbl, err_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        let payload = self.emit_sum_payload_ptr(value);
        if self.is_language_drop_ty(ok_ty) {
            let loaded = self.fresh();
            writeln!(
                self.out,
                "  %{} = load {}, ptr {}",
                loaded,
                llvm_ty(ok_ty, self.structs),
                payload
            )
            .unwrap();
            self.emit_drop_value(ok_ty, format!("%{loaded}"));
        }
        writeln!(self.out, "  call void @free(ptr {})", payload).unwrap();
        writeln!(self.out, "  br label %{}", free_lbl).unwrap();

        writeln!(self.out, "{}:", err_lbl).unwrap();
        let payload = self.emit_sum_payload_ptr(value);
        if self.is_language_drop_ty(err_ty) {
            let loaded = self.fresh();
            writeln!(
                self.out,
                "  %{} = load {}, ptr {}",
                loaded,
                llvm_ty(err_ty, self.structs),
                payload
            )
            .unwrap();
            self.emit_drop_value(err_ty, format!("%{loaded}"));
        }
        writeln!(self.out, "  call void @free(ptr {})", payload).unwrap();
        writeln!(self.out, "  br label %{}", free_lbl).unwrap();

        writeln!(self.out, "{}:", free_lbl).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", value).unwrap();
    }

    fn emit_drop_struct_fields(&mut self, name: &str, value: &str) {
        let Some(sdef) = self.struct_def(name).cloned() else {
            return;
        };
        let struct_ll = format!("%struct.{}", sanitize(name));
        for (i, (_, fty)) in sdef.fields.iter().enumerate().rev() {
            if !self.is_language_drop_ty(fty) {
                continue;
            }
            let field = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                field, struct_ll, value, i
            )
            .unwrap();
            self.emit_drop_value(fty, format!("%{field}"));
        }
    }

    fn emit_drop_enum_value(&mut self, name: &str, value: &str) {
        let edef = self
            .enums
            .iter()
            .find(|e| e.name == name)
            .cloned()
            .expect("typechecked enum drop");
        let enum_ll = format!("%enum.{}", sanitize(name));
        let tag = self.fresh();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 0",
            tag, enum_ll, value
        )
        .unwrap();
        let cont_lbl = self.fresh_label("drop.enum.cont");
        let default_lbl = self.fresh_label("drop.enum.default");
        let arm_labels = edef
            .variants
            .iter()
            .map(|_| self.fresh_label("drop.enum.arm"))
            .collect::<Vec<_>>();
        writeln!(self.out, "  switch i32 %{}, label %{} [", tag, default_lbl).unwrap();
        for (i, lbl) in arm_labels.iter().enumerate() {
            writeln!(self.out, "    i32 {}, label %{}", i, lbl).unwrap();
        }
        writeln!(self.out, "  ]").unwrap();
        writeln!(self.out, "{}:", default_lbl).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        for (idx, (variant, lbl)) in edef.variants.iter().zip(&arm_labels).enumerate() {
            writeln!(self.out, "{}:", lbl).unwrap();
            self.emit_drop_enum_payload(&enum_ll, value, idx + 1, &variant.fields);
            writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        }
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn emit_drop_enum_payload(
        &mut self,
        enum_ll: &str,
        value: &str,
        payload_idx: usize,
        fields: &EnumVariantFields,
    ) {
        match fields {
            EnumVariantFields::Unit => {}
            EnumVariantFields::Tuple(ts) if ts.len() == 1 => {
                if !self.is_language_drop_ty(&ts[0]) {
                    return;
                }
                let payload = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    payload, enum_ll, value, payload_idx
                )
                .unwrap();
                self.emit_drop_value(&ts[0], format!("%{payload}"));
            }
            EnumVariantFields::Tuple(ts) => {
                let payload_ll = {
                    let inner = ts
                        .iter()
                        .map(|t| llvm_ty(t, self.structs))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {inner} }}")
                };
                let payload = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    payload, enum_ll, value, payload_idx
                )
                .unwrap();
                for (i, ty) in ts.iter().enumerate().rev() {
                    if !self.is_language_drop_ty(ty) {
                        continue;
                    }
                    let field = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} %{}, {}",
                        field, payload_ll, payload, i
                    )
                    .unwrap();
                    self.emit_drop_value(ty, format!("%{field}"));
                }
            }
            EnumVariantFields::Struct(fs) => {
                let payload_ll = {
                    let inner = fs
                        .iter()
                        .map(|(_, t)| llvm_ty(t, self.structs))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {inner} }}")
                };
                let payload = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    payload, enum_ll, value, payload_idx
                )
                .unwrap();
                for (i, (_, ty)) in fs.iter().enumerate().rev() {
                    if !self.is_language_drop_ty(ty) {
                        continue;
                    }
                    let field = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} %{}, {}",
                        field, payload_ll, payload, i
                    )
                    .unwrap();
                    self.emit_drop_value(ty, format!("%{field}"));
                }
            }
        }
    }

    fn emit_drop_local_if_live(
        &mut self,
        name: &str,
        locals: &HashMap<String, (Ty, String)>,
        drop_flags: &HashMap<String, String>,
    ) {
        let Some(flag) = drop_flags.get(name) else {
            return;
        };
        let Some((ty, ptr)) = locals.get(name) else {
            return;
        };
        let live = self.fresh();
        let drop_lbl = self.fresh_label("drop.live");
        let cont_lbl = self.fresh_label("drop.cont");
        writeln!(self.out, "  %{} = load i1, ptr {}", live, flag).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            live, drop_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", drop_lbl).unwrap();
        let value = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            value,
            llvm_ty(ty, self.structs),
            ptr
        )
        .unwrap();
        self.emit_drop_value(ty, format!("%{value}"));
        self.emit_set_drop_flag(flag, false);
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn emit_drop_scope_from(
        &mut self,
        start: usize,
        visible_order: &[String],
        locals: &HashMap<String, (Ty, String)>,
        drop_flags: &HashMap<String, String>,
    ) {
        for name in visible_order.iter().skip(start).rev() {
            self.emit_drop_local_if_live(name, locals, drop_flags);
        }
    }

    fn clear_drop_flag_for_local(
        &mut self,
        name: &str,
        locals: &HashMap<String, (Ty, String)>,
        drop_flags: &HashMap<String, String>,
        force: bool,
    ) {
        let Some(flag) = drop_flags.get(name) else {
            return;
        };
        let Some((ty, _)) = locals.get(name) else {
            return;
        };
        if force || (self.is_language_drop_ty(ty) && !self.is_copy_like_ty(ty)) {
            self.emit_set_drop_flag(flag, false);
        }
    }

    fn expr_codegen_ty(&self, expr: &Expr, locals: &HashMap<String, (Ty, String)>) -> Ty {
        match expr {
            Expr::Ident(name) => locals
                .get(name)
                .map(|(ty, _)| ty.clone())
                .unwrap_or_else(|| Ty::Struct(name.clone())),
            Expr::AddrOf(inner) => Ty::Ptr(Box::new(self.expr_codegen_ty(inner, locals))),
            Expr::Deref(inner) => match self.expr_codegen_ty(inner, locals) {
                Ty::Ptr(inner) => inner.as_ref().clone(),
                Ty::Arc(inner) => inner.as_ref().clone(),
                Ty::MutexGuard(inner) => inner.as_ref().clone(),
                other => other,
            },
            Expr::Field(inner, _) => self.expr_codegen_ty(inner, locals),
            _ => Ty::Unit,
        }
    }

    fn is_runtime_owner_ty(&self, ty: &Ty) -> bool {
        matches!(
            ty,
            Ty::HeapVector(_)
                | Ty::List(_)
                | Ty::Arc(_)
                | Ty::Mutex(_)
                | Ty::MutexGuard(_)
                | Ty::Matrix(_, _)
        )
    }

    fn clear_runtime_read_or_consumed_expr(
        &mut self,
        expr: &Expr,
        locals: &HashMap<String, (Ty, String)>,
        drop_flags: &HashMap<String, String>,
    ) {
        let ty = self.expr_codegen_ty(expr, locals);
        self.clear_consumed_custom_drop_locals_expr(
            expr,
            locals,
            drop_flags,
            !self.is_runtime_owner_ty(&ty),
        );
    }

    fn clear_consumed_custom_drop_locals_expr(
        &mut self,
        expr: &Expr,
        locals: &HashMap<String, (Ty, String)>,
        drop_flags: &HashMap<String, String>,
        consume_result: bool,
    ) {
        match expr {
            Expr::Ident(name) => {
                if consume_result {
                    self.clear_drop_flag_for_local(name, locals, drop_flags, false);
                }
            }
            Expr::Int(_)
            | Expr::Float(_)
            | Expr::Bool(_)
            | Expr::String(_)
            | Expr::Spawn { .. }
            | Expr::EnumVariant { .. } => {}
            Expr::SpawnClosure { closure } => {
                self.clear_consumed_custom_drop_locals_expr(closure, locals, drop_flags, true);
            }
            Expr::Neg(inner) | Expr::Not(inner) | Expr::BitNot(inner) => {
                self.clear_consumed_custom_drop_locals_expr(inner, locals, drop_flags, true);
            }
            Expr::AddrOf(inner) | Expr::Deref(inner) | Expr::Field(inner, _) => {
                self.clear_consumed_custom_drop_locals_expr(inner, locals, drop_flags, false);
            }
            Expr::Add(l, r) | Expr::Sub(l, r) | Expr::Mul(l, r) | Expr::VecDot(l, r) => {
                self.clear_runtime_read_or_consumed_expr(l, locals, drop_flags);
                self.clear_runtime_read_or_consumed_expr(r, locals, drop_flags);
            }
            Expr::Div(l, r)
            | Expr::Rem(l, r)
            | Expr::BitAnd(l, r)
            | Expr::BitOr(l, r)
            | Expr::BitXor(l, r)
            | Expr::Shl(l, r)
            | Expr::Shr(l, r)
            | Expr::Eq(l, r)
            | Expr::Ne(l, r)
            | Expr::Lt(l, r)
            | Expr::Le(l, r)
            | Expr::Gt(l, r)
            | Expr::Ge(l, r)
            | Expr::Index(l, r) => {
                self.clear_consumed_custom_drop_locals_expr(l, locals, drop_flags, true);
                self.clear_consumed_custom_drop_locals_expr(r, locals, drop_flags, true);
            }
            Expr::Call { name, args } => {
                if name == DROP_METHOD || name == JOIN || name == MATRIX_DROP || name == VECTOR_DROP
                {
                    if let Some(Expr::Ident(arg_name)) = args.first() {
                        self.clear_drop_flag_for_local(arg_name, locals, drop_flags, true);
                    } else if let Some(arg) = args.first() {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                    }
                    return;
                }
                if name == MATRIX_CLONE
                    || name == MATRIX_ROWS
                    || name == MATRIX_COLS
                    || name == MATRIX_LEN
                    || name == VECTOR_CLONE
                    || name == VECTOR_LEN
                {
                    for arg in args {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, false);
                    }
                    return;
                }
                if name == MATRIX_GET || name == VECTOR_GET {
                    if let Some(first) = args.first() {
                        self.clear_consumed_custom_drop_locals_expr(
                            first, locals, drop_flags, false,
                        );
                    }
                    for arg in args.iter().skip(1) {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                    }
                    return;
                }
                if name == MATRIX_SET || name == VECTOR_SET {
                    if let Some(first) = args.first() {
                        self.clear_consumed_custom_drop_locals_expr(
                            first, locals, drop_flags, false,
                        );
                    }
                    for arg in args.iter().skip(1) {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                    }
                    return;
                }
                if name == OUTER {
                    for arg in args {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, false);
                    }
                    return;
                }
                if name == PRINTLN || name == LEN {
                    for arg in args {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, false);
                    }
                    return;
                }
                if let Some((Ty::Fn(params, _), _)) = locals.get(name) {
                    for (arg, param_ty) in args.iter().zip(params) {
                        self.clear_consumed_custom_drop_locals_expr(
                            arg,
                            locals,
                            drop_flags,
                            !matches!(param_ty, Ty::Ptr(_)),
                        );
                    }
                    return;
                }
                if let Some(sig) = self.fn_sigs.get(name) {
                    let params = sig.params.clone();
                    for (arg, param_ty) in args.iter().zip(params) {
                        self.clear_consumed_custom_drop_locals_expr(
                            arg,
                            locals,
                            drop_flags,
                            !matches!(param_ty, Ty::Ptr(_)),
                        );
                    }
                    return;
                }
                for arg in args {
                    self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                }
            }
            Expr::GenericCall { args, .. } | Expr::EnumTuple { args, .. } => {
                for arg in args {
                    self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                }
            }
            Expr::CallExpr { callee, args } => {
                self.clear_consumed_custom_drop_locals_expr(callee, locals, drop_flags, true);
                for arg in args {
                    self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                }
            }
            Expr::MethodCall {
                receiver,
                name,
                args,
            } => {
                if name == CLONE_METHOD
                    || name == TO_MATRIX
                    || name == TO_ARRAY
                    || name == TO_VEC
                    || name == "det"
                    || name == LIST_LEN
                    || name == LIST_CAPACITY
                    || name == LIST_GET
                    || name == LIST_PUSH
                    || name == MUTEX_LOCK
                    || name == MUTEX_TRY_LOCK
                {
                    self.clear_consumed_custom_drop_locals_expr(
                        receiver, locals, drop_flags, false,
                    );
                    for arg in args {
                        self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                    }
                    return;
                }
                let recv_ty = self.expr_codegen_ty(receiver, locals);
                let symbol = method_symbol(method_receiver_owner_ty(&recv_ty), name);
                if let Some(sig) = self.fn_sigs.get(&symbol) {
                    let params = sig.params.clone();
                    let receiver_consumed =
                        params.first().is_some_and(|t| !matches!(t, Ty::Ptr(_)));
                    self.clear_consumed_custom_drop_locals_expr(
                        receiver,
                        locals,
                        drop_flags,
                        receiver_consumed,
                    );
                    for (arg, param_ty) in args.iter().zip(params.iter().skip(1)) {
                        self.clear_consumed_custom_drop_locals_expr(
                            arg,
                            locals,
                            drop_flags,
                            !matches!(param_ty, Ty::Ptr(_)),
                        );
                    }
                    return;
                }
                self.clear_consumed_custom_drop_locals_expr(receiver, locals, drop_flags, true);
                for arg in args {
                    self.clear_consumed_custom_drop_locals_expr(arg, locals, drop_flags, true);
                }
            }
            Expr::StructLit { fields, .. }
            | Expr::VectorLit { fields, .. }
            | Expr::EnumStruct { fields, .. } => {
                for (_, value) in fields {
                    self.clear_consumed_custom_drop_locals_expr(value, locals, drop_flags, true);
                }
            }
            Expr::AnonVectorLit(elems) | Expr::ArrayLit(elems) => {
                for elem in elems {
                    self.clear_consumed_custom_drop_locals_expr(elem, locals, drop_flags, true);
                }
            }
            Expr::Match { scrutinee, arms } => {
                self.clear_consumed_custom_drop_locals_expr(scrutinee, locals, drop_flags, true);
                for (_, arm) in arms {
                    self.clear_consumed_custom_drop_locals_expr(arm, locals, drop_flags, true);
                }
            }
            Expr::Closure {
                is_move,
                params,
                body,
                ..
            } => {
                if *is_move {
                    let outer_env = locals
                        .iter()
                        .map(|(name, (ty, _))| (name.clone(), ty.clone()))
                        .collect::<HashMap<_, _>>();
                    let (captures, _) = closure_capture_names(params, body, &outer_env);
                    for name in captures {
                        if let Some((ty, _)) = locals.get(&name) {
                            if !self.is_copy_like_ty(ty) {
                                self.clear_drop_flag_for_local(&name, locals, drop_flags, true);
                            }
                        }
                    }
                }
            }
            Expr::Quant { body } | Expr::Gpu { body } => {
                for st in &body.stmts {
                    self.clear_consumed_custom_drop_locals_stmt(st, locals, drop_flags);
                }
                if let Some(tail) = &body.tail {
                    self.clear_consumed_custom_drop_locals_expr(tail, locals, drop_flags, true);
                }
            }
        }
    }

    fn clear_consumed_custom_drop_locals_stmt(
        &mut self,
        stmt: &Stmt,
        locals: &HashMap<String, (Ty, String)>,
        drop_flags: &HashMap<String, String>,
    ) {
        match stmt {
            Stmt::Let {
                init: Some(init), ..
            }
            | Stmt::Expr(init)
            | Stmt::Return(init) => {
                self.clear_consumed_custom_drop_locals_expr(init, locals, drop_flags, true);
            }
            Stmt::Assign { value, .. } => {
                self.clear_consumed_custom_drop_locals_expr(value, locals, drop_flags, true);
            }
            Stmt::If { cond, then_block } => {
                self.clear_consumed_custom_drop_locals_expr(cond, locals, drop_flags, true);
                for st in &then_block.stmts {
                    self.clear_consumed_custom_drop_locals_stmt(st, locals, drop_flags);
                }
            }
            Stmt::While { cond, body } => {
                self.clear_consumed_custom_drop_locals_expr(cond, locals, drop_flags, true);
                for st in &body.stmts {
                    self.clear_consumed_custom_drop_locals_stmt(st, locals, drop_flags);
                }
            }
            Stmt::For {
                start, end, body, ..
            } => {
                self.clear_consumed_custom_drop_locals_expr(start, locals, drop_flags, true);
                self.clear_consumed_custom_drop_locals_expr(end, locals, drop_flags, true);
                for st in &body.stmts {
                    self.clear_consumed_custom_drop_locals_stmt(st, locals, drop_flags);
                }
            }
            Stmt::Loop { body } | Stmt::Quant { body } | Stmt::Gpu { body } => {
                for st in &body.stmts {
                    self.clear_consumed_custom_drop_locals_stmt(st, locals, drop_flags);
                }
            }
            Stmt::Let { init: None, .. } | Stmt::Break => {}
        }
    }

    fn emit_fn_value_call(
        &mut self,
        callee: &str,
        params: &[Ty],
        ret: &Ty,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let code = self.fresh();
        let env = self.fresh();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 0",
            code,
            fn_value_ll_ty(),
            callee
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = extractvalue {} {}, 1",
            env,
            fn_value_ll_ty(),
            callee
        )
        .unwrap();
        let mut arg_strs = vec![format!("ptr %{env}")];
        for (arg, param_ty) in args.iter().zip(params) {
            let (arg_ty, arg_val) = self.emit_expr(arg, locals, Some(param_ty));
            debug_assert!(types_match(&arg_ty, param_ty));
            arg_strs.push(format!("{} {}", llvm_ty(param_ty, self.structs), arg_val));
        }
        if matches!(ret, Ty::Unit) {
            writeln!(self.out, "  call void %{}({})", code, arg_strs.join(", ")).unwrap();
            (Ty::Unit, String::new())
        } else {
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = call {} %{}({})",
                tmp,
                llvm_ty(ret, self.structs),
                code,
                arg_strs.join(", ")
            )
            .unwrap();
            (ret.clone(), format!("%{tmp}"))
        }
    }

    fn emit_abort_if_pthread_error(&mut self, status: &str, label: &str) {
        let failed = self.fresh();
        let abort_lbl = self.fresh_label(label);
        let ok_lbl = self.fresh_label(&format!("{label}.ok"));
        writeln!(self.out, "  %{} = icmp ne i32 {}, 0", failed, status).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            failed, abort_lbl, ok_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();
        writeln!(self.out, "{}:", ok_lbl).unwrap();
    }

    fn emit_thread_spawn(
        &mut self,
        target: &Expr,
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let entry_ty = Ty::Fn(Vec::new(), Box::new(Ty::Unit));
        let (fn_ty, fn_value) = self.emit_expr(target, locals, Some(&entry_ty));
        debug_assert!(types_match(&fn_ty, &entry_ty));

        let payload = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 32)", payload).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            fn_value_ll_ty(),
            fn_value,
            payload
        )
        .unwrap();

        let thread = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 8)", thread).unwrap();
        let rc = self.fresh();
        writeln!(
            self.out,
            "  %{} = call i32 @pthread_create(ptr %{}, ptr null, ptr @nialang.thread.entry, ptr %{})",
            rc, thread, payload
        )
        .unwrap();
        self.emit_abort_if_pthread_error(&format!("%{rc}"), "thread.spawn.abort");
        (Ty::Thread, format!("%{thread}"))
    }

    fn emit_thread_join(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        debug_assert_eq!(args.len(), 1);
        let (thread_ty, thread) = self.emit_expr(&args[0], locals, Some(&Ty::Thread));
        debug_assert!(matches!(thread_ty, Ty::Thread));
        let thread_id = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr {}", thread_id, thread).unwrap();
        let rc = self.fresh();
        writeln!(
            self.out,
            "  %{} = call i32 {}(ptr {}, ptr null)",
            rc,
            pthread_join_symbol(),
            format!("%{thread_id}")
        )
        .unwrap();
        self.emit_abort_if_pthread_error(&format!("%{rc}"), "thread.join.abort");
        writeln!(self.out, "  call void @free(ptr {})", thread).unwrap();
        (Ty::Unit, String::new())
    }

    fn emit_thread_detach_drop(&mut self, thread: &str) {
        let thread_id = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr {}", thread_id, thread).unwrap();
        writeln!(self.out, "  call i32 @pthread_detach(ptr %{})", thread_id).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", thread).unwrap();
    }

    fn ensure_function_value_wrapper(&mut self, name: &str) -> String {
        let wrapper = format!("__nia_fn_value_{}", sanitize(name));
        if !self.closures.borrow_mut().wrappers.insert(wrapper.clone()) {
            return wrapper;
        }
        let (sig_params, sig_ret) = {
            let sig = self.fn_sigs.get(name).expect("typechecked function value");
            (sig.params.clone(), sig.ret.clone())
        };
        let ret_ty = sig_ret.clone().unwrap_or(Ty::Unit);
        let ret_ll = if matches!(ret_ty, Ty::Unit) {
            "void".to_string()
        } else {
            llvm_ty(&ret_ty, self.structs)
        };
        let mut params = vec!["ptr %__env".to_string()];
        for (idx, ty) in sig_params.iter().enumerate() {
            params.push(format!("{} %arg{}", llvm_ty(ty, self.structs), idx));
        }
        let mut def = String::new();
        writeln!(
            def,
            "define internal {} @{}({}) {{",
            ret_ll,
            sanitize(&wrapper),
            params.join(", ")
        )
        .unwrap();
        writeln!(def, "entry:").unwrap();
        let call_args = sig_params
            .iter()
            .enumerate()
            .map(|(idx, ty)| format!("{} %arg{}", llvm_ty(ty, self.structs), idx))
            .collect::<Vec<_>>()
            .join(", ");
        if matches!(ret_ty, Ty::Unit) {
            writeln!(def, "  call void @{}({})", sanitize(name), call_args).unwrap();
            writeln!(def, "  ret void").unwrap();
        } else {
            writeln!(
                def,
                "  %ret = call {} @{}({})",
                llvm_ty(&ret_ty, self.structs),
                sanitize(name),
                call_args
            )
            .unwrap();
            writeln!(def, "  ret {} %ret", llvm_ty(&ret_ty, self.structs)).unwrap();
        }
        writeln!(def, "}}").unwrap();
        self.closures.borrow_mut().defs.push(def);
        wrapper
    }

    fn emit_closure_drop_def(&self, name: &str, captures: &[(String, Ty)]) -> String {
        let mut drop_gen = Gen::new(
            self.structs,
            self.enums,
            self.vectors,
            self.fn_sigs,
            self.str_lit_syms,
            self.mode,
            self.closures.clone(),
        );
        let env_ll = Self::closure_env_ll_ty(captures);
        writeln!(
            drop_gen.out,
            "define internal void @{}(ptr %env) {{",
            sanitize(name)
        )
        .unwrap();
        writeln!(drop_gen.out, "entry:").unwrap();
        for (idx, (_, capture_ty)) in captures.iter().enumerate().rev() {
            if !drop_gen.is_language_drop_ty(capture_ty) {
                continue;
            }
            let field_ptr = drop_gen.fresh();
            writeln!(
                drop_gen.out,
                "  %{} = getelementptr inbounds {}, ptr %env, i32 0, i32 {}",
                field_ptr, env_ll, idx
            )
            .unwrap();
            let value = drop_gen.fresh();
            writeln!(
                drop_gen.out,
                "  %{} = load {}, ptr %{}",
                value,
                llvm_ty(capture_ty, drop_gen.structs),
                field_ptr
            )
            .unwrap();
            drop_gen.emit_drop_value(capture_ty, format!("%{value}"));
        }
        writeln!(drop_gen.out, "  call void @free(ptr %env)").unwrap();
        writeln!(drop_gen.out, "  ret void").unwrap();
        writeln!(drop_gen.out, "}}").unwrap();
        drop_gen.out
    }

    fn closure_captures_cloneable(&self, captures: &[(String, Ty)]) -> bool {
        captures
            .iter()
            .all(|(_, ty)| supports_clone_method_ty(ty, self.structs, self.enums, self.vectors))
    }

    fn emit_closure_clone_def(&self, name: &str, captures: &[(String, Ty)]) -> String {
        let mut clone_gen = Gen::new(
            self.structs,
            self.enums,
            self.vectors,
            self.fn_sigs,
            self.str_lit_syms,
            self.mode,
            self.closures.clone(),
        );
        let env_ll = Self::closure_env_ll_ty(captures);
        writeln!(
            clone_gen.out,
            "define internal ptr @{}(ptr %env) {{",
            sanitize(name)
        )
        .unwrap();
        writeln!(clone_gen.out, "entry:").unwrap();
        let sz = clone_gen.emit_sizeof_ll_ty_i64(&env_ll);
        let out = clone_gen.fresh();
        writeln!(clone_gen.out, "  %{} = call ptr @malloc(i64 {})", out, sz).unwrap();
        for (idx, (_, capture_ty)) in captures.iter().enumerate() {
            let src_ptr = clone_gen.fresh();
            writeln!(
                clone_gen.out,
                "  %{} = getelementptr inbounds {}, ptr %env, i32 0, i32 {}",
                src_ptr, env_ll, idx
            )
            .unwrap();
            let raw = clone_gen.fresh();
            writeln!(
                clone_gen.out,
                "  %{} = load {}, ptr %{}",
                raw,
                llvm_ty(capture_ty, clone_gen.structs),
                src_ptr
            )
            .unwrap();
            let cloned = clone_gen.emit_clone_value(capture_ty, &format!("%{raw}"));
            let dst_ptr = clone_gen.fresh();
            writeln!(
                clone_gen.out,
                "  %{} = getelementptr inbounds {}, ptr %{}, i32 0, i32 {}",
                dst_ptr, env_ll, out, idx
            )
            .unwrap();
            writeln!(
                clone_gen.out,
                "  store {} {}, ptr %{}",
                llvm_ty(capture_ty, clone_gen.structs),
                cloned,
                dst_ptr
            )
            .unwrap();
        }
        writeln!(clone_gen.out, "  ret ptr %{}", out).unwrap();
        writeln!(clone_gen.out, "}}").unwrap();
        clone_gen.out
    }

    fn emit_closure_literal(
        &mut self,
        _is_move: bool,
        params: &[(String, Option<Ty>)],
        explicit_ret: Option<&Ty>,
        body: &Block,
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        let expected = match hint {
            Some(Ty::Fn(params, ret)) => Some((params.as_slice(), ret.as_ref())),
            _ => None,
        };
        let param_tys = params
            .iter()
            .enumerate()
            .map(|(idx, (_, ann))| match (ann, expected) {
                (Some(ty), _) => ty.clone(),
                (None, Some((expected_params, _))) => expected_params[idx].clone(),
                (None, None) => unreachable!("typechecked closure parameter"),
            })
            .collect::<Vec<_>>();
        let ret_ty = match (explicit_ret, expected) {
            (Some(ty), _) => ty.clone(),
            (None, Some((_, ret))) => ret.clone(),
            (None, None) => unreachable!("typechecked closure return"),
        };
        let id = {
            let mut state = self.closures.borrow_mut();
            let id = state.next_id;
            state.next_id += 1;
            id
        };
        let name = format!("__nia_closure_{id}");
        let outer_env = locals
            .iter()
            .map(|(name, (ty, _))| (name.clone(), ty.clone()))
            .collect::<HashMap<_, _>>();
        let (capture_names, _) = closure_capture_names(params, body, &outer_env);
        let captures = capture_names
            .iter()
            .filter_map(|name| outer_env.get(name).map(|ty| (name.clone(), ty.clone())))
            .collect::<Vec<_>>();
        let env_ll = Self::closure_env_ll_ty(&captures);
        let (env_value, drop_value, clone_value) = if captures.is_empty() {
            ("null".to_string(), "null".to_string(), "null".to_string())
        } else {
            let sz = self.emit_sizeof_ll_ty_i64(&env_ll);
            let env = self.fresh();
            writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", env, sz).unwrap();
            for (idx, (capture_name, capture_ty)) in captures.iter().enumerate() {
                let (_, capture_val) =
                    self.emit_expr(&Expr::Ident(capture_name.clone()), locals, Some(capture_ty));
                let field_ptr = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr %{}, i32 0, i32 {}",
                    field_ptr, env_ll, env, idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store {} {}, ptr %{}",
                    llvm_ty(capture_ty, self.structs),
                    capture_val,
                    field_ptr
                )
                .unwrap();
            }
            let drop_name = format!("{name}_drop");
            let drop_def = self.emit_closure_drop_def(&drop_name, &captures);
            self.closures.borrow_mut().defs.push(drop_def);
            let clone_value = if self.closure_captures_cloneable(&captures) {
                let clone_name = format!("{name}_clone");
                let clone_def = self.emit_closure_clone_def(&clone_name, &captures);
                self.closures.borrow_mut().defs.push(clone_def);
                format!("@{}", sanitize(&clone_name))
            } else {
                "null".to_string()
            };
            (
                format!("%{env}"),
                format!("@{}", sanitize(&drop_name)),
                clone_value,
            )
        };
        let f = FnDef {
            name: name.clone(),
            is_extern: false,
            is_quantum: false,
            params: std::iter::once((CLOSURE_ENV_PARAM.to_string(), Ty::Ptr(Box::new(Ty::Unit))))
                .chain(
                    params
                        .iter()
                        .zip(&param_tys)
                        .map(|((name, _), ty)| (name.clone(), ty.clone())),
                )
                .collect(),
            ret: if matches!(ret_ty, Ty::Unit) {
                None
            } else {
                Some(ret_ty.clone())
            },
            body: body.clone(),
            closure_captures: captures,
        };
        let def = Gen::new(
            self.structs,
            self.enums,
            self.vectors,
            self.fn_sigs,
            self.str_lit_syms,
            self.mode,
            self.closures.clone(),
        )
        .emit_fn(&f);
        self.closures.borrow_mut().defs.push(def);
        (
            Ty::Fn(param_tys, Box::new(ret_ty)),
            self.emit_fn_value(
                &format!("@{}", sanitize(&name)),
                &env_value,
                &drop_value,
                &clone_value,
            ),
        )
    }

    /// Emits pointer to first byte of global string constant.
    fn fmt_ptr(&mut self, sym: &str, size: u32) -> String {
        let tmp = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds [{} x i8], ptr @{}, i64 0, i64 0",
            tmp, size, sym
        )
        .unwrap();
        format!("%{tmp}")
    }

    /// Emits plain `printf` call for constant text fragment.
    fn emit_printf_text(&mut self, sym: &str, size: u32) {
        let p = self.fmt_ptr(sym, size);
        if self.mode == CodegenMode::QirRunner {
            let qstr = self.fresh();
            writeln!(
                self.out,
                "  %{} = call ptr @__quantum__rt__string_create(ptr {})",
                qstr, p
            )
            .unwrap();
            writeln!(
                self.out,
                "  call void @__quantum__rt__message(ptr %{})",
                qstr
            )
            .unwrap();
            return;
        }
        writeln!(self.out, "  call i32 (ptr, ...) @printf(ptr {})", p).unwrap();
    }

    /// Convenience wrapper for dynamic known-at-runtime `usize` sizes.
    fn emit_printf_text_dynamic(&mut self, sym: &str, size: usize) {
        self.emit_printf_text(sym, size as u32);
    }

    /// Looks up struct definition by source-level name.
    fn struct_def(&self, name: &str) -> Option<&StructDef> {
        self.structs.iter().find(|s| s.name == name)
    }

    fn vector_def(&self, name: &str) -> Option<&VectorDef> {
        self.vectors.iter().find(|v| v.name == name)
    }

    fn enum_tag(&self, enum_name: &str, variant: &str) -> Option<i32> {
        let e = self.enums.iter().find(|e| e.name == enum_name)?;
        e.variants
            .iter()
            .position(|v| v.name == variant)
            .map(|i| i as i32)
    }

    fn enum_variant_index(&self, enum_name: &str, variant: &str) -> Option<usize> {
        let e = self.enums.iter().find(|e| e.name == enum_name)?;
        e.variants.iter().position(|v| v.name == variant)
    }

    fn enum_variant_def(
        &self,
        enum_name: &str,
        variant: &str,
    ) -> Option<&crate::ast::EnumVariantDef> {
        let e = self.enums.iter().find(|e| e.name == enum_name)?;
        e.variants.iter().find(|v| v.name == variant)
    }

    fn emit_enum_unit_variant(&mut self, enum_name: &str, variant: &str) -> (Ty, String) {
        let tag = self
            .enum_tag(enum_name, variant)
            .expect("typechecked enum variant");
        let enum_ll = format!("%enum.{}", sanitize(enum_name));
        let idx = self
            .enum_variant_index(enum_name, variant)
            .expect("typechecked enum idx");
        let vdef = self
            .enum_variant_def(enum_name, variant)
            .expect("typechecked enum variant")
            .clone();
        let payload_ll = enum_variant_payload_ty(&vdef, self.structs);
        let with_tag = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} poison, i32 {}, 0",
            with_tag, enum_ll, tag
        )
        .unwrap();
        let with_payload = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} %{}, {} zeroinitializer, {}",
            with_payload,
            enum_ll,
            with_tag,
            payload_ll,
            idx + 1
        )
        .unwrap();
        (Ty::Enum(enum_name.to_string()), format!("%{with_payload}"))
    }

    fn emit_enum_tuple_variant(
        &mut self,
        enum_name: &str,
        variant: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let tag = self
            .enum_tag(enum_name, variant)
            .expect("typechecked enum variant");
        let idx = self
            .enum_variant_index(enum_name, variant)
            .expect("typechecked enum idx");
        let vdef = self
            .enum_variant_def(enum_name, variant)
            .expect("typechecked enum variant")
            .clone();
        let EnumVariantFields::Tuple(ts) = vdef.fields else {
            unreachable!("typechecked tuple variant")
        };
        let enum_ll = format!("%enum.{}", sanitize(enum_name));
        let with_tag = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} poison, i32 {}, 0",
            with_tag, enum_ll, tag
        )
        .unwrap();
        let payload_val = if ts.len() == 1 {
            let (_, v) = self.emit_expr(&args[0], locals, Some(&ts[0]));
            (llvm_ty(&ts[0], self.structs), v)
        } else {
            let payload_ll = if ts.len() == 1 {
                llvm_ty(&ts[0], self.structs)
            } else {
                let inner = ts
                    .iter()
                    .map(|t| llvm_ty(t, self.structs))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {inner} }}")
            };
            let mut agg = "poison".to_string();
            for (i, (a, t)) in args.iter().zip(ts.iter()).enumerate() {
                let (_, av) = self.emit_expr(a, locals, Some(t));
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} {}, {} {}, {}",
                    tmp,
                    payload_ll,
                    agg,
                    llvm_ty(t, self.structs),
                    av,
                    i
                )
                .unwrap();
                agg = format!("%{tmp}");
            }
            (payload_ll, agg)
        };
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} %{}, {} {}, {}",
            out,
            enum_ll,
            with_tag,
            payload_val.0,
            payload_val.1,
            idx + 1
        )
        .unwrap();
        (Ty::Enum(enum_name.to_string()), format!("%{out}"))
    }

    fn emit_enum_struct_variant(
        &mut self,
        enum_name: &str,
        variant: &str,
        fields: &[(String, Expr)],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let tag = self
            .enum_tag(enum_name, variant)
            .expect("typechecked enum variant");
        let idx = self
            .enum_variant_index(enum_name, variant)
            .expect("typechecked enum idx");
        let vdef = self
            .enum_variant_def(enum_name, variant)
            .expect("typechecked enum variant")
            .clone();
        let EnumVariantFields::Struct(fs) = vdef.fields else {
            unreachable!("typechecked struct variant")
        };
        let enum_ll = format!("%enum.{}", sanitize(enum_name));
        let with_tag = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} poison, i32 {}, 0",
            with_tag, enum_ll, tag
        )
        .unwrap();
        let payload_ll = {
            let inner = fs
                .iter()
                .map(|(_, t)| llvm_ty(t, self.structs))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {inner} }}")
        };
        let mut agg = "poison".to_string();
        for (i, (fname, fty)) in fs.iter().enumerate() {
            let (_, fe) = fields
                .iter()
                .find(|(n, _)| n == fname)
                .expect("typechecked");
            let (_, fv) = self.emit_expr(fe, locals, Some(fty));
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                payload_ll,
                agg,
                llvm_ty(fty, self.structs),
                fv,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue {} %{}, {} {}, {}",
            out,
            enum_ll,
            with_tag,
            payload_ll,
            agg,
            idx + 1
        )
        .unwrap();
        (Ty::Enum(enum_name.to_string()), format!("%{out}"))
    }

    fn emit_sizeof_i64(&mut self, ty: &Ty) -> String {
        let ty_ll = llvm_ty(ty, self.structs);
        self.emit_sizeof_ll_ty_i64(&ty_ll)
    }

    fn emit_sizeof_ll_ty_i64(&mut self, ty_ll: &str) -> String {
        let gep = self.fresh();
        let sz = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr {}, ptr null, i64 1",
            gep, ty_ll
        )
        .unwrap();
        writeln!(self.out, "  %{} = ptrtoint ptr %{} to i64", sz, gep).unwrap();
        format!("%{sz}")
    }

    fn arc_inner_ll_ty(&self, elem_ty: &Ty) -> String {
        format!("{{ i64, {} }}", llvm_ty(elem_ty, self.structs))
    }

    fn arc_refcount_ptr(&mut self, arc: &str, elem_ty: &Ty) -> String {
        let inner_ll = self.arc_inner_ll_ty(elem_ty);
        let ptr = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i32 0, i32 0",
            ptr, inner_ll, arc
        )
        .unwrap();
        format!("%{ptr}")
    }

    fn arc_value_ptr(&mut self, arc: &str, elem_ty: &Ty) -> String {
        let inner_ll = self.arc_inner_ll_ty(elem_ty);
        let ptr = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i32 0, i32 1",
            ptr, inner_ll, arc
        )
        .unwrap();
        format!("%{ptr}")
    }

    fn emit_arc_new_value(&mut self, elem_ty: &Ty, value: &str) -> String {
        let inner_ll = self.arc_inner_ll_ty(elem_ty);
        let size = self.emit_sizeof_ll_ty_i64(&inner_ll);
        let raw = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", raw, size).unwrap();
        let arc = format!("%{raw}");
        let refcount_ptr = self.arc_refcount_ptr(&arc, elem_ty);
        writeln!(self.out, "  store i64 1, ptr {}, align 8", refcount_ptr).unwrap();
        let value_ptr = self.arc_value_ptr(&arc, elem_ty);
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(elem_ty, self.structs),
            value,
            value_ptr
        )
        .unwrap();
        arc
    }

    fn emit_arc_clone_value(&mut self, elem_ty: &Ty, arc: &str) -> String {
        let refcount_ptr = self.arc_refcount_ptr(arc, elem_ty);
        let old = self.fresh();
        writeln!(
            self.out,
            "  %{} = atomicrmw add ptr {}, i64 1 monotonic, align 8",
            old, refcount_ptr
        )
        .unwrap();
        arc.to_string()
    }

    fn emit_arc_drop_value(&mut self, elem_ty: &Ty, arc: &str) {
        let refcount_ptr = self.arc_refcount_ptr(arc, elem_ty);
        let old = self.fresh();
        let is_last = self.fresh();
        let drop_lbl = self.fresh_label("arc.drop.last");
        let cont_lbl = self.fresh_label("arc.drop.cont");
        writeln!(
            self.out,
            "  %{} = atomicrmw sub ptr {}, i64 1 release, align 8",
            old, refcount_ptr
        )
        .unwrap();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 1", is_last, old).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            is_last, drop_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", drop_lbl).unwrap();
        writeln!(self.out, "  fence acquire").unwrap();
        if self.is_language_drop_ty(elem_ty) {
            let value_ptr = self.arc_value_ptr(arc, elem_ty);
            let loaded = self.fresh();
            writeln!(
                self.out,
                "  %{} = load {}, ptr {}",
                loaded,
                llvm_ty(elem_ty, self.structs),
                value_ptr
            )
            .unwrap();
            self.emit_drop_value(elem_ty, format!("%{loaded}"));
        }
        writeln!(self.out, "  call void @free(ptr {})", arc).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn mutex_inner_ll_ty(&self, elem_ty: &Ty) -> String {
        format!("{{ [64 x i8], {} }}", llvm_ty(elem_ty, self.structs))
    }

    fn mutex_lock_ptr(&mut self, mutex: &str, elem_ty: &Ty) -> String {
        let inner_ll = self.mutex_inner_ll_ty(elem_ty);
        let ptr = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i32 0, i32 0",
            ptr, inner_ll, mutex
        )
        .unwrap();
        format!("%{ptr}")
    }

    fn mutex_value_ptr(&mut self, mutex: &str, elem_ty: &Ty) -> String {
        let inner_ll = self.mutex_inner_ll_ty(elem_ty);
        let ptr = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i32 0, i32 1",
            ptr, inner_ll, mutex
        )
        .unwrap();
        format!("%{ptr}")
    }

    fn emit_mutex_new_value(&mut self, elem_ty: &Ty, value: &str) -> String {
        let inner_ll = self.mutex_inner_ll_ty(elem_ty);
        let size = self.emit_sizeof_ll_ty_i64(&inner_ll);
        let raw = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", raw, size).unwrap();
        let mutex = format!("%{raw}");
        let lock_ptr = self.mutex_lock_ptr(&mutex, elem_ty);
        let rc = self.fresh();
        writeln!(
            self.out,
            "  %{} = call i32 @pthread_mutex_init(ptr {}, ptr null)",
            rc, lock_ptr
        )
        .unwrap();
        self.emit_abort_if_pthread_error(&format!("%{rc}"), "mutex.init.abort");
        let value_ptr = self.mutex_value_ptr(&mutex, elem_ty);
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(elem_ty, self.structs),
            value,
            value_ptr
        )
        .unwrap();
        mutex
    }

    fn emit_mutex_lock_value(&mut self, elem_ty: &Ty, mutex: &str) -> String {
        let lock_ptr = self.mutex_lock_ptr(mutex, elem_ty);
        let rc = self.fresh();
        writeln!(
            self.out,
            "  %{} = call i32 @pthread_mutex_lock(ptr {})",
            rc, lock_ptr
        )
        .unwrap();
        self.emit_abort_if_pthread_error(&format!("%{rc}"), "mutex.lock.abort");
        mutex.to_string()
    }

    fn emit_mutex_try_lock_value(&mut self, elem_ty: &Ty, mutex: &str) -> String {
        let lock_ptr = self.mutex_lock_ptr(mutex, elem_ty);
        let rc = self.fresh();
        let some_lbl = self.fresh_label("mutex.try_lock.some");
        let busy_lbl = self.fresh_label("mutex.try_lock.busy");
        let none_lbl = self.fresh_label("mutex.try_lock.none");
        let abort_lbl = self.fresh_label("mutex.try_lock.abort");
        let cont_lbl = self.fresh_label("mutex.try_lock.cont");
        let result_slot = self.fresh();
        writeln!(self.out, "  %{} = alloca ptr", result_slot).unwrap();
        writeln!(
            self.out,
            "  %{} = call i32 @pthread_mutex_trylock(ptr {})",
            rc, lock_ptr
        )
        .unwrap();
        let is_ok = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i32 %{}, 0", is_ok, rc).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            is_ok, some_lbl, busy_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", some_lbl).unwrap();
        let some = self.emit_sum_value(
            1,
            Some(&Ty::MutexGuard(Box::new(elem_ty.clone()))),
            Some(mutex),
        );
        writeln!(self.out, "  store ptr {}, ptr %{}", some, result_slot).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", busy_lbl).unwrap();
        let is_busy = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i32 %{}, 16", is_busy, rc).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            is_busy, none_lbl, abort_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", none_lbl).unwrap();
        let none = self.emit_sum_value(0, None, None);
        writeln!(self.out, "  store ptr {}, ptr %{}", none, result_slot).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        let out = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr %{}", out, result_slot).unwrap();
        format!("%{out}")
    }

    fn emit_mutex_guard_drop_value(&mut self, elem_ty: &Ty, guard: &str) {
        let lock_ptr = self.mutex_lock_ptr(guard, elem_ty);
        let rc = self.fresh();
        writeln!(
            self.out,
            "  %{} = call i32 @pthread_mutex_unlock(ptr {})",
            rc, lock_ptr
        )
        .unwrap();
        self.emit_abort_if_pthread_error(&format!("%{rc}"), "mutex.unlock.abort");
    }

    fn emit_mutex_drop_value(&mut self, elem_ty: &Ty, mutex: &str) {
        let lock_ptr = self.mutex_lock_ptr(mutex, elem_ty);
        let rc = self.fresh();
        writeln!(
            self.out,
            "  %{} = call i32 @pthread_mutex_destroy(ptr {})",
            rc, lock_ptr
        )
        .unwrap();
        self.emit_abort_if_pthread_error(&format!("%{rc}"), "mutex.destroy.abort");
        if self.is_language_drop_ty(elem_ty) {
            let value_ptr = self.mutex_value_ptr(mutex, elem_ty);
            let loaded = self.fresh();
            writeln!(
                self.out,
                "  %{} = load {}, ptr {}",
                loaded,
                llvm_ty(elem_ty, self.structs),
                value_ptr
            )
            .unwrap();
            self.emit_drop_value(elem_ty, format!("%{loaded}"));
        }
        writeln!(self.out, "  call void @free(ptr {})", mutex).unwrap();
    }

    fn emit_pi_value(&mut self) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = fadd double 3.1415926535897931e+00, 0.0",
            out
        )
        .unwrap();
        format!("%{out}")
    }

    fn emit_sum_value(
        &mut self,
        tag: i32,
        payload_ty: Option<&Ty>,
        payload_value: Option<&str>,
    ) -> String {
        let obj = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 16)", obj).unwrap();
        writeln!(self.out, "  store i32 {}, ptr %{}", tag, obj).unwrap();
        let payload_slot = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr i8, ptr %{}, i64 8",
            payload_slot, obj
        )
        .unwrap();
        if let (Some(payload_ty), Some(payload_value)) = (payload_ty, payload_value) {
            let payload_size = self.emit_sizeof_i64(payload_ty);
            let payload = self.fresh();
            writeln!(
                self.out,
                "  %{} = call ptr @malloc(i64 {})",
                payload, payload_size
            )
            .unwrap();
            writeln!(
                self.out,
                "  store {} {}, ptr %{}",
                llvm_ty(payload_ty, self.structs),
                payload_value,
                payload
            )
            .unwrap();
            writeln!(self.out, "  store ptr %{}, ptr %{}", payload, payload_slot).unwrap();
        } else {
            writeln!(self.out, "  store ptr null, ptr %{}", payload_slot).unwrap();
        }
        format!("%{obj}")
    }

    fn emit_option_none_value(&mut self, hint: Option<&Ty>) -> (Ty, String) {
        let Some(Ty::Option(elem_ty)) = hint else {
            unreachable!("typechecked None")
        };
        let value = self.emit_sum_value(0, None, None);
        (Ty::Option(elem_ty.clone()), value)
    }

    fn emit_option_some_value(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        let (elem_ty, payload) = match hint {
            Some(Ty::Option(elem_ty)) => {
                let elem_ty = elem_ty.as_ref().clone();
                let (_, payload) = self.emit_expr(&args[0], locals, Some(&elem_ty));
                (elem_ty, payload)
            }
            _ => self.emit_expr(&args[0], locals, None),
        };
        let value = self.emit_sum_value(1, Some(&elem_ty), Some(&payload));
        (Ty::Option(Box::new(elem_ty)), value)
    }

    fn emit_result_value(
        &mut self,
        tag: i32,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        let Some(Ty::ResultType(ok_ty, err_ty)) = hint else {
            unreachable!("typechecked Result constructor")
        };
        let payload_ty = if tag == 0 {
            ok_ty.as_ref()
        } else {
            err_ty.as_ref()
        };
        let (_, payload) = self.emit_expr(&args[0], locals, Some(payload_ty));
        let value = self.emit_sum_value(tag, Some(payload_ty), Some(&payload));
        (Ty::ResultType(ok_ty.clone(), err_ty.clone()), value)
    }

    fn emit_builtin_sum_match(
        &mut self,
        sum_value: &str,
        variants: &[(&str, Option<Ty>, i32)],
        arms: &[(MatchPattern, Expr)],
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        let tag_tmp = self.fresh();
        writeln!(self.out, "  %{} = load i32, ptr {}", tag_tmp, sum_value).unwrap();
        let cont_lbl = self.fresh_label("match.cont");
        let default_lbl = self.fresh_label("match.default");
        let arm_labels = arms
            .iter()
            .map(|_| self.fresh_label("match.arm"))
            .collect::<Vec<_>>();
        writeln!(
            self.out,
            "  switch i32 %{}, label %{} [",
            tag_tmp, default_lbl
        )
        .unwrap();
        for ((pat, _), lbl) in arms.iter().zip(&arm_labels) {
            let variant = match pat {
                MatchPattern::Unit { variant, .. }
                | MatchPattern::Tuple { variant, .. }
                | MatchPattern::Struct { variant, .. } => variant,
            };
            let tag = variants
                .iter()
                .find(|(candidate, _, _)| candidate == variant)
                .map(|(_, _, tag)| *tag)
                .expect("typechecked builtin sum variant");
            writeln!(self.out, "    i32 {}, label %{}", tag, lbl).unwrap();
        }
        writeln!(self.out, "  ]").unwrap();
        writeln!(self.out, "{}:", default_lbl).unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        let mut arm_vals: Vec<(String, String)> = Vec::new();
        let mut out_ty: Option<Ty> = None;
        for ((pat, arm_expr), lbl) in arms.iter().zip(&arm_labels) {
            writeln!(self.out, "{}:", lbl).unwrap();
            let mut arm_locals = locals.clone();
            let variant = match pat {
                MatchPattern::Unit { variant, .. }
                | MatchPattern::Tuple { variant, .. }
                | MatchPattern::Struct { variant, .. } => variant,
            };
            if let MatchPattern::Tuple { bindings, .. } = pat {
                let payload_ty = variants
                    .iter()
                    .find(|(candidate, _, _)| candidate == variant)
                    .and_then(|(_, payload_ty, _)| payload_ty.as_ref())
                    .expect("typechecked builtin sum payload");
                let payload_ptr = self.emit_sum_payload_ptr(sum_value);
                let loaded = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr {}",
                    loaded,
                    llvm_ty(payload_ty, self.structs),
                    payload_ptr
                )
                .unwrap();
                let ptr = format!("%{}.addr", sanitize(&bindings[0]));
                writeln!(
                    self.out,
                    "  {} = alloca {}",
                    ptr,
                    llvm_ty(payload_ty, self.structs)
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store {} %{}, ptr {}",
                    llvm_ty(payload_ty, self.structs),
                    loaded,
                    ptr
                )
                .unwrap();
                writeln!(self.out, "  call void @free(ptr {})", payload_ptr).unwrap();
                arm_locals.insert(bindings[0].clone(), (payload_ty.clone(), ptr));
            }
            let arm_hint = hint.or(out_ty.as_ref());
            let (at, av) = self.emit_expr(arm_expr, &arm_locals, arm_hint);
            if !matches!(at, Ty::Unit) {
                arm_vals.push((av.clone(), lbl.clone()));
            }
            if out_ty.is_none() {
                out_ty = Some(at.clone());
            }
            writeln!(self.out, "  call void @free(ptr {})", sum_value).unwrap();
            writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        }
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        let out_ty = out_ty.expect("typechecked non-empty match");
        if matches!(out_ty, Ty::Unit) {
            (Ty::Unit, String::new())
        } else {
            let phi = self.fresh();
            let ll = llvm_ty(&out_ty, self.structs);
            let incoming = arm_vals
                .iter()
                .map(|(v, l)| format!("[ {}, %{} ]", v, l))
                .collect::<Vec<_>>()
                .join(", ");
            writeln!(self.out, "  %{} = phi {} {}", phi, ll, incoming).unwrap();
            (out_ty, format!("%{phi}"))
        }
    }

    fn emit_f64_binop(&mut self, op: &str, left: &str, right: &str) -> String {
        let out = self.fresh();
        writeln!(self.out, "  %{} = {} double {}, {}", out, op, left, right).unwrap();
        format!("%{out}")
    }

    fn emit_f64_unary_lib_call(&mut self, name: &str, arg: &str) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = call double @{}(double {})",
            out, name, arg
        )
        .unwrap();
        format!("%{out}")
    }

    fn emit_value_ptr(&mut self, ty: &Ty, value: &str) -> String {
        let slot = self.fresh();
        let ll = llvm_ty(ty, self.structs);
        writeln!(self.out, "  %{} = alloca {}", slot, ll).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", ll, value, slot).unwrap();
        format!("%{slot}")
    }

    fn emit_digest_output_call(&mut self, callee: &str, args: &str) -> (Ty, String) {
        let ty = digest_ty();
        let ll = llvm_ty(&ty, self.structs);
        let out_ptr = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", out_ptr, ll).unwrap();
        writeln!(
            self.out,
            "  call void @{}({}, ptr %{})",
            callee, args, out_ptr
        )
        .unwrap();
        let out = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", out, ll, out_ptr).unwrap();
        (ty, format!("%{out}"))
    }

    fn emit_crypto_builtin_call(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> Option<(Ty, String)> {
        if name == SHA256 || name == MERKLE_LEAF_HASH {
            let (data_ty, data_val) = self.emit_expr(&args[0], locals, None);
            let Ty::Array(elem_ty, len) = &data_ty else {
                unreachable!("typechecked byte array crypto input")
            };
            debug_assert!(types_match(elem_ty, &Ty::U8));
            let data_ptr = self.emit_value_ptr(&data_ty, &data_val);
            let callee = if name == SHA256 {
                "nialang.crypto.sha256"
            } else {
                "nialang.crypto.merkle_leaf_hash"
            };
            return Some(
                self.emit_digest_output_call(callee, &format!("ptr {}, i64 {}", data_ptr, len)),
            );
        }

        if name == DIGEST_EQ {
            let expected = digest_ty();
            let (lt, lv) = self.emit_expr(&args[0], locals, Some(&expected));
            let (rt, rv) = self.emit_expr(&args[1], locals, Some(&expected));
            debug_assert!(types_match(&lt, &expected));
            debug_assert!(types_match(&rt, &expected));
            let lp = self.emit_value_ptr(&lt, &lv);
            let rp = self.emit_value_ptr(&rt, &rv);
            let out = self.fresh();
            writeln!(
                self.out,
                "  %{} = call i1 @nialang.crypto.digest_eq(ptr {}, ptr {})",
                out, lp, rp
            )
            .unwrap();
            return Some((Ty::Bool, format!("%{out}")));
        }

        if name == MERKLE_NODE_HASH {
            let expected = digest_ty();
            let (lt, lv) = self.emit_expr(&args[0], locals, Some(&expected));
            let (rt, rv) = self.emit_expr(&args[1], locals, Some(&expected));
            debug_assert!(types_match(&lt, &expected));
            debug_assert!(types_match(&rt, &expected));
            let lp = self.emit_value_ptr(&lt, &lv);
            let rp = self.emit_value_ptr(&rt, &rv);
            return Some(self.emit_digest_output_call(
                "nialang.crypto.merkle_node_hash",
                &format!("ptr {}, ptr {}", lp, rp),
            ));
        }

        if name == MERKLE_ROOT {
            let (digests_ty, digests_val) = self.emit_expr(&args[0], locals, None);
            let Ty::Array(elem_ty, count) = &digests_ty else {
                unreachable!("typechecked digest array")
            };
            debug_assert!(types_match(elem_ty, &digest_ty()));
            let digests_ptr = self.emit_value_ptr(&digests_ty, &digests_val);
            return Some(self.emit_digest_output_call(
                "nialang.crypto.merkle_root",
                &format!("ptr {}, i64 {}", digests_ptr, count),
            ));
        }

        if name == MERKLE_ROOT_FROM_DATA {
            let (data_ty, data_val) = self.emit_expr(&args[0], locals, None);
            let Ty::Array(row_ty, leaves) = &data_ty else {
                unreachable!("typechecked leaf data array")
            };
            let Ty::Array(elem_ty, leaf_len) = row_ty.as_ref() else {
                unreachable!("typechecked leaf data rows")
            };
            debug_assert!(types_match(elem_ty, &Ty::U8));
            let data_ptr = self.emit_value_ptr(&data_ty, &data_val);
            return Some(self.emit_digest_output_call(
                "nialang.crypto.merkle_root_from_data",
                &format!("ptr {}, i64 {}, i64 {}", data_ptr, leaf_len, leaves),
            ));
        }

        if name == MERKLE_VERIFY {
            let expected = digest_ty();
            let (root_ty, root_val) = self.emit_expr(&args[0], locals, Some(&expected));
            let (leaf_ty, leaf_val) = self.emit_expr(&args[1], locals, Some(&expected));
            let (_, index_val) = self.emit_expr(&args[2], locals, Some(&Ty::I32));
            let (proof_ty, proof_val) = self.emit_expr(&args[3], locals, None);
            let Ty::Array(proof_elem_ty, depth) = &proof_ty else {
                unreachable!("typechecked proof array")
            };
            debug_assert!(types_match(&root_ty, &expected));
            debug_assert!(types_match(&leaf_ty, &expected));
            debug_assert!(types_match(proof_elem_ty, &expected));
            let root_ptr = self.emit_value_ptr(&root_ty, &root_val);
            let leaf_ptr = self.emit_value_ptr(&leaf_ty, &leaf_val);
            let proof_ptr = self.emit_value_ptr(&proof_ty, &proof_val);
            let index64 = self.fresh();
            writeln!(self.out, "  %{} = sext i32 {} to i64", index64, index_val).unwrap();
            let out = self.fresh();
            writeln!(
                self.out,
                "  %{} = call i1 @nialang.crypto.merkle_verify(ptr {}, ptr {}, i64 %{}, ptr {}, i64 {})",
                out, root_ptr, leaf_ptr, index64, proof_ptr, depth
            )
            .unwrap();
            return Some((Ty::Bool, format!("%{out}")));
        }

        None
    }

    fn emit_complex_construct(&mut self, re: &str, im: &str) -> String {
        let with_re = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue %struct.Complex poison, double {}, 0",
            with_re, re
        )
        .unwrap();
        let with_im = self.fresh();
        writeln!(
            self.out,
            "  %{} = insertvalue %struct.Complex %{}, double {}, 1",
            with_im, with_re, im
        )
        .unwrap();
        format!("%{with_im}")
    }

    fn emit_complex_part(&mut self, value: &str, field: u32) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = extractvalue %struct.Complex {}, {}",
            out, value, field
        )
        .unwrap();
        format!("%{out}")
    }

    fn emit_complex_parts(&mut self, value: &str) -> (String, String) {
        (
            self.emit_complex_part(value, 0),
            self.emit_complex_part(value, 1),
        )
    }

    fn emit_complex_add_sub(&mut self, left: &str, right: &str, is_add: bool) -> String {
        let (lr, li) = self.emit_complex_parts(left);
        let (rr, ri) = self.emit_complex_parts(right);
        let op = if is_add { "fadd" } else { "fsub" };
        let re = self.emit_f64_binop(op, &lr, &rr);
        let im = self.emit_f64_binop(op, &li, &ri);
        self.emit_complex_construct(&re, &im)
    }

    fn emit_complex_mul_value(&mut self, left: &str, right: &str) -> String {
        let (lr, li) = self.emit_complex_parts(left);
        let (rr, ri) = self.emit_complex_parts(right);
        let lr_rr = self.emit_f64_binop("fmul", &lr, &rr);
        let li_ri = self.emit_f64_binop("fmul", &li, &ri);
        let lr_ri = self.emit_f64_binop("fmul", &lr, &ri);
        let li_rr = self.emit_f64_binop("fmul", &li, &rr);
        let re = self.emit_f64_binop("fsub", &lr_rr, &li_ri);
        let im = self.emit_f64_binop("fadd", &lr_ri, &li_rr);
        self.emit_complex_construct(&re, &im)
    }

    fn emit_complex_scale_value(&mut self, value: &str, scale: &str) -> String {
        let (re, im) = self.emit_complex_parts(value);
        let scaled_re = self.emit_f64_binop("fmul", &re, scale);
        let scaled_im = self.emit_f64_binop("fmul", &im, scale);
        self.emit_complex_construct(&scaled_re, &scaled_im)
    }

    fn emit_complex_div_value(&mut self, left: &str, right: &str) -> String {
        let (lr, li) = self.emit_complex_parts(left);
        let (rr, ri) = self.emit_complex_parts(right);
        let rr_rr = self.emit_f64_binop("fmul", &rr, &rr);
        let ri_ri = self.emit_f64_binop("fmul", &ri, &ri);
        let denom = self.emit_f64_binop("fadd", &rr_rr, &ri_ri);
        let lr_rr = self.emit_f64_binop("fmul", &lr, &rr);
        let li_ri = self.emit_f64_binop("fmul", &li, &ri);
        let re_num = self.emit_f64_binop("fadd", &lr_rr, &li_ri);
        let li_rr = self.emit_f64_binop("fmul", &li, &rr);
        let lr_ri = self.emit_f64_binop("fmul", &lr, &ri);
        let im_num = self.emit_f64_binop("fsub", &li_rr, &lr_ri);
        let re = self.emit_f64_binop("fdiv", &re_num, &denom);
        let im = self.emit_f64_binop("fdiv", &im_num, &denom);
        self.emit_complex_construct(&re, &im)
    }

    fn matrix_field_ptr(&mut self, matrix: &str, field: u32) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {{ ptr, i64, i64 }}, ptr {}, i32 0, i32 {}",
            out, matrix, field
        )
        .unwrap();
        format!("%{out}")
    }

    fn emit_matrix_header_alloc(&mut self, data: &str, rows: &str, cols: &str) -> String {
        let matrix = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 24)", matrix).unwrap();
        let matrix_ref = format!("%{matrix}");
        let data_ptr = self.matrix_field_ptr(&matrix_ref, 0);
        let rows_ptr = self.matrix_field_ptr(&matrix_ref, 1);
        let cols_ptr = self.matrix_field_ptr(&matrix_ref, 2);
        writeln!(self.out, "  store ptr {}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", rows, rows_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", cols, cols_ptr).unwrap();
        matrix_ref
    }

    fn matrix_load_i64_field(&mut self, matrix: &str, field: u32) -> String {
        let ptr = self.matrix_field_ptr(matrix, field);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn matrix_load_data_ptr(&mut self, matrix: &str) -> String {
        let ptr = self.matrix_field_ptr(matrix, 0);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn matrix_cell_ptr(&mut self, matrix: &str, row: &str, col: &str, elem_ty: &Ty) -> String {
        let cols = self.matrix_load_i64_field(matrix, 2);
        let row64 = self.fresh();
        let col64 = self.fresh();
        let row_offset = self.fresh();
        let index = self.fresh();
        let data = self.matrix_load_data_ptr(matrix);
        let cell = self.fresh();
        writeln!(self.out, "  %{} = sext i32 {} to i64", row64, row).unwrap();
        writeln!(self.out, "  %{} = sext i32 {} to i64", col64, col).unwrap();
        writeln!(self.out, "  %{} = mul i64 %{}, {}", row_offset, row64, cols).unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            index, row_offset, col64
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            data,
            index
        )
        .unwrap();
        format!("%{cell}")
    }

    fn heap_vector_field_ptr(&mut self, vector: &str, field: u32) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {{ ptr, i64 }}, ptr {}, i32 0, i32 {}",
            out, vector, field
        )
        .unwrap();
        format!("%{out}")
    }

    fn heap_vector_load_i64_field(&mut self, vector: &str, field: u32) -> String {
        let ptr = self.heap_vector_field_ptr(vector, field);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn heap_vector_load_data_ptr(&mut self, vector: &str) -> String {
        let ptr = self.heap_vector_field_ptr(vector, 0);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn heap_vector_cell_ptr(&mut self, vector: &str, idx: &str, elem_ty: &Ty) -> String {
        let idx64 = self.fresh();
        let data = self.heap_vector_load_data_ptr(vector);
        let cell = self.fresh();
        writeln!(self.out, "  %{} = sext i32 {} to i64", idx64, idx).unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            data,
            idx64
        )
        .unwrap();
        format!("%{cell}")
    }

    fn heap_vector_data_cell_ptr(&mut self, data: &str, idx: &str, elem_ty: &Ty) -> String {
        let cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 {}",
            cell,
            llvm_ty(elem_ty, self.structs),
            data,
            idx
        )
        .unwrap();
        format!("%{cell}")
    }

    fn list_field_ptr(&mut self, list: &str, field: u32) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {{ ptr, i64, i64 }}, ptr {}, i32 0, i32 {}",
            out, list, field
        )
        .unwrap();
        format!("%{out}")
    }

    fn list_load_i64_field(&mut self, list: &str, field: u32) -> String {
        let ptr = self.list_field_ptr(list, field);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn list_load_data_ptr(&mut self, list: &str) -> String {
        let ptr = self.list_field_ptr(list, 0);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn list_cell_ptr(&mut self, list: &str, idx: &str, elem_ty: &Ty) -> String {
        let idx64 = self.fresh();
        let len = self.list_load_i64_field(list, 1);
        let non_negative = self.fresh();
        let below_len = self.fresh();
        let in_bounds = self.fresh();
        let ok_lbl = self.fresh_label("list.get.bounds.ok");
        let abort_lbl = self.fresh_label("list.get.bounds.abort");
        writeln!(self.out, "  %{} = sext i32 {} to i64", idx64, idx).unwrap();
        writeln!(self.out, "  %{} = icmp sge i64 %{}, 0", non_negative, idx64).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            below_len, idx64, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = and i1 %{}, %{}",
            in_bounds, non_negative, below_len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            in_bounds, ok_lbl, abort_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();
        writeln!(self.out, "{}:", ok_lbl).unwrap();
        let data = self.list_load_data_ptr(list);
        self.heap_vector_data_cell_ptr(&data, &format!("%{idx64}"), elem_ty)
    }

    fn emit_list_alloc(&mut self, elem_ty: &Ty, capacity: &str) -> String {
        let elem_size = self.emit_sizeof_i64(elem_ty);
        let bytes = self.fresh();
        let data = self.fresh();
        let list = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 {}, {}",
            bytes, capacity, elem_size
        )
        .unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 %{})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 24)", list).unwrap();
        let data_ptr = self.list_field_ptr(&format!("%{list}"), 0);
        let len_ptr = self.list_field_ptr(&format!("%{list}"), 1);
        let cap_ptr = self.list_field_ptr(&format!("%{list}"), 2);
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 0, ptr {}", len_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", capacity, cap_ptr).unwrap();
        format!("%{list}")
    }

    fn emit_list_push_value(&mut self, list: &str, elem_ty: &Ty, value: &str) {
        let len = self.list_load_i64_field(list, 1);
        let cap = self.list_load_i64_field(list, 2);
        let full = self.fresh();
        let grow_lbl = self.fresh_label("list.push.grow");
        let insert_lbl = self.fresh_label("list.push.insert");
        writeln!(self.out, "  %{} = icmp eq i64 {}, {}", full, len, cap).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            full, grow_lbl, insert_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", grow_lbl).unwrap();
        let cap_is_zero = self.fresh();
        let doubled = self.fresh();
        let new_cap = self.fresh();
        let elem_size = self.emit_sizeof_i64(elem_ty);
        let bytes = self.fresh();
        let data = self.list_load_data_ptr(list);
        let new_data = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i64 {}, 0", cap_is_zero, cap).unwrap();
        writeln!(self.out, "  %{} = mul i64 {}, 2", doubled, cap).unwrap();
        writeln!(
            self.out,
            "  %{} = select i1 %{}, i64 4, i64 %{}",
            new_cap, cap_is_zero, doubled
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            bytes, new_cap, elem_size
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = call ptr @realloc(ptr {}, i64 %{})",
            new_data, data, bytes
        )
        .unwrap();
        let data_ptr = self.list_field_ptr(list, 0);
        let cap_ptr = self.list_field_ptr(list, 2);
        writeln!(self.out, "  store ptr %{}, ptr {}", new_data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr {}", new_cap, cap_ptr).unwrap();
        writeln!(self.out, "  br label %{}", insert_lbl).unwrap();

        writeln!(self.out, "{}:", insert_lbl).unwrap();
        let current_data = self.list_load_data_ptr(list);
        let cell = self.heap_vector_data_cell_ptr(&current_data, &len, elem_ty);
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(elem_ty, self.structs),
            value,
            cell
        )
        .unwrap();
        let next_len = self.fresh();
        let len_ptr = self.list_field_ptr(list, 1);
        writeln!(self.out, "  %{} = add i64 {}, 1", next_len, len).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr {}", next_len, len_ptr).unwrap();
    }

    fn emit_heap_vector_alloc(&mut self, elem_ty: &Ty, len: &str) -> (String, String) {
        let elem_size = self.emit_sizeof_i64(elem_ty);
        let bytes = self.fresh();
        let data = self.fresh();
        let vector = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", bytes, len, elem_size).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 %{})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 16)", vector).unwrap();

        let data_ptr = self.heap_vector_field_ptr(&format!("%{vector}"), 0);
        let len_ptr = self.heap_vector_field_ptr(&format!("%{vector}"), 1);
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", len, len_ptr).unwrap();
        (format!("%{vector}"), format!("%{data}"))
    }

    fn emit_heap_vector_shape_check(&mut self, left: &str, right: &str, label: &str) -> String {
        let left_len = self.heap_vector_load_i64_field(left, 1);
        let right_len = self.heap_vector_load_i64_field(right, 1);
        let same_len = self.fresh();
        let ok_lbl = self.fresh_label(&format!("heap.vector.{label}.len.ok"));
        let abort_lbl = self.fresh_label(&format!("heap.vector.{label}.len.abort"));
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            same_len, left_len, right_len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            same_len, ok_lbl, abort_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();
        writeln!(self.out, "{}:", ok_lbl).unwrap();
        left_len
    }

    fn matrix_data_cell_ptr(
        &mut self,
        data: &str,
        n: &str,
        row: &str,
        col: &str,
        elem_ty: &Ty,
    ) -> String {
        let row_offset = self.fresh();
        let index = self.fresh();
        let cell = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", row_offset, row, n).unwrap();
        writeln!(self.out, "  %{} = add i64 %{}, {}", index, row_offset, col).unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            data,
            index
        )
        .unwrap();
        format!("%{cell}")
    }

    /// Emits primitive scalar printing (`int`, `bool`, pointer) with optional newline.
    ///
    /// This function centralizes ABI-sensitive formatting:
    /// - sign/zero extension for small integers,
    /// - `%lld`/`%llu` for 64-bit lanes,
    /// - split hi/lo printing for 128-bit values,
    /// - `ptrtoint` for pointer hex output.
    fn emit_print_primitive(&mut self, ty: &Ty, v: &str, newline: bool) {
        if self.mode == CodegenMode::QirRunner {
            self.emit_qir_print_primitive(ty, v, newline);
            return;
        }

        match ty {
            Ty::I8 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i32", 4)
                } else {
                    ("nialang.std.fmt.i32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = sext i8 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::U8 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = zext i8 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::I16 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i32", 4)
                } else {
                    ("nialang.std.fmt.i32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = sext i16 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::U16 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = zext i16 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::I32 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i32", 4)
                } else {
                    ("nialang.std.fmt.i32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::U32 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::I64 | Ty::Isize => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i64", 6)
                } else {
                    ("nialang.std.fmt.i64.nn", 5)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::U64 | Ty::Usize => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u64", 6)
                } else {
                    ("nialang.std.fmt.u64.nn", 5)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::Bool => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = zext i1 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::String => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.str", 4)
                } else {
                    ("nialang.std.fmt.str.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, ptr {})",
                    p, v
                )
                .unwrap();
            }
            Ty::I128 | Ty::U128 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i128hex", 18)
                } else {
                    ("nialang.std.fmt.i128hex.nn", 17)
                };
                let p = self.fmt_ptr(sym, sz);
                let hi128 = self.fresh();
                let hi64 = self.fresh();
                let lo64 = self.fresh();
                writeln!(self.out, "  %{} = lshr i128 {}, 64", hi128, v).unwrap();
                writeln!(self.out, "  %{} = trunc i128 %{} to i64", hi64, hi128).unwrap();
                writeln!(self.out, "  %{} = trunc i128 {} to i64", lo64, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 %{}, i64 %{})",
                    p, hi64, lo64
                )
                .unwrap();
            }
            Ty::Ptr(_) => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.ptrhex", 8)
                } else {
                    ("nialang.std.fmt.ptrhex.nn", 7)
                };
                let p = self.fmt_ptr(sym, sz);
                let addr = self.fresh();
                writeln!(self.out, "  %{} = ptrtoint ptr {} to i64", addr, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 %{})",
                    p, addr
                )
                .unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.f64", 4)
                } else {
                    ("nialang.std.fmt.f64.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                match ty {
                    Ty::F64 => {
                        writeln!(
                            self.out,
                            "  call i32 (ptr, ...) @printf(ptr {}, double {})",
                            p, v
                        )
                        .unwrap();
                    }
                    Ty::F32 => {
                        let e = self.fresh();
                        writeln!(self.out, "  %{} = fpext float {} to double", e, v).unwrap();
                        writeln!(
                            self.out,
                            "  call i32 (ptr, ...) @printf(ptr {}, double %{})",
                            p, e
                        )
                        .unwrap();
                    }
                    Ty::F16 => {
                        let e = self.fresh();
                        writeln!(self.out, "  %{} = fpext half {} to double", e, v).unwrap();
                        writeln!(
                            self.out,
                            "  call i32 (ptr, ...) @printf(ptr {}, double %{})",
                            p, e
                        )
                        .unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            Ty::Array(_, _)
            | Ty::AtomicBool
            | Ty::AtomicI8
            | Ty::AtomicU8
            | Ty::AtomicI16
            | Ty::AtomicU16
            | Ty::AtomicI32
            | Ty::AtomicU32
            | Ty::AtomicI64
            | Ty::AtomicU64
            | Ty::AtomicI128
            | Ty::AtomicU128
            | Ty::AtomicIsize
            | Ty::AtomicUsize
            | Ty::AtomicPtr(_)
            | Ty::Thread
            | Ty::Struct(_)
            | Ty::Enum(_)
            | Ty::Qubit
            | Ty::Result
            | Ty::Unit
            | Ty::Vector(_, _)
            | Ty::AnonVector(_, _)
            | Ty::HeapVector(_)
            | Ty::List(_)
            | Ty::Arc(_)
            | Ty::Mutex(_)
            | Ty::MutexGuard(_)
            | Ty::Option(_)
            | Ty::ResultType(_, _)
            | Ty::Matrix(_, _)
            | Ty::Fn(_, _) => unreachable!("typechecked"),
        }
    }

    fn emit_qir_print_primitive(&mut self, ty: &Ty, v: &str, _newline: bool) {
        match ty {
            Ty::Bool => {
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__bool_record_output(i1 {}, ptr null)",
                    v
                )
                .unwrap();
            }
            Ty::I8 | Ty::I16 | Ty::I32 => {
                let val = self.emit_int_to_i64(ty, v, true);
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__int_record_output(i64 {}, ptr null)",
                    val
                )
                .unwrap();
            }
            Ty::U8 | Ty::U16 | Ty::U32 => {
                let val = self.emit_int_to_i64(ty, v, false);
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__int_record_output(i64 {}, ptr null)",
                    val
                )
                .unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__int_record_output(i64 {}, ptr null)",
                    v
                )
                .unwrap();
            }
            Ty::I128 | Ty::U128 => {
                let t = self.fresh();
                writeln!(self.out, "  %{} = trunc i128 {} to i64", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__int_record_output(i64 %{}, ptr null)",
                    t
                )
                .unwrap();
            }
            Ty::F16 => {
                let t = self.fresh();
                writeln!(self.out, "  %{} = fpext half {} to double", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__double_record_output(double %{}, ptr null)",
                    t
                )
                .unwrap();
            }
            Ty::F32 => {
                let t = self.fresh();
                writeln!(self.out, "  %{} = fpext float {} to double", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__double_record_output(double %{}, ptr null)",
                    t
                )
                .unwrap();
            }
            Ty::F64 => {
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__double_record_output(double {}, ptr null)",
                    v
                )
                .unwrap();
            }
            Ty::String => {
                let qstr = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = call ptr @__quantum__rt__string_create(ptr {})",
                    qstr, v
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__message(ptr %{})",
                    qstr
                )
                .unwrap();
            }
            Ty::Ptr(_) => {
                let addr = self.fresh();
                writeln!(self.out, "  %{} = ptrtoint ptr {} to i64", addr, v).unwrap();
                writeln!(
                    self.out,
                    "  call void @__quantum__rt__int_record_output(i64 %{}, ptr null)",
                    addr
                )
                .unwrap();
            }
            Ty::Array(_, _)
            | Ty::AtomicBool
            | Ty::AtomicI8
            | Ty::AtomicU8
            | Ty::AtomicI16
            | Ty::AtomicU16
            | Ty::AtomicI32
            | Ty::AtomicU32
            | Ty::AtomicI64
            | Ty::AtomicU64
            | Ty::AtomicI128
            | Ty::AtomicU128
            | Ty::AtomicIsize
            | Ty::AtomicUsize
            | Ty::AtomicPtr(_)
            | Ty::Thread
            | Ty::Struct(_)
            | Ty::Enum(_)
            | Ty::Qubit
            | Ty::Result
            | Ty::Unit
            | Ty::Vector(_, _)
            | Ty::AnonVector(_, _)
            | Ty::HeapVector(_)
            | Ty::List(_)
            | Ty::Arc(_)
            | Ty::Mutex(_)
            | Ty::MutexGuard(_)
            | Ty::Option(_)
            | Ty::ResultType(_, _)
            | Ty::Matrix(_, _)
            | Ty::Fn(_, _) => unreachable!("typechecked"),
        }
    }

    fn emit_int_to_i64(&mut self, ty: &Ty, v: &str, signed: bool) -> String {
        let op = if signed { "sext" } else { "zext" };
        let t = self.fresh();
        writeln!(
            self.out,
            "  %{} = {} {} {} to i64",
            t,
            op,
            llvm_ty(ty, self.structs),
            v
        )
        .unwrap();
        format!("%{t}")
    }

    fn emit_qir_gate_1(&mut self, intrinsic: &str, qubit: &str) {
        writeln!(
            self.out,
            "  call void @__quantum__qis__{}__body(ptr {})",
            intrinsic, qubit
        )
        .unwrap();
    }

    fn emit_qir_gate_2(&mut self, intrinsic: &str, left: &str, right: &str) {
        writeln!(
            self.out,
            "  call void @__quantum__qis__{}__body(ptr {}, ptr {})",
            intrinsic, left, right
        )
        .unwrap();
    }

    fn emit_qir_rotation(&mut self, intrinsic: &str, theta: &str, qubit: &str) {
        writeln!(
            self.out,
            "  call void @__quantum__qis__{}__body(double {}, ptr {})",
            intrinsic, theta, qubit
        )
        .unwrap();
    }

    fn emit_qir_rotation_const(&mut self, intrinsic: &str, theta: f64, qubit: &str) {
        self.emit_qir_rotation(intrinsic, &format!("{theta:.17e}"), qubit);
    }

    fn emit_qir_controlled_phase(&mut self, theta: &str, control: &str, target: &str) {
        let half = self.fresh();
        let neg_half = self.fresh();
        writeln!(
            self.out,
            "  %{} = fmul double {}, 5.00000000000000000e-1",
            half, theta
        )
        .unwrap();
        writeln!(self.out, "  %{} = fneg double %{}", neg_half, half).unwrap();
        self.emit_qir_rotation("rz", &format!("%{half}"), target);
        self.emit_qir_gate_2("cnot", control, target);
        self.emit_qir_rotation("rz", &format!("%{neg_half}"), target);
        self.emit_qir_gate_2("cnot", control, target);
        self.emit_qir_rotation("rz", &format!("%{half}"), control);
    }

    fn emit_qir_controlled_axis_rotation(
        &mut self,
        intrinsic: &str,
        theta: &str,
        control: &str,
        target: &str,
    ) {
        let half = self.fresh();
        let neg_half = self.fresh();
        writeln!(
            self.out,
            "  %{} = fmul double {}, 5.00000000000000000e-1",
            half, theta
        )
        .unwrap();
        writeln!(self.out, "  %{} = fneg double %{}", neg_half, half).unwrap();
        self.emit_qir_rotation(intrinsic, &format!("%{half}"), target);
        self.emit_qir_gate_2("cnot", control, target);
        self.emit_qir_rotation(intrinsic, &format!("%{neg_half}"), target);
        self.emit_qir_gate_2("cnot", control, target);
    }

    fn emit_qir_ccnot(&mut self, control_a: &str, control_b: &str, target: &str) {
        self.emit_qir_gate_1("h", target);
        self.emit_qir_gate_2("cnot", control_b, target);
        self.emit_qir_rotation_const("rz", -std::f64::consts::FRAC_PI_4, target);
        self.emit_qir_gate_2("cnot", control_a, target);
        self.emit_qir_gate_1("t", target);
        self.emit_qir_gate_2("cnot", control_b, target);
        self.emit_qir_rotation_const("rz", -std::f64::consts::FRAC_PI_4, target);
        self.emit_qir_gate_2("cnot", control_a, target);
        self.emit_qir_gate_1("t", control_b);
        self.emit_qir_gate_1("t", target);
        self.emit_qir_gate_2("cnot", control_a, control_b);
        self.emit_qir_gate_1("h", target);
        self.emit_qir_gate_1("t", control_a);
        self.emit_qir_rotation_const("rz", -std::f64::consts::FRAC_PI_4, control_b);
        self.emit_qir_gate_2("cnot", control_a, control_b);
    }

    fn emit_qir_builtin_call(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> Option<(Ty, String)> {
        if self.mode != CodegenMode::QirRunner {
            return None;
        }

        if name == QUBIT {
            let q = self.fresh();
            writeln!(
                self.out,
                "  %{} = call ptr @__quantum__rt__qubit_allocate()",
                q
            )
            .unwrap();
            return Some((Ty::Qubit, format!("%{q}")));
        }
        if name == MEASURE {
            let (_, q) = self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
            let result_id = self.qir_next_result;
            self.qir_next_result += 1;
            let r = if result_id == 0 {
                "null".to_string()
            } else {
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = inttoptr i64 {} to ptr", tmp, result_id).unwrap();
                format!("%{tmp}")
            };
            writeln!(
                self.out,
                "  call void @__quantum__qis__mz__body(ptr {}, ptr {})",
                q, r
            )
            .unwrap();
            return Some((Ty::Result, r));
        }
        if name == READ {
            let (_, r) = self.emit_expr(&args[0], locals, Some(&Ty::Result));
            let b = self.fresh();
            writeln!(
                self.out,
                "  %{} = call i1 @__quantum__rt__read_result(ptr {})",
                b, r
            )
            .unwrap();
            return Some((Ty::Bool, format!("%{b}")));
        }
        if name == RECORD {
            let (ty, v) = self.emit_expr(&args[0], locals, None);
            match ty {
                Ty::Result => {
                    writeln!(
                        self.out,
                        "  call void @__quantum__rt__result_record_output(ptr {}, ptr null)",
                        v
                    )
                    .unwrap();
                }
                Ty::Bool => {
                    writeln!(
                        self.out,
                        "  call void @__quantum__rt__bool_record_output(i1 {}, ptr null)",
                        v
                    )
                    .unwrap();
                }
                _ => unreachable!("typechecked q_record argument"),
            }
            return Some((Ty::Unit, String::new()));
        }

        match name {
            GATE_I => {
                self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
                Some((Ty::Unit, String::new()))
            }
            GATE_H | GATE_X | GATE_Y | GATE_Z | GATE_S | GATE_T => {
                let (_, q) = self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
                let intrinsic = match name {
                    GATE_H => "h",
                    GATE_X => "x",
                    GATE_Y => "y",
                    GATE_Z => "z",
                    GATE_S => "s",
                    GATE_T => "t",
                    _ => unreachable!(),
                };
                self.emit_qir_gate_1(intrinsic, &q);
                Some((Ty::Unit, String::new()))
            }
            GATE_SDG | GATE_TDG => {
                let (_, q) = self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
                let theta = if name == GATE_SDG {
                    -std::f64::consts::FRAC_PI_2
                } else {
                    -std::f64::consts::FRAC_PI_4
                };
                self.emit_qir_rotation_const("rz", theta, &q);
                Some((Ty::Unit, String::new()))
            }
            GATE_CNOT | GATE_CZ | GATE_SWAP => {
                let (_, a) = self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
                let (_, b) = self.emit_expr(&args[1], locals, Some(&Ty::Qubit));
                let intrinsic = match name {
                    GATE_CNOT => "cnot",
                    GATE_CZ => "cz",
                    GATE_SWAP => "swap",
                    _ => unreachable!(),
                };
                self.emit_qir_gate_2(intrinsic, &a, &b);
                Some((Ty::Unit, String::new()))
            }
            GATE_RX | GATE_RY | GATE_RZ | GATE_R1 => {
                let (_, theta) = self.emit_expr(&args[0], locals, Some(&Ty::F64));
                let (_, q) = self.emit_expr(&args[1], locals, Some(&Ty::Qubit));
                let intrinsic = match name {
                    GATE_RX => "rx",
                    GATE_RY => "ry",
                    GATE_RZ | GATE_R1 => "rz",
                    _ => unreachable!(),
                };
                self.emit_qir_rotation(intrinsic, &theta, &q);
                Some((Ty::Unit, String::new()))
            }
            GATE_CH | GATE_CY | GATE_CS | GATE_CSDG | GATE_CT | GATE_CTDG => {
                let (_, c) = self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
                let (_, t) = self.emit_expr(&args[1], locals, Some(&Ty::Qubit));
                match name {
                    GATE_CH => {
                        self.emit_qir_rotation_const("ry", -std::f64::consts::FRAC_PI_4, &t);
                        self.emit_qir_gate_2("cz", &c, &t);
                        self.emit_qir_rotation_const("ry", std::f64::consts::FRAC_PI_4, &t);
                    }
                    GATE_CY => {
                        self.emit_qir_rotation_const("rz", -std::f64::consts::FRAC_PI_2, &t);
                        self.emit_qir_gate_2("cnot", &c, &t);
                        self.emit_qir_gate_1("s", &t);
                    }
                    GATE_CS => self.emit_qir_controlled_phase(
                        &format!("{:.17e}", std::f64::consts::FRAC_PI_2),
                        &c,
                        &t,
                    ),
                    GATE_CSDG => self.emit_qir_controlled_phase(
                        &format!("{:.17e}", -std::f64::consts::FRAC_PI_2),
                        &c,
                        &t,
                    ),
                    GATE_CT => self.emit_qir_controlled_phase(
                        &format!("{:.17e}", std::f64::consts::FRAC_PI_4),
                        &c,
                        &t,
                    ),
                    GATE_CTDG => self.emit_qir_controlled_phase(
                        &format!("{:.17e}", -std::f64::consts::FRAC_PI_4),
                        &c,
                        &t,
                    ),
                    _ => unreachable!(),
                }
                Some((Ty::Unit, String::new()))
            }
            GATE_CRX | GATE_CRY | GATE_CRZ | GATE_CR1 => {
                let (_, theta) = self.emit_expr(&args[0], locals, Some(&Ty::F64));
                let (_, c) = self.emit_expr(&args[1], locals, Some(&Ty::Qubit));
                let (_, t) = self.emit_expr(&args[2], locals, Some(&Ty::Qubit));
                match name {
                    GATE_CRX => {
                        self.emit_qir_gate_1("h", &t);
                        self.emit_qir_controlled_axis_rotation("rz", &theta, &c, &t);
                        self.emit_qir_gate_1("h", &t);
                    }
                    GATE_CRY => self.emit_qir_controlled_axis_rotation("ry", &theta, &c, &t),
                    GATE_CRZ => self.emit_qir_controlled_axis_rotation("rz", &theta, &c, &t),
                    GATE_CR1 => self.emit_qir_controlled_phase(&theta, &c, &t),
                    _ => unreachable!(),
                }
                Some((Ty::Unit, String::new()))
            }
            GATE_CCNOT | GATE_CCZ | GATE_CSWAP => {
                let (_, a) = self.emit_expr(&args[0], locals, Some(&Ty::Qubit));
                let (_, b) = self.emit_expr(&args[1], locals, Some(&Ty::Qubit));
                let (_, c) = self.emit_expr(&args[2], locals, Some(&Ty::Qubit));
                match name {
                    GATE_CCNOT => self.emit_qir_ccnot(&a, &b, &c),
                    GATE_CCZ => {
                        self.emit_qir_gate_1("h", &c);
                        self.emit_qir_ccnot(&a, &b, &c);
                        self.emit_qir_gate_1("h", &c);
                    }
                    GATE_CSWAP => {
                        self.emit_qir_gate_2("cnot", &c, &b);
                        self.emit_qir_ccnot(&a, &b, &c);
                        self.emit_qir_gate_2("cnot", &c, &b);
                    }
                    _ => unreachable!(),
                }
                Some((Ty::Unit, String::new()))
            }
            _ => None,
        }
    }

    /// Emits array printing in `[a, b, c]` style with optional trailing newline.
    ///
    /// Elements are extracted using `extractvalue` and recursively rendered
    /// via `emit_print_value`, so nested printable composites are supported.
    fn emit_print_array(&mut self, elem_ty: &Ty, n: usize, arr_v: &str, newline: bool) {
        self.emit_printf_text("nialang.std.txt.arr_open", 2);
        for i in 0..n {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let ev = self.fresh();
            let llvm_arr = format!("[{} x {}]", n, llvm_ty(elem_ty, self.structs));
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ev, llvm_arr, arr_v, i
            )
            .unwrap();
            self.emit_print_value(elem_ty, &format!("%{ev}"), false);
        }
        if newline {
            self.emit_printf_text("nialang.std.txt.arr_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.arr_close", 2);
        }
    }

    fn emit_print_anon_vector(&mut self, elem_ty: &Ty, n: usize, vec_v: &str, newline: bool) {
        let Some((open_sym, open_size)) = anon_vector_print_open_symbol(elem_ty) else {
            unreachable!("typechecked anonymous vector print element type")
        };
        self.emit_printf_text(open_sym, open_size);
        for i in 0..n {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let ev = self.fresh();
            let llvm_vec = format!("[{} x {}]", n, llvm_ty(elem_ty, self.structs));
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ev, llvm_vec, vec_v, i
            )
            .unwrap();
            self.emit_print_value(elem_ty, &format!("%{ev}"), false);
        }
        if newline {
            self.emit_printf_text("nialang.std.txt.anonvec.close_ln", 4);
        } else {
            self.emit_printf_text("nialang.std.txt.anonvec.close", 3);
        }
    }

    /// Emits struct printing in two source-level forms:
    /// - named structs -> JSON-like object form (`{"x": 1, "y": 2}`)
    /// - tuple structs -> tuple form (`(1, 2)`)
    ///
    /// Field values are pulled from aggregate SSA value via `extractvalue` and then
    /// recursively rendered according to each field type.
    fn emit_print_struct(&mut self, sname: &str, struct_v: &str, newline: bool) {
        let sdef = self.struct_def(sname).expect("typechecked struct");
        let is_tuple = sdef.is_tuple;
        let fields = sdef.fields.clone();
        if is_tuple {
            self.emit_printf_text("nialang.std.txt.tuple_open", 2);
            for (i, (_, fty)) in fields.iter().enumerate() {
                if i > 0 {
                    self.emit_printf_text("nialang.std.txt.arr_sep", 3);
                }
                let fv = self.fresh();
                let llvm_st = format!("%struct.{}", sanitize(sname));
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    fv, llvm_st, struct_v, i
                )
                .unwrap();
                self.emit_print_value(fty, &format!("%{fv}"), false);
            }
            if newline {
                self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
            } else {
                self.emit_printf_text("nialang.std.txt.tuple_close", 2);
            }
            return;
        }

        self.emit_printf_text("nialang.std.txt.obj_open", 2);
        for (i, (fname, fty)) in fields.iter().enumerate() {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let key_sym = struct_field_key_symbol(sname, fname);
            let key_sz = fname.as_bytes().len() + 5;
            self.emit_printf_text_dynamic(&key_sym, key_sz);
            let fv = self.fresh();
            let llvm_st = format!("%struct.{}", sanitize(sname));
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                fv, llvm_st, struct_v, i
            )
            .unwrap();
            self.emit_print_value(fty, &format!("%{fv}"), false);
        }
        if newline {
            self.emit_printf_text("nialang.std.txt.obj_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.obj_close", 2);
        }
    }

    /// Vector values use `%struct.<name>` aggregates; print as `(Name {"X": …})`.
    fn emit_print_vector(&mut self, vname: &str, vec_v: &str, newline: bool) {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector");
        let fty = vdef.ty.clone();
        let llvm_st = format!("%struct.{}", sanitize(vname));
        self.emit_printf_text("nialang.std.txt.tuple_open", 2);
        let ty_sym = vector_print_ty_prefix_symbol(vname);
        let ty_sz = format!("{} ", ty_print_label(&vdef.ty)).as_bytes().len() + 1;
        self.emit_printf_text_dynamic(&ty_sym, ty_sz);
        self.emit_printf_text("nialang.std.txt.obj_open", 2);
        for (i, fname) in vdef.fields.iter().enumerate() {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let key_sym = struct_field_key_symbol(vname, fname);
            let key_sz = fname.as_bytes().len() + 5;
            self.emit_printf_text_dynamic(&key_sym, key_sz);
            let fv = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                fv, llvm_st, vec_v, i
            )
            .unwrap();
            self.emit_print_value(&fty, &format!("%{fv}"), false);
        }
        self.emit_printf_text("nialang.std.txt.obj_close", 2);
        if newline {
            self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.tuple_close", 2);
        }
    }

    /// Prints enum as `Variant: <payload>` (unit: `Variant: ()` with newline when requested).
    fn emit_print_enum(&mut self, ename: &str, ev: &str, newline: bool) {
        let edef = self
            .enums
            .iter()
            .find(|e| e.name == ename)
            .expect("typechecked");
        let enum_ll = format!("%enum.{}", sanitize(ename));
        let tag = self.fresh();
        writeln!(self.out, "  %{} = extractvalue {} {}, 0", tag, enum_ll, ev).unwrap();
        let default_lbl = self.fresh_label("println.enum.default");
        let cont_lbl = self.fresh_label("println.enum.cont");
        let arm_lbls: Vec<String> = edef
            .variants
            .iter()
            .map(|_| self.fresh_label("println.enum.arm"))
            .collect();
        writeln!(self.out, "  switch i32 %{}, label %{} [", tag, default_lbl).unwrap();
        for (i, _) in edef.variants.iter().enumerate() {
            writeln!(self.out, "    i32 {}, label %{}", i as i32, arm_lbls[i]).unwrap();
        }
        writeln!(self.out, "  ]").unwrap();
        writeln!(self.out, "{}:", default_lbl).unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        for (i, vdef) in edef.variants.iter().enumerate() {
            writeln!(self.out, "{}:", arm_lbls[i]).unwrap();
            let sym = enum_variant_prefix_symbol(ename, &vdef.name);
            let prefix = format!("{}: ", vdef.name);
            let prefix_len = prefix.as_bytes().len() + 1;
            self.emit_printf_text_dynamic(&sym, prefix_len);
            let payload_idx = i + 1;
            match &vdef.fields {
                EnumVariantFields::Unit => {
                    self.emit_printf_text("nialang.std.txt.tuple_open", 2);
                    if newline {
                        self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
                    } else {
                        self.emit_printf_text("nialang.std.txt.tuple_close", 2);
                    }
                }
                EnumVariantFields::Tuple(ts) if ts.len() == 1 => {
                    let pl = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        pl, enum_ll, ev, payload_idx
                    )
                    .unwrap();
                    self.emit_print_value(&ts[0], &format!("%{pl}"), newline);
                }
                EnumVariantFields::Tuple(ts) => {
                    let pl = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        pl, enum_ll, ev, payload_idx
                    )
                    .unwrap();
                    let payload_ll = {
                        let inner = ts
                            .iter()
                            .map(|t| llvm_ty(t, self.structs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ {inner} }}")
                    };
                    self.emit_printf_text("nialang.std.txt.tuple_open", 2);
                    for j in 0..ts.len() {
                        if j > 0 {
                            self.emit_printf_text("nialang.std.txt.arr_sep", 3);
                        }
                        let fv = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = extractvalue {} %{}, {}",
                            fv, payload_ll, pl, j
                        )
                        .unwrap();
                        self.emit_print_value(&ts[j], &format!("%{fv}"), false);
                    }
                    if newline {
                        self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
                    } else {
                        self.emit_printf_text("nialang.std.txt.tuple_close", 2);
                    }
                }
                EnumVariantFields::Struct(fs) => {
                    let pl = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        pl, enum_ll, ev, payload_idx
                    )
                    .unwrap();
                    let payload_ll = {
                        let inner = fs
                            .iter()
                            .map(|(_, t)| llvm_ty(t, self.structs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ {inner} }}")
                    };
                    self.emit_printf_text("nialang.std.txt.obj_open", 2);
                    for (j, (fname, fty)) in fs.iter().enumerate() {
                        if j > 0 {
                            self.emit_printf_text("nialang.std.txt.arr_sep", 3);
                        }
                        let ksym = enum_struct_field_key_symbol(ename, &vdef.name, fname);
                        let key = format!("\"{}\": ", fname);
                        let key_sz = key.as_bytes().len() + 1;
                        self.emit_printf_text_dynamic(&ksym, key_sz);
                        let fv = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = extractvalue {} %{}, {}",
                            fv, payload_ll, pl, j
                        )
                        .unwrap();
                        self.emit_print_value(fty, &format!("%{fv}"), false);
                    }
                    if newline {
                        self.emit_printf_text("nialang.std.txt.obj_close_ln", 3);
                    } else {
                        self.emit_printf_text("nialang.std.txt.obj_close", 2);
                    }
                }
            }
            writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        }
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn emit_print_matrix_summary(&mut self, matrix: &str, newline: bool) {
        let rows = self.matrix_load_i64_field(matrix, 1);
        let cols = self.matrix_load_i64_field(matrix, 2);
        let (sym, size) = if newline {
            ("nialang.std.fmt.matrix", 30)
        } else {
            ("nialang.std.fmt.matrix.nn", 29)
        };
        let p = self.fmt_ptr(sym, size);
        writeln!(
            self.out,
            "  call i32 (ptr, ...) @printf(ptr {}, i64 {}, i64 {})",
            p, rows, cols
        )
        .unwrap();
    }

    fn emit_print_matrix(&mut self, elem_ty: &Ty, matrix: &str, newline: bool) {
        if matches!(elem_ty, Ty::Unit) {
            self.emit_print_matrix_summary(matrix, newline);
            return;
        }

        let rows = self.matrix_load_i64_field(matrix, 1);
        let cols = self.matrix_load_i64_field(matrix, 2);
        let data = self.matrix_load_data_ptr(matrix);
        let row_addr = self.fresh();
        let col_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", row_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", col_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", row_addr).unwrap();

        let row_cond = self.fresh_label("println.matrix.row.cond");
        let row_body = self.fresh_label("println.matrix.row.body");
        let row_sep = self.fresh_label("println.matrix.row.sep");
        let row_item = self.fresh_label("println.matrix.row.item");
        let row_latch = self.fresh_label("println.matrix.row.latch");
        let row_done = self.fresh_label("println.matrix.row.done");
        let col_cond = self.fresh_label("println.matrix.col.cond");
        let col_body = self.fresh_label("println.matrix.col.body");
        let col_sep = self.fresh_label("println.matrix.col.sep");
        let col_item = self.fresh_label("println.matrix.col.item");
        let col_latch = self.fresh_label("println.matrix.col.latch");
        let col_done = self.fresh_label("println.matrix.col.done");

        self.emit_printf_text("nialang.std.txt.arr_open", 2);
        writeln!(self.out, "  br label %{}", row_cond).unwrap();

        writeln!(self.out, "{}:", row_cond).unwrap();
        let row = self.fresh();
        let has_row = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", row, row_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_row, row, rows).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_row, row_body, row_done
        )
        .unwrap();

        writeln!(self.out, "{}:", row_body).unwrap();
        let first_row = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 0", first_row, row).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            first_row, row_item, row_sep
        )
        .unwrap();

        writeln!(self.out, "{}:", row_sep).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_sep", 3);
        writeln!(self.out, "  br label %{}", row_item).unwrap();

        writeln!(self.out, "{}:", row_item).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_open", 2);
        writeln!(self.out, "  store i64 0, ptr %{}", col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond).unwrap();

        writeln!(self.out, "{}:", col_cond).unwrap();
        let col = self.fresh();
        let has_col = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", col, col_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_col, col, cols).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_col, col_body, col_done
        )
        .unwrap();

        writeln!(self.out, "{}:", col_body).unwrap();
        let first_col = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 0", first_col, col).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            first_col, col_item, col_sep
        )
        .unwrap();

        writeln!(self.out, "{}:", col_sep).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_sep", 3);
        writeln!(self.out, "  br label %{}", col_item).unwrap();

        writeln!(self.out, "{}:", col_item).unwrap();
        let row_offset = self.fresh();
        let index = self.fresh();
        let cell_ptr = self.fresh();
        let cell = self.fresh();
        writeln!(self.out, "  %{} = mul i64 %{}, {}", row_offset, row, cols).unwrap();
        writeln!(self.out, "  %{} = add i64 %{}, %{}", index, row_offset, col).unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell_ptr,
            llvm_ty(elem_ty, self.structs),
            data,
            index
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            cell_ptr
        )
        .unwrap();
        self.emit_print_value(elem_ty, &format!("%{cell}"), false);
        writeln!(self.out, "  br label %{}", col_latch).unwrap();

        writeln!(self.out, "{}:", col_latch).unwrap();
        let col_next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", col_next, col).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", col_next, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond).unwrap();

        writeln!(self.out, "{}:", col_done).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_close", 2);
        writeln!(self.out, "  br label %{}", row_latch).unwrap();

        writeln!(self.out, "{}:", row_latch).unwrap();
        let row_next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", row_next, row).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", row_next, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond).unwrap();

        writeln!(self.out, "{}:", row_done).unwrap();
        if newline {
            self.emit_printf_text("nialang.std.txt.arr_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.arr_close", 2);
        }
    }

    /// Recursive dispatcher for printable value lowering.
    ///
    /// Keeps all `println` formatting behavior in one place and ensures nested
    /// composites (array/struct of array/struct/primitive/pointer) print consistently.
    fn emit_print_value(&mut self, ty: &Ty, v: &str, newline: bool) {
        match ty {
            Ty::Array(elem, n) => {
                self.emit_print_array(elem, *n, v, newline);
            }
            Ty::Struct(sname) => {
                if self.struct_def(sname).is_some() {
                    self.emit_print_struct(sname, v, newline);
                } else {
                    self.emit_print_vector(sname, v, newline);
                }
            }
            Ty::Vector(vname, _) => {
                self.emit_print_vector(vname, v, newline);
            }
            Ty::AnonVector(elem_ty, n) => {
                self.emit_print_anon_vector(elem_ty, *n, v, newline);
            }
            Ty::HeapVector(elem_ty) => {
                self.emit_print_heap_vector(elem_ty, v, newline);
            }
            Ty::Enum(ename) => {
                self.emit_print_enum(ename, v, newline);
            }
            Ty::Matrix(elem_ty, _) => {
                self.emit_print_matrix(elem_ty, v, newline);
            }
            _ => self.emit_print_primitive(ty, v, newline),
        }
    }

    fn emit_print_heap_vector(&mut self, elem_ty: &Ty, vector: &str, newline: bool) {
        let Some((open_sym, open_size)) = anon_vector_print_open_symbol(elem_ty) else {
            unreachable!("typechecked heap anonymous vector print element type")
        };
        let len = self.heap_vector_load_i64_field(vector, 1);
        let data = self.heap_vector_load_data_ptr(vector);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label("println.heap.vector.cond");
        let body_lbl = self.fresh_label("println.heap.vector.body");
        let sep_lbl = self.fresh_label("println.heap.vector.sep");
        let item_lbl = self.fresh_label("println.heap.vector.item");
        let latch_lbl = self.fresh_label("println.heap.vector.latch");
        let done_lbl = self.fresh_label("println.heap.vector.done");

        self.emit_printf_text(open_sym, open_size);
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_item, idx, len).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let first = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 0", first, idx).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            first, item_lbl, sep_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", sep_lbl).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_sep", 3);
        writeln!(self.out, "  br label %{}", item_lbl).unwrap();

        writeln!(self.out, "{}:", item_lbl).unwrap();
        let cell_ptr = self.heap_vector_data_cell_ptr(&data, &format!("%{idx}"), elem_ty);
        let cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            cell,
            llvm_ty(elem_ty, self.structs),
            cell_ptr
        )
        .unwrap();
        self.emit_print_value(elem_ty, &format!("%{cell}"), false);
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        if newline {
            self.emit_printf_text("nialang.std.txt.anonvec.close_ln", 4);
        } else {
            self.emit_printf_text("nialang.std.txt.anonvec.close", 3);
        }
    }

    fn emit_matrix_new(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (src_ty, src_val) = self.emit_expr(&args[0], locals, None);
        self.emit_matrix_from_array(src_ty, &src_val)
    }

    fn emit_matrix_from_array(&mut self, src_ty: Ty, src_val: &str) -> (Ty, String) {
        let Ty::Array(row_ty, rows) = src_ty else {
            unreachable!("typechecked matrix source")
        };
        let Ty::Array(cell_ty, cols) = row_ty.as_ref() else {
            unreachable!("typechecked matrix source")
        };
        let len = rows * cols;
        let bytes = len * matrix_elem_size(cell_ty);
        let data = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        let matrix = self.emit_matrix_header_alloc(
            &format!("%{data}"),
            &rows.to_string(),
            &cols.to_string(),
        );

        let src_ll = llvm_ty(
            &Ty::Array(Box::new(Ty::Array(cell_ty.clone(), *cols)), rows),
            self.structs,
        );
        let row_ll = llvm_ty(&Ty::Array(cell_ty.clone(), *cols), self.structs);
        for row in 0..rows {
            let row_val = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                row_val, src_ll, src_val, row
            )
            .unwrap();
            for col in 0..*cols {
                let raw_cell = self.fresh();
                let flat = row * *cols + col;
                let cell_ptr = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} %{}, {}",
                    raw_cell, row_ll, row_val, col
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr %{}, i64 {}",
                    cell_ptr,
                    llvm_ty(cell_ty, self.structs),
                    data,
                    flat
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store {} %{}, ptr %{}",
                    llvm_ty(cell_ty, self.structs),
                    raw_cell,
                    cell_ptr
                )
                .unwrap();
            }
        }

        (Ty::Matrix(cell_ty.clone(), Some((rows, *cols))), matrix)
    }

    fn emit_matrix_to_array(
        &mut self,
        elem_ty: &Ty,
        rows: usize,
        cols: usize,
        matrix: &str,
    ) -> (Ty, String) {
        let data = self.matrix_load_data_ptr(matrix);
        let row_ty = Ty::Array(Box::new(elem_ty.clone()), cols);
        let arr_ty = Ty::Array(Box::new(row_ty.clone()), rows);
        let row_ll = llvm_ty(&row_ty, self.structs);
        let arr_ll = llvm_ty(&arr_ty, self.structs);
        let mut arr_agg = "poison".to_string();

        for row in 0..rows {
            let mut row_agg = "poison".to_string();
            for col in 0..cols {
                let flat = row * cols + col;
                let cell_ptr = self.fresh();
                let cell = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr {}, i64 {}",
                    cell_ptr,
                    llvm_ty(elem_ty, self.structs),
                    data,
                    flat
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr %{}",
                    cell,
                    llvm_ty(elem_ty, self.structs),
                    cell_ptr
                )
                .unwrap();
                let next_row = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} {}, {} %{}, {}",
                    next_row,
                    row_ll,
                    row_agg,
                    llvm_ty(elem_ty, self.structs),
                    cell,
                    col
                )
                .unwrap();
                row_agg = format!("%{next_row}");
            }
            let next_arr = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next_arr, arr_ll, arr_agg, row_ll, row_agg, row
            )
            .unwrap();
            arr_agg = format!("%{next_arr}");
        }

        (arr_ty, arr_agg)
    }

    fn emit_matrix_clone(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (matrix_ty, matrix) = self.emit_expr(&args[0], locals, None);
        let cloned = self.emit_matrix_clone_value(&matrix_ty, &matrix);
        (matrix_ty, cloned)
    }

    fn emit_matrix_drop(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (_, matrix) = self.emit_expr(&args[0], locals, None);
        self.emit_matrix_drop_value(&matrix);
        (Ty::Unit, String::new())
    }

    fn emit_matrix_get(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (matrix_ty, matrix) = self.emit_expr(&args[0], locals, None);
        let Ty::Matrix(elem_ty, _) = matrix_ty else {
            unreachable!("typechecked matrix_get")
        };
        let (_, row) = self.emit_expr(&args[1], locals, Some(&Ty::I32));
        let (_, col) = self.emit_expr(&args[2], locals, Some(&Ty::I32));
        let cell = self.matrix_cell_ptr(&matrix, &row, &col, &elem_ty);
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            out,
            llvm_ty(&elem_ty, self.structs),
            cell
        )
        .unwrap();
        ((*elem_ty).clone(), format!("%{out}"))
    }

    fn emit_matrix_set(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (matrix_ty, matrix) = self.emit_expr(&args[0], locals, None);
        let Ty::Matrix(elem_ty, _) = matrix_ty else {
            unreachable!("typechecked matrix_set")
        };
        let (_, row) = self.emit_expr(&args[1], locals, Some(&Ty::I32));
        let (_, col) = self.emit_expr(&args[2], locals, Some(&Ty::I32));
        let (value_ty, value) = self.emit_expr(&args[3], locals, Some(&elem_ty));
        debug_assert!(types_match(&value_ty, &elem_ty));
        let cell = self.matrix_cell_ptr(&matrix, &row, &col, &elem_ty);
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(&elem_ty, self.structs),
            value,
            cell
        )
        .unwrap();
        (Ty::Unit, String::new())
    }

    fn emit_matrix_info(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (_, matrix) = self.emit_expr(&args[0], locals, None);
        let value64 = if name == MATRIX_ROWS {
            self.matrix_load_i64_field(&matrix, 1)
        } else if name == MATRIX_COLS {
            self.matrix_load_i64_field(&matrix, 2)
        } else {
            let rows = self.matrix_load_i64_field(&matrix, 1);
            let cols = self.matrix_load_i64_field(&matrix, 2);
            let len = self.fresh();
            writeln!(self.out, "  %{} = mul i64 {}, {}", len, rows, cols).unwrap();
            format!("%{len}")
        };
        let out = self.fresh();
        writeln!(self.out, "  %{} = trunc i64 {} to i32", out, value64).unwrap();
        (Ty::I32, format!("%{out}"))
    }

    fn emit_heap_vector_clone(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (vector_ty, vector) = self.emit_expr(&args[0], locals, None);
        let cloned = self.emit_heap_vector_clone_value(&vector_ty, &vector);
        (vector_ty, cloned)
    }

    fn emit_heap_vector_drop(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (vector_ty, vector) = self.emit_expr(&args[0], locals, None);
        let Ty::HeapVector(elem_ty) = vector_ty else {
            unreachable!("typechecked vector_drop")
        };
        self.emit_heap_vector_drop_value(&elem_ty, &vector);
        (Ty::Unit, String::new())
    }

    fn emit_heap_vector_get(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (vector_ty, vector) = self.emit_expr(&args[0], locals, None);
        let Ty::HeapVector(elem_ty) = vector_ty else {
            unreachable!("typechecked vector_get")
        };
        let (_, idx) = self.emit_expr(&args[1], locals, Some(&Ty::I32));
        let cell = self.heap_vector_cell_ptr(&vector, &idx, &elem_ty);
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            out,
            llvm_ty(&elem_ty, self.structs),
            cell
        )
        .unwrap();
        ((*elem_ty).clone(), format!("%{out}"))
    }

    fn emit_heap_vector_set(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (vector_ty, vector) = self.emit_expr(&args[0], locals, None);
        let Ty::HeapVector(elem_ty) = vector_ty else {
            unreachable!("typechecked vector_set")
        };
        let (_, idx) = self.emit_expr(&args[1], locals, Some(&Ty::I32));
        let (value_ty, value) = self.emit_expr(&args[2], locals, Some(&elem_ty));
        debug_assert!(types_match(&value_ty, &elem_ty));
        let cell = self.heap_vector_cell_ptr(&vector, &idx, &elem_ty);
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(&elem_ty, self.structs),
            value,
            cell
        )
        .unwrap();
        (Ty::Unit, String::new())
    }

    fn emit_heap_vector_info(
        &mut self,
        _name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (_, vector) = self.emit_expr(&args[0], locals, None);
        let value64 = self.heap_vector_load_i64_field(&vector, 1);
        let out = self.fresh();
        writeln!(self.out, "  %{} = trunc i64 {} to i32", out, value64).unwrap();
        (Ty::I32, format!("%{out}"))
    }

    fn emit_matrix_elem_binop(
        &mut self,
        elem_ty: &Ty,
        left: &str,
        right: &str,
        op: &str,
    ) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                let llvm_op = matrix_int_binop_instruction(op);
                writeln!(self.out, "  %{} = {} i8 {}, {}", out, llvm_op, left, right).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                let llvm_op = matrix_int_binop_instruction(op);
                writeln!(self.out, "  %{} = {} i16 {}, {}", out, llvm_op, left, right).unwrap();
            }
            Ty::I32 => {
                if op == "/" {
                    let llvm_op = matrix_int_div_instruction(elem_ty);
                    writeln!(self.out, "  %{} = {} i32 {}, {}", out, llvm_op, left, right).unwrap();
                } else {
                    let llvm_op = matrix_int_binop_instruction(op);
                    writeln!(
                        self.out,
                        "  %{} = {} nsw i32 {}, {}",
                        out, llvm_op, left, right
                    )
                    .unwrap();
                }
            }
            Ty::U32 => {
                let llvm_op = if op == "/" {
                    matrix_int_div_instruction(elem_ty)
                } else {
                    matrix_int_binop_instruction(op)
                };
                writeln!(self.out, "  %{} = {} i32 {}, {}", out, llvm_op, left, right).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                if op == "/" {
                    let llvm_op = matrix_int_div_instruction(elem_ty);
                    writeln!(self.out, "  %{} = {} i64 {}, {}", out, llvm_op, left, right).unwrap();
                } else {
                    let llvm_op = matrix_int_binop_instruction(op);
                    writeln!(self.out, "  %{} = {} i64 {}, {}", out, llvm_op, left, right).unwrap();
                }
            }
            Ty::I128 | Ty::U128 => {
                let llvm_op = matrix_int_binop_instruction(op);
                writeln!(
                    self.out,
                    "  %{} = {} i128 {}, {}",
                    out, llvm_op, left, right
                )
                .unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                let llvm_op = matrix_float_binop_instruction(op);
                writeln!(
                    self.out,
                    "  %{} = {} {} {}, {}",
                    out, llvm_op, ll, left, right
                )
                .unwrap();
            }
            _ => unreachable!("typechecked numeric matrix element"),
        }
        format!("%{out}")
    }

    fn emit_matrix_elem_div(&mut self, elem_ty: &Ty, left: &str, right: &str) -> String {
        self.emit_matrix_elem_binop(elem_ty, left, right, "/")
    }

    fn emit_matrix_elem_neg(&mut self, elem_ty: &Ty, value: &str) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                writeln!(self.out, "  %{} = sub i8 0, {}", out, value).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                writeln!(self.out, "  %{} = sub i16 0, {}", out, value).unwrap();
            }
            Ty::I32 => {
                writeln!(self.out, "  %{} = sub nsw i32 0, {}", out, value).unwrap();
            }
            Ty::U32 => {
                writeln!(self.out, "  %{} = sub i32 0, {}", out, value).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(self.out, "  %{} = sub i64 0, {}", out, value).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                writeln!(self.out, "  %{} = sub i128 0, {}", out, value).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(self.out, "  %{} = fneg {} {}", out, ll, value).unwrap();
            }
            _ => unreachable!("typechecked numeric matrix element"),
        }
        format!("%{out}")
    }

    /// Returns an `i1` SSA value: true when `value` is numerically zero.
    fn emit_matrix_elem_is_zero(&mut self, elem_ty: &Ty, value: &str) -> String {
        let out = self.fresh();
        if is_float_ty(elem_ty) {
            let ll = llvm_ty(elem_ty, self.structs);
            writeln!(
                self.out,
                "  %{} = fcmp oeq {} {}, {}",
                out,
                ll,
                value,
                matrix_zero_value(elem_ty)
            )
            .unwrap();
        } else {
            let ll = llvm_ty(elem_ty, self.structs);
            writeln!(
                self.out,
                "  %{} = icmp eq {} {}, {}",
                out,
                ll,
                value,
                matrix_zero_value(elem_ty)
            )
            .unwrap();
        }
        format!("%{out}")
    }

    fn emit_matrix_binop(
        &mut self,
        left: &str,
        right: &str,
        elem_ty: &Ty,
        shape: Option<(usize, usize)>,
        op: &str,
    ) -> (Ty, String) {
        let label_op = matrix_binop_label(op);
        let left_rows = self.matrix_load_i64_field(left, 1);
        let left_cols = self.matrix_load_i64_field(left, 2);
        let right_rows = self.matrix_load_i64_field(right, 1);
        let right_cols = self.matrix_load_i64_field(right, 2);
        let rows_match = self.fresh();
        let cols_match = self.fresh();
        let shape_match = self.fresh();
        let ok_lbl = self.fresh_label(&format!("matrix.{label_op}.shape.ok"));
        let abort_lbl = self.fresh_label(&format!("matrix.{label_op}.shape.abort"));
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            rows_match, left_rows, right_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            cols_match, left_cols, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = and i1 %{}, %{}",
            shape_match, rows_match, cols_match
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            shape_match, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        let len = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 {}, {}",
            len, left_rows, left_cols
        )
        .unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        let matrix = self.emit_matrix_header_alloc(&format!("%{data}"), &left_rows, &left_cols);

        let left_data = self.matrix_load_data_ptr(left);
        let right_data = self.matrix_load_data_ptr(right);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label(&format!("matrix.{label_op}.cond"));
        let body_lbl = self.fresh_label(&format!("matrix.{label_op}.body"));
        let latch_lbl = self.fresh_label(&format!("matrix.{label_op}.latch"));
        let done_lbl = self.fresh_label(&format!("matrix.{label_op}.done"));
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            has_item, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let ll = llvm_ty(elem_ty, self.structs);
        let left_cell_ptr = self.fresh();
        let right_cell_ptr = self.fresh();
        let out_cell_ptr = self.fresh();
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            left_cell_ptr, ll, left_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            right_cell_ptr, ll, right_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell_ptr, ll, data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            left_cell, ll, left_cell_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            right_cell, ll, right_cell_ptr
        )
        .unwrap();
        let value = self.emit_matrix_elem_binop(
            elem_ty,
            &format!("%{left_cell}"),
            &format!("%{right_cell}"),
            op,
        );
        writeln!(self.out, "  store {} {}, ptr %{}", ll, value, out_cell_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (Ty::Matrix(Box::new(elem_ty.clone()), shape), matrix)
    }

    fn emit_matrix_scalar_mul(
        &mut self,
        matrix: &str,
        scalar: &str,
        elem_ty: &Ty,
        shape: Option<(usize, usize)>,
    ) -> (Ty, String) {
        let rows = self.matrix_load_i64_field(matrix, 1);
        let cols = self.matrix_load_i64_field(matrix, 2);
        let len = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", len, rows, cols).unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        let out_matrix = self.emit_matrix_header_alloc(&format!("%{data}"), &rows, &cols);

        let matrix_data = self.matrix_load_data_ptr(matrix);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label("matrix.scalar.mul.cond");
        let body_lbl = self.fresh_label("matrix.scalar.mul.body");
        let latch_lbl = self.fresh_label("matrix.scalar.mul.latch");
        let done_lbl = self.fresh_label("matrix.scalar.mul.done");
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            has_item, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let ll = llvm_ty(elem_ty, self.structs);
        let cell_ptr = self.fresh();
        let out_cell_ptr = self.fresh();
        let cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell_ptr, ll, matrix_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell_ptr, ll, data, idx
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", cell, ll, cell_ptr).unwrap();
        let value = self.emit_matrix_elem_binop(elem_ty, &format!("%{cell}"), scalar, "*");
        writeln!(self.out, "  store {} {}, ptr %{}", ll, value, out_cell_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (Ty::Matrix(Box::new(elem_ty.clone()), shape), out_matrix)
    }

    fn emit_matrix_matmul(
        &mut self,
        left: &str,
        right: &str,
        elem_ty: &Ty,
        shape: Option<(usize, usize)>,
    ) -> (Ty, String) {
        let left_rows = self.matrix_load_i64_field(left, 1);
        let left_cols = self.matrix_load_i64_field(left, 2);
        let right_rows = self.matrix_load_i64_field(right, 1);
        let right_cols = self.matrix_load_i64_field(right, 2);
        let shape_match = self.fresh();
        let ok_lbl = self.fresh_label("matrix.matmul.shape.ok");
        let abort_lbl = self.fresh_label("matrix.matmul.shape.abort");
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            shape_match, left_cols, right_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            shape_match, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        let len = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 {}, {}",
            len, left_rows, right_cols
        )
        .unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        let out_matrix =
            self.emit_matrix_header_alloc(&format!("%{data}"), &left_rows, &right_cols);

        let ll = llvm_ty(elem_ty, self.structs);
        let left_data = self.matrix_load_data_ptr(left);
        let right_data = self.matrix_load_data_ptr(right);
        let row_addr = self.fresh();
        let col_addr = self.fresh();
        let inner_addr = self.fresh();
        let acc_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", row_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", col_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", inner_addr).unwrap();
        writeln!(self.out, "  %{} = alloca {}", acc_addr, ll).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", row_addr).unwrap();

        let row_cond_lbl = self.fresh_label("matrix.matmul.row.cond");
        let row_body_lbl = self.fresh_label("matrix.matmul.row.body");
        let row_latch_lbl = self.fresh_label("matrix.matmul.row.latch");
        let done_lbl = self.fresh_label("matrix.matmul.done");
        let col_cond_lbl = self.fresh_label("matrix.matmul.col.cond");
        let col_body_lbl = self.fresh_label("matrix.matmul.col.body");
        let col_latch_lbl = self.fresh_label("matrix.matmul.col.latch");
        let col_done_lbl = self.fresh_label("matrix.matmul.col.done");
        let inner_cond_lbl = self.fresh_label("matrix.matmul.inner.cond");
        let inner_body_lbl = self.fresh_label("matrix.matmul.inner.body");
        let inner_latch_lbl = self.fresh_label("matrix.matmul.inner.latch");
        let inner_done_lbl = self.fresh_label("matrix.matmul.inner.done");
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", row_cond_lbl).unwrap();
        let row = self.fresh();
        let has_row = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", row, row_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            has_row, row, left_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_row, row_body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", row_body_lbl).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_cond_lbl).unwrap();
        let col = self.fresh();
        let has_col = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", col, col_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            has_col, col, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_col, col_body_lbl, col_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", col_body_lbl).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll,
            matrix_zero_value(elem_ty),
            acc_addr
        )
        .unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", inner_addr).unwrap();
        writeln!(self.out, "  br label %{}", inner_cond_lbl).unwrap();

        writeln!(self.out, "{}:", inner_cond_lbl).unwrap();
        let inner = self.fresh();
        let has_inner = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", inner, inner_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            has_inner, inner, left_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_inner, inner_body_lbl, inner_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", inner_body_lbl).unwrap();
        let left_row_offset = self.fresh();
        let left_idx = self.fresh();
        let right_row_offset = self.fresh();
        let right_idx = self.fresh();
        let left_cell_ptr = self.fresh();
        let right_cell_ptr = self.fresh();
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        let acc = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            left_row_offset, row, left_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            left_idx, left_row_offset, inner
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            right_row_offset, inner, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            right_idx, right_row_offset, col
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            left_cell_ptr, ll, left_data, left_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            right_cell_ptr, ll, right_data, right_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            left_cell, ll, left_cell_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            right_cell, ll, right_cell_ptr
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", acc, ll, acc_addr).unwrap();
        let product = self.emit_matrix_elem_binop(
            elem_ty,
            &format!("%{left_cell}"),
            &format!("%{right_cell}"),
            "*",
        );
        let next_acc = self.emit_matrix_elem_binop(elem_ty, &format!("%{acc}"), &product, "+");
        writeln!(self.out, "  store {} {}, ptr %{}", ll, next_acc, acc_addr).unwrap();
        writeln!(self.out, "  br label %{}", inner_latch_lbl).unwrap();

        writeln!(self.out, "{}:", inner_latch_lbl).unwrap();
        let next_inner = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_inner, inner).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_inner, inner_addr).unwrap();
        writeln!(self.out, "  br label %{}", inner_cond_lbl).unwrap();

        writeln!(self.out, "{}:", inner_done_lbl).unwrap();
        let out_row_offset = self.fresh();
        let out_idx = self.fresh();
        let out_cell_ptr = self.fresh();
        let out_value = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            out_row_offset, row, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            out_idx, out_row_offset, col
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell_ptr, ll, data, out_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            out_value, ll, acc_addr
        )
        .unwrap();
        writeln!(
            self.out,
            "  store {} %{}, ptr %{}",
            ll, out_value, out_cell_ptr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", col_latch_lbl).unwrap();

        writeln!(self.out, "{}:", col_latch_lbl).unwrap();
        let next_col = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_col, col).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_col, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_done_lbl).unwrap();
        writeln!(self.out, "  br label %{}", row_latch_lbl).unwrap();

        writeln!(self.out, "{}:", row_latch_lbl).unwrap();
        let next_row = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_row, row).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_row, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (Ty::Matrix(Box::new(elem_ty.clone()), shape), out_matrix)
    }

    fn emit_matrix_det_lu(&mut self, elem_ty: &Ty, matrix: &str, n: &str) -> (Ty, String) {
        let ll = llvm_ty(elem_ty, self.structs);
        let src_data = self.matrix_load_data_ptr(matrix);
        let cell_count = self.fresh();
        let bytes = self.fresh();
        let work = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", cell_count, n, n).unwrap();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            bytes,
            cell_count,
            matrix_elem_size(elem_ty)
        )
        .unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 %{})", work, bytes).unwrap();

        let copy_idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", copy_idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", copy_idx_addr).unwrap();
        let copy_cond_lbl = self.fresh_label("matrix.det.lu.copy.cond");
        let copy_body_lbl = self.fresh_label("matrix.det.lu.copy.body");
        let copy_done_lbl = self.fresh_label("matrix.det.lu.copy.done");
        writeln!(self.out, "  br label %{}", copy_cond_lbl).unwrap();

        writeln!(self.out, "{}:", copy_cond_lbl).unwrap();
        let copy_idx = self.fresh();
        let copy_has = self.fresh();
        writeln!(
            self.out,
            "  %{} = load i64, ptr %{}",
            copy_idx, copy_idx_addr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            copy_has, copy_idx, cell_count
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            copy_has, copy_body_lbl, copy_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", copy_body_lbl).unwrap();
        let src_ptr = self.fresh();
        let dst_ptr = self.fresh();
        let copy_val = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            src_ptr, ll, src_data, copy_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            dst_ptr, ll, work, copy_idx
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", copy_val, ll, src_ptr).unwrap();
        writeln!(self.out, "  store {} %{}, ptr %{}", ll, copy_val, dst_ptr).unwrap();
        let copy_next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", copy_next, copy_idx).unwrap();
        writeln!(
            self.out,
            "  store i64 %{}, ptr %{}",
            copy_next, copy_idx_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", copy_cond_lbl).unwrap();

        writeln!(self.out, "{}:", copy_done_lbl).unwrap();
        let det_addr = self.fresh();
        let sign_addr = self.fresh();
        let k_addr = self.fresh();
        let pivot_addr = self.fresh();
        let row_addr = self.fresh();
        let col_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", det_addr, ll).unwrap();
        writeln!(self.out, "  %{} = alloca i1", sign_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", k_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", pivot_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", row_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", col_addr).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll,
            matrix_one_value(elem_ty),
            det_addr
        )
        .unwrap();
        writeln!(self.out, "  store i1 1, ptr %{}", sign_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", k_addr).unwrap();

        let k_cond_lbl = self.fresh_label("matrix.det.lu.k.cond");
        let k_body_lbl = self.fresh_label("matrix.det.lu.k.body");
        let find_cond_lbl = self.fresh_label("matrix.det.lu.find.cond");
        let find_check_lbl = self.fresh_label("matrix.det.lu.find.check");
        let find_next_lbl = self.fresh_label("matrix.det.lu.find.next");
        let pivot_found_lbl = self.fresh_label("matrix.det.lu.pivot.found");
        let zero_lbl = self.fresh_label("matrix.det.lu.zero");
        let swap_cond_lbl = self.fresh_label("matrix.det.lu.swap.cond");
        let swap_body_lbl = self.fresh_label("matrix.det.lu.swap.body");
        let swap_done_lbl = self.fresh_label("matrix.det.lu.swap.done");
        let no_swap_lbl = self.fresh_label("matrix.det.lu.swap.skip");
        let after_swap_lbl = self.fresh_label("matrix.det.lu.after.swap");
        let row_cond_lbl = self.fresh_label("matrix.det.lu.row.cond");
        let row_body_lbl = self.fresh_label("matrix.det.lu.row.body");
        let col_cond_lbl = self.fresh_label("matrix.det.lu.col.cond");
        let col_body_lbl = self.fresh_label("matrix.det.lu.col.body");
        let col_done_lbl = self.fresh_label("matrix.det.lu.col.done");
        let row_latch_lbl = self.fresh_label("matrix.det.lu.row.latch");
        let k_latch_lbl = self.fresh_label("matrix.det.lu.k.latch");
        let product_done_lbl = self.fresh_label("matrix.det.lu.product.done");
        let sign_neg_lbl = self.fresh_label("matrix.det.lu.sign.neg");
        let cleanup_lbl = self.fresh_label("matrix.det.lu.cleanup");
        writeln!(self.out, "  br label %{}", k_cond_lbl).unwrap();

        writeln!(self.out, "{}:", k_cond_lbl).unwrap();
        let k = self.fresh();
        let has_k = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", k, k_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_k, k, n).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_k, k_body_lbl, product_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", k_body_lbl).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", k, pivot_addr).unwrap();
        writeln!(self.out, "  br label %{}", find_cond_lbl).unwrap();

        writeln!(self.out, "{}:", find_cond_lbl).unwrap();
        let pivot = self.fresh();
        let pivot_has = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", pivot, pivot_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            pivot_has, pivot, n
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            pivot_has, find_check_lbl, zero_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", find_check_lbl).unwrap();
        let pivot_cell_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{pivot}"),
            &format!("%{k}"),
            elem_ty,
        );
        let pivot_cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            pivot_cell, ll, pivot_cell_ptr
        )
        .unwrap();
        let pivot_is_zero = self.emit_matrix_elem_is_zero(elem_ty, &format!("%{pivot_cell}"));
        writeln!(
            self.out,
            "  br i1 {}, label %{}, label %{}",
            pivot_is_zero, find_next_lbl, pivot_found_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", find_next_lbl).unwrap();
        let next_pivot = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_pivot, pivot).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_pivot, pivot_addr).unwrap();
        writeln!(self.out, "  br label %{}", find_cond_lbl).unwrap();

        writeln!(self.out, "{}:", zero_lbl).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll,
            matrix_zero_value(elem_ty),
            det_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", cleanup_lbl).unwrap();

        writeln!(self.out, "{}:", pivot_found_lbl).unwrap();
        let pivot_final = self.fresh();
        let needs_swap = self.fresh();
        writeln!(
            self.out,
            "  %{} = load i64, ptr %{}",
            pivot_final, pivot_addr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp ne i64 %{}, %{}",
            needs_swap, pivot_final, k
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            needs_swap, swap_cond_lbl, no_swap_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", swap_cond_lbl).unwrap();
        let swap_col = self.fresh();
        let swap_has_col = self.fresh();
        writeln!(self.out, "  store i64 0, ptr %{}", col_addr).unwrap();
        writeln!(self.out, "  br label %{}", swap_body_lbl).unwrap();

        writeln!(self.out, "{}:", swap_body_lbl).unwrap();
        writeln!(self.out, "  %{} = load i64, ptr %{}", swap_col, col_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            swap_has_col, swap_col, n
        )
        .unwrap();
        let swap_do_lbl = self.fresh_label("matrix.det.lu.swap.do");
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            swap_has_col, swap_do_lbl, swap_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", swap_do_lbl).unwrap();
        let a_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{k}"),
            &format!("%{swap_col}"),
            elem_ty,
        );
        let b_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{pivot_final}"),
            &format!("%{swap_col}"),
            elem_ty,
        );
        let a_val = self.fresh();
        let b_val = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr {}", a_val, ll, a_ptr).unwrap();
        writeln!(self.out, "  %{} = load {}, ptr {}", b_val, ll, b_ptr).unwrap();
        writeln!(self.out, "  store {} %{}, ptr {}", ll, b_val, a_ptr).unwrap();
        writeln!(self.out, "  store {} %{}, ptr {}", ll, a_val, b_ptr).unwrap();
        let swap_next_col = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", swap_next_col, swap_col).unwrap();
        writeln!(
            self.out,
            "  store i64 %{}, ptr %{}",
            swap_next_col, col_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", swap_body_lbl).unwrap();

        writeln!(self.out, "{}:", swap_done_lbl).unwrap();
        let sign = self.fresh();
        let next_sign = self.fresh();
        writeln!(self.out, "  %{} = load i1, ptr %{}", sign, sign_addr).unwrap();
        writeln!(self.out, "  %{} = xor i1 %{}, true", next_sign, sign).unwrap();
        writeln!(self.out, "  store i1 %{}, ptr %{}", next_sign, sign_addr).unwrap();
        writeln!(self.out, "  br label %{}", after_swap_lbl).unwrap();

        writeln!(self.out, "{}:", no_swap_lbl).unwrap();
        writeln!(self.out, "  br label %{}", after_swap_lbl).unwrap();

        writeln!(self.out, "{}:", after_swap_lbl).unwrap();
        let pivot_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{k}"),
            &format!("%{k}"),
            elem_ty,
        );
        let pivot_val = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            pivot_val, ll, pivot_ptr
        )
        .unwrap();
        let det_current = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            det_current, ll, det_addr
        )
        .unwrap();
        let det_next = self.emit_matrix_elem_binop(
            elem_ty,
            &format!("%{det_current}"),
            &format!("%{pivot_val}"),
            "*",
        );
        writeln!(self.out, "  store {} {}, ptr %{}", ll, det_next, det_addr).unwrap();
        let first_row = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", first_row, k).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", first_row, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", row_cond_lbl).unwrap();
        let row = self.fresh();
        let has_row = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", row, row_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_row, row, n).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_row, row_body_lbl, k_latch_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", row_body_lbl).unwrap();
        let below_pivot_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{row}"),
            &format!("%{k}"),
            elem_ty,
        );
        let below_pivot = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            below_pivot, ll, below_pivot_ptr
        )
        .unwrap();
        let factor = self.emit_matrix_elem_div(
            elem_ty,
            &format!("%{below_pivot}"),
            &format!("%{pivot_val}"),
        );
        writeln!(self.out, "  store i64 %{}, ptr %{}", first_row, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_cond_lbl).unwrap();
        let col = self.fresh();
        let has_col = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", col, col_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_col, col, n).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_col, col_body_lbl, col_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", col_body_lbl).unwrap();
        let aij_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{row}"),
            &format!("%{col}"),
            elem_ty,
        );
        let akj_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{k}"),
            &format!("%{col}"),
            elem_ty,
        );
        let aij = self.fresh();
        let akj = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr {}", aij, ll, aij_ptr).unwrap();
        writeln!(self.out, "  %{} = load {}, ptr {}", akj, ll, akj_ptr).unwrap();
        let scaled = self.emit_matrix_elem_binop(elem_ty, &factor, &format!("%{akj}"), "*");
        let updated = self.emit_matrix_elem_binop(elem_ty, &format!("%{aij}"), &scaled, "-");
        writeln!(self.out, "  store {} {}, ptr {}", ll, updated, aij_ptr).unwrap();
        let next_col = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_col, col).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_col, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_done_lbl).unwrap();
        writeln!(self.out, "  br label %{}", row_latch_lbl).unwrap();

        writeln!(self.out, "{}:", row_latch_lbl).unwrap();
        let next_row = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_row, row).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_row, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", k_latch_lbl).unwrap();
        let next_k = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_k, k).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_k, k_addr).unwrap();
        writeln!(self.out, "  br label %{}", k_cond_lbl).unwrap();

        writeln!(self.out, "{}:", product_done_lbl).unwrap();
        let final_sign = self.fresh();
        writeln!(self.out, "  %{} = load i1, ptr %{}", final_sign, sign_addr).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            final_sign, cleanup_lbl, sign_neg_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", sign_neg_lbl).unwrap();
        let positive_det = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            positive_det, ll, det_addr
        )
        .unwrap();
        let negative_det = self.emit_matrix_elem_neg(elem_ty, &format!("%{positive_det}"));
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll, negative_det, det_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", cleanup_lbl).unwrap();

        writeln!(self.out, "{}:", cleanup_lbl).unwrap();
        writeln!(self.out, "  call void @free(ptr %{})", work).unwrap();
        let result = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", result, ll, det_addr).unwrap();
        (elem_ty.clone(), format!("%{result}"))
    }

    fn emit_matrix_det_value(&mut self, elem_ty: Ty, matrix: &str) -> (Ty, String) {
        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 2);
        let square = self.fresh();
        let ok_lbl = self.fresh_label("matrix.det.shape.ok");
        let abort_lbl = self.fresh_label("matrix.det.shape.abort");
        writeln!(self.out, "  %{} = icmp eq i64 {}, {}", square, rows, cols).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            square, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        self.emit_matrix_det_lu(&elem_ty, matrix, &rows)
    }

    /// Resolves numeric field index for named/tuple field access codegen.
    fn struct_idx(&self, sname: &str, field: &str) -> Option<u32> {
        let s = self.structs.iter().find(|s| s.name == sname)?;
        s.fields
            .iter()
            .position(|(n, _)| n == field)
            .map(|i| i as u32)
    }

    fn vector_idx(&self, vname: &str, field: &str) -> Option<u32> {
        let v = self.vectors.iter().find(|v| v.name == vname)?;
        v.fields.iter().position(|n| n == field).map(|i| i as u32)
    }

    /// If `t` is a declared `vector Name ...`, returns `Name`.
    fn as_nia_vector_name(&self, t: &Ty) -> Option<&str> {
        match t {
            Ty::Struct(s) => self
                .vectors
                .iter()
                .find(|v| v.name == *s)
                .map(|v| v.name.as_str()),
            Ty::Vector(n, _) => self
                .vectors
                .iter()
                .find(|v| v.name == *n)
                .map(|v| v.name.as_str()),
            _ => None,
        }
    }

    fn vector_value_meta(&self, t: &Ty) -> Option<(Ty, usize, String)> {
        match t {
            Ty::Struct(name) | Ty::Vector(name, _) => {
                let v = self.vectors.iter().find(|v| v.name == *name)?;
                Some((
                    v.ty.clone(),
                    v.fields.len(),
                    format!("%struct.{}", sanitize(&v.name)),
                ))
            }
            Ty::AnonVector(elem, n) => Some((elem.as_ref().clone(), *n, llvm_ty(t, self.structs))),
            _ => None,
        }
    }

    /// Single-axis `+` / `-` for vector components (matches scalar `Expr::Add` / `Sub`).
    fn emit_scalar_vec_binop(&mut self, elem_ty: &Ty, a: &str, b: &str, is_add: bool) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i8 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i16 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I32 => {
                let op = if is_add { "add nsw" } else { "sub nsw" };
                writeln!(self.out, "  %{} = {} i32 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::U32 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i32 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i64 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i128 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let op = if is_add { "fadd" } else { "fsub" };
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(self.out, "  %{} = {} {} %{}, %{}", out, op, ll, a, b).unwrap();
            }
            _ => unreachable!("typechecked vector axis"),
        }
        out
    }

    /// Per-axis `*` between two vector components (matches scalar `Expr::Mul` on integers/floats).
    fn emit_scalar_vec_mul_pair(&mut self, elem_ty: &Ty, a: &str, b: &str) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                writeln!(self.out, "  %{} = mul i8 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                writeln!(self.out, "  %{} = mul i16 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I32 => {
                writeln!(self.out, "  %{} = mul nsw i32 %{}, %{}", out, a, b).unwrap();
            }
            Ty::U32 => {
                writeln!(self.out, "  %{} = mul i32 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(self.out, "  %{} = mul i64 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                writeln!(self.out, "  %{} = mul i128 %{}, %{}", out, a, b).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(self.out, "  %{} = fmul {} %{}, %{}", out, ll, a, b).unwrap();
            }
            _ => unreachable!("typechecked vector axis"),
        }
        out
    }

    fn emit_scalar_vec_mul(&mut self, elem_ty: &Ty, comp: &str, scalar_ssa: &str) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                writeln!(self.out, "  %{} = mul i8 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                writeln!(self.out, "  %{} = mul i16 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I32 => {
                writeln!(
                    self.out,
                    "  %{} = mul nsw i32 %{}, {}",
                    out, comp, scalar_ssa
                )
                .unwrap();
            }
            Ty::U32 => {
                writeln!(self.out, "  %{} = mul i32 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(self.out, "  %{} = mul i64 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                writeln!(self.out, "  %{} = mul i128 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(
                    self.out,
                    "  %{} = fmul {} %{}, {}",
                    out, ll, comp, scalar_ssa
                )
                .unwrap();
            }
            _ => unreachable!("typechecked vector axis"),
        }
        out
    }

    fn vector_axis_ty_cloned(&self, vname: &str) -> Ty {
        self.vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("vector decl")
            .ty
            .clone()
    }

    fn emit_nia_vector_scalar_mul(
        &mut self,
        vname: &str,
        vec_val: &str,
        scalar_ssa: &str,
    ) -> String {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector scalar mul");
        let llvm_st = format!("%struct.{}", sanitize(vname));
        let elem_ty = &vdef.ty;
        let mut agg = "poison".to_string();
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vec_val, i
            )
            .unwrap();
            let ci = self.emit_scalar_vec_mul(elem_ty, &ai, scalar_ssa);
            let ci_v = format!("%{}", ci);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_st,
                agg,
                llvm_ty(elem_ty, self.structs),
                ci_v,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", slot, llvm_st).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, slot).unwrap();
        let loadt = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, slot).unwrap();
        format!("%{loadt}")
    }

    /// Component-wise vector sum/difference (`vector` decl aggregates).
    fn emit_nia_vector_binop(&mut self, vname: &str, vl: &str, vr: &str, is_add: bool) -> String {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector binop");
        let llvm_st = format!("%struct.{}", sanitize(vname));
        let elem_ty = &vdef.ty;
        let mut agg = "poison".to_string();
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vl, i
            )
            .unwrap();
            let bi = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                bi, llvm_st, vr, i
            )
            .unwrap();
            let ci = self.emit_scalar_vec_binop(elem_ty, &ai, &bi, is_add);
            let ci_v = format!("%{}", ci);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_st,
                agg,
                llvm_ty(elem_ty, self.structs),
                ci_v,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", slot, llvm_st).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, slot).unwrap();
        let loadt = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, slot).unwrap();
        format!("%{loadt}")
    }

    /// Component-wise vector product (`vector` decl aggregates).
    fn emit_nia_vector_component_mul(&mut self, vname: &str, vl: &str, vr: &str) -> String {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector component mul");
        let llvm_st = format!("%struct.{}", sanitize(vname));
        let elem_ty = &vdef.ty;
        let mut agg = "poison".to_string();
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vl, i
            )
            .unwrap();
            let bi = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                bi, llvm_st, vr, i
            )
            .unwrap();
            let ci = self.emit_scalar_vec_mul_pair(elem_ty, &ai, &bi);
            let ci_v = format!("%{}", ci);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_st,
                agg,
                llvm_ty(elem_ty, self.structs),
                ci_v,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", slot, llvm_st).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, slot).unwrap();
        let loadt = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, slot).unwrap();
        format!("%{loadt}")
    }

    /// Dot product of two `vector` values (sum of per-axis products).
    fn emit_nia_vector_dot(&mut self, vname: &str, vl: &str, vr: &str) -> (Ty, String) {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector dot");
        let elem_ty = vdef.ty.clone();
        let llvm_st = format!("%struct.{}", sanitize(vname));
        assert!(
            !vdef.fields.is_empty(),
            "vector type must have at least one axis"
        );
        let mut acc: Option<String> = None;
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vl, i
            )
            .unwrap();
            let bi = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                bi, llvm_st, vr, i
            )
            .unwrap();
            let pi = self.emit_scalar_vec_mul_pair(&elem_ty, &ai, &bi);
            acc = Some(match acc {
                None => pi,
                Some(prev) => self.emit_scalar_vec_binop(&elem_ty, &prev, &pi, true),
            });
        }
        let id = acc.expect("non-empty vector");
        (elem_ty, format!("%{id}"))
    }

    fn emit_heap_vector_lit(
        &mut self,
        elems: &[Expr],
        elem_ty: &Ty,
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let len = elems.len();
        let (vector, data) = self.emit_heap_vector_alloc(elem_ty, &len.to_string());
        for (i, elem) in elems.iter().enumerate() {
            let (et, value) = self.emit_expr(elem, locals, Some(elem_ty));
            debug_assert!(types_match(&et, elem_ty));
            let cell = self.heap_vector_data_cell_ptr(&data, &i.to_string(), elem_ty);
            writeln!(
                self.out,
                "  store {} {}, ptr {}",
                llvm_ty(elem_ty, self.structs),
                value,
                cell
            )
            .unwrap();
        }
        (Ty::HeapVector(Box::new(elem_ty.clone())), vector)
    }

    fn emit_heap_vector_binop(
        &mut self,
        elem_ty: &Ty,
        left: &str,
        right: &str,
        is_add: bool,
    ) -> (Ty, String) {
        let len =
            self.emit_heap_vector_shape_check(left, right, if is_add { "add" } else { "sub" });
        let (out_vector, out_data) = self.emit_heap_vector_alloc(elem_ty, &len);
        let left_data = self.heap_vector_load_data_ptr(left);
        let right_data = self.heap_vector_load_data_ptr(right);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let label = if is_add { "add" } else { "sub" };
        let cond_lbl = self.fresh_label(&format!("heap.vector.{label}.cond"));
        let body_lbl = self.fresh_label(&format!("heap.vector.{label}.body"));
        let latch_lbl = self.fresh_label(&format!("heap.vector.{label}.latch"));
        let done_lbl = self.fresh_label(&format!("heap.vector.{label}.done"));
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_item, idx, len).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let left_ptr = self.heap_vector_data_cell_ptr(&left_data, &format!("%{idx}"), elem_ty);
        let right_ptr = self.heap_vector_data_cell_ptr(&right_data, &format!("%{idx}"), elem_ty);
        let out_ptr = self.heap_vector_data_cell_ptr(&out_data, &format!("%{idx}"), elem_ty);
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        let elem_ll = llvm_ty(elem_ty, self.structs);
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            left_cell, elem_ll, left_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            right_cell, elem_ll, right_ptr
        )
        .unwrap();
        let value = self.emit_scalar_vec_binop(elem_ty, &left_cell, &right_cell, is_add);
        writeln!(self.out, "  store {} %{}, ptr {}", elem_ll, value, out_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (Ty::HeapVector(Box::new(elem_ty.clone())), out_vector)
    }

    fn emit_heap_vector_component_mul(
        &mut self,
        elem_ty: &Ty,
        left: &str,
        right: &str,
    ) -> (Ty, String) {
        let len = self.emit_heap_vector_shape_check(left, right, "mul");
        let (out_vector, out_data) = self.emit_heap_vector_alloc(elem_ty, &len);
        let left_data = self.heap_vector_load_data_ptr(left);
        let right_data = self.heap_vector_load_data_ptr(right);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label("heap.vector.mul.cond");
        let body_lbl = self.fresh_label("heap.vector.mul.body");
        let latch_lbl = self.fresh_label("heap.vector.mul.latch");
        let done_lbl = self.fresh_label("heap.vector.mul.done");
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_item, idx, len).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let left_ptr = self.heap_vector_data_cell_ptr(&left_data, &format!("%{idx}"), elem_ty);
        let right_ptr = self.heap_vector_data_cell_ptr(&right_data, &format!("%{idx}"), elem_ty);
        let out_ptr = self.heap_vector_data_cell_ptr(&out_data, &format!("%{idx}"), elem_ty);
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        let elem_ll = llvm_ty(elem_ty, self.structs);
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            left_cell, elem_ll, left_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            right_cell, elem_ll, right_ptr
        )
        .unwrap();
        let value = self.emit_scalar_vec_mul_pair(elem_ty, &left_cell, &right_cell);
        writeln!(self.out, "  store {} %{}, ptr {}", elem_ll, value, out_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (Ty::HeapVector(Box::new(elem_ty.clone())), out_vector)
    }

    fn emit_heap_vector_scalar_mul(
        &mut self,
        elem_ty: &Ty,
        vector: &str,
        scalar: &str,
    ) -> (Ty, String) {
        let len = self.heap_vector_load_i64_field(vector, 1);
        let (out_vector, out_data) = self.emit_heap_vector_alloc(elem_ty, &len);
        let data = self.heap_vector_load_data_ptr(vector);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label("heap.vector.scalar.mul.cond");
        let body_lbl = self.fresh_label("heap.vector.scalar.mul.body");
        let latch_lbl = self.fresh_label("heap.vector.scalar.mul.latch");
        let done_lbl = self.fresh_label("heap.vector.scalar.mul.done");
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_item, idx, len).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let cell_ptr = self.heap_vector_data_cell_ptr(&data, &format!("%{idx}"), elem_ty);
        let out_ptr = self.heap_vector_data_cell_ptr(&out_data, &format!("%{idx}"), elem_ty);
        let cell = self.fresh();
        let elem_ll = llvm_ty(elem_ty, self.structs);
        writeln!(self.out, "  %{} = load {}, ptr {}", cell, elem_ll, cell_ptr).unwrap();
        let value = self.emit_scalar_vec_mul(elem_ty, &cell, scalar);
        writeln!(self.out, "  store {} %{}, ptr {}", elem_ll, value, out_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (Ty::HeapVector(Box::new(elem_ty.clone())), out_vector)
    }

    fn emit_heap_vector_dot(&mut self, elem_ty: &Ty, left: &str, right: &str) -> (Ty, String) {
        let len = self.emit_heap_vector_shape_check(left, right, "dot");
        let left_data = self.heap_vector_load_data_ptr(left);
        let right_data = self.heap_vector_load_data_ptr(right);
        let idx_addr = self.fresh();
        let acc_addr = self.fresh();
        let elem_ll = llvm_ty(elem_ty, self.structs);
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  %{} = alloca {}", acc_addr, elem_ll).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            elem_ll,
            matrix_zero_value(elem_ty),
            acc_addr
        )
        .unwrap();

        let cond_lbl = self.fresh_label("heap.vector.dot.cond");
        let body_lbl = self.fresh_label("heap.vector.dot.body");
        let latch_lbl = self.fresh_label("heap.vector.dot.latch");
        let done_lbl = self.fresh_label("heap.vector.dot.done");
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_item, idx, len).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let left_ptr = self.heap_vector_data_cell_ptr(&left_data, &format!("%{idx}"), elem_ty);
        let right_ptr = self.heap_vector_data_cell_ptr(&right_data, &format!("%{idx}"), elem_ty);
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        let acc = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            left_cell, elem_ll, left_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            right_cell, elem_ll, right_ptr
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", acc, elem_ll, acc_addr).unwrap();
        let product = self.emit_scalar_vec_mul_pair(elem_ty, &left_cell, &right_cell);
        let next_acc = self.emit_scalar_vec_binop(elem_ty, &acc, &product, true);
        writeln!(
            self.out,
            "  store {} %{}, ptr %{}",
            elem_ll, next_acc, acc_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        let out = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", out, elem_ll, acc_addr).unwrap();
        (elem_ty.clone(), format!("%{out}"))
    }

    fn emit_anon_vector_lit(
        &mut self,
        elems: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        let mut emitted: Vec<String> = Vec::new();
        let (elem_ty, n) = match hint {
            Some(Ty::AnonVector(elem_ty, n)) => (elem_ty.as_ref().clone(), *n),
            Some(Ty::HeapVector(elem_ty)) => {
                return self.emit_heap_vector_lit(elems, elem_ty, locals);
            }
            _ => {
                let first = elems
                    .first()
                    .expect("typechecked non-empty anonymous vector");
                let (first_ty, first_value) = self.emit_expr(first, locals, None);
                emitted.push(first_value);
                (first_ty, elems.len())
            }
        };
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let mut agg = "poison".to_string();
        for (i, elem) in elems.iter().enumerate() {
            let value = if i < emitted.len() {
                emitted[i].clone()
            } else {
                let (_, value) = self.emit_expr(elem, locals, Some(&elem_ty));
                value
            };
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_vec,
                agg,
                llvm_ty(&elem_ty, self.structs),
                value,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        (vec_ty, agg)
    }

    fn emit_anon_vector_binop(
        &mut self,
        elem_ty: &Ty,
        n: usize,
        vl: &str,
        vr: &str,
        is_add: bool,
    ) -> String {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for i in 0..n {
            let left = self.fresh();
            let right = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left, llvm_vec, vl, i
            )
            .unwrap();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                right, llvm_vec, vr, i
            )
            .unwrap();
            let value = self.emit_scalar_vec_binop(elem_ty, &left, &right, is_add);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} %{}, {}",
                tmp, llvm_vec, agg, elem_ll, value, i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        agg
    }

    fn emit_anon_vector_component_mul(
        &mut self,
        elem_ty: &Ty,
        n: usize,
        vl: &str,
        vr: &str,
    ) -> String {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for i in 0..n {
            let left = self.fresh();
            let right = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left, llvm_vec, vl, i
            )
            .unwrap();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                right, llvm_vec, vr, i
            )
            .unwrap();
            let value = self.emit_scalar_vec_mul_pair(elem_ty, &left, &right);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} %{}, {}",
                tmp, llvm_vec, agg, elem_ll, value, i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        agg
    }

    fn emit_anon_vector_scalar_mul(
        &mut self,
        elem_ty: &Ty,
        n: usize,
        vec_val: &str,
        scalar: &str,
    ) -> String {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for i in 0..n {
            let cell = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                cell, llvm_vec, vec_val, i
            )
            .unwrap();
            let value = self.emit_scalar_vec_mul(elem_ty, &cell, scalar);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} %{}, {}",
                tmp, llvm_vec, agg, elem_ll, value, i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        agg
    }

    fn emit_anon_vector_dot(&mut self, elem_ty: &Ty, n: usize, vl: &str, vr: &str) -> (Ty, String) {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let mut acc: Option<String> = None;
        for i in 0..n {
            let left = self.fresh();
            let right = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left, llvm_vec, vl, i
            )
            .unwrap();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                right, llvm_vec, vr, i
            )
            .unwrap();
            let product = self.emit_scalar_vec_mul_pair(elem_ty, &left, &right);
            acc = Some(match acc {
                None => product,
                Some(prev) => self.emit_scalar_vec_binop(elem_ty, &prev, &product, true),
            });
        }
        let id = acc.expect("typechecked non-empty anonymous vector");
        (elem_ty.clone(), format!("%{id}"))
    }

    fn emit_matrix_vector_shape_check(
        &mut self,
        matrix: &str,
        expected_rows: usize,
        expected_cols: usize,
        label: &str,
    ) -> (String, String, String) {
        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 2);
        let rows_ok = self.fresh();
        let cols_ok = self.fresh();
        let shape_ok = self.fresh();
        let ok_lbl = self.fresh_label(&format!("{label}.shape.ok"));
        let abort_lbl = self.fresh_label(&format!("{label}.shape.abort"));
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            rows_ok, rows, expected_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            cols_ok, cols, expected_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = and i1 %{}, %{}",
            shape_ok, rows_ok, cols_ok
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            shape_ok, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        (rows, cols, self.matrix_load_data_ptr(matrix))
    }

    fn emit_matrix_vector_product(
        &mut self,
        matrix: &str,
        vec_ty: &Ty,
        vec_val: &str,
        out_ty: Ty,
        elem_ty: &Ty,
    ) -> (Ty, String) {
        let (_, in_len, vec_ll) = self
            .vector_value_meta(vec_ty)
            .expect("typechecked Matrix @ vector right operand");
        let (_, out_len, out_ll) = self
            .vector_value_meta(&out_ty)
            .expect("typechecked Matrix @ vector result");
        let (_, cols, data) =
            self.emit_matrix_vector_shape_check(matrix, out_len, in_len, "matrix.vector.matmul");
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for row in 0..out_len {
            let mut acc = matrix_zero_value(elem_ty).to_string();
            for col_idx in 0..in_len {
                let row_offset = self.fresh();
                let flat_idx = self.fresh();
                let matrix_cell_ptr = self.fresh();
                let matrix_cell = self.fresh();
                let vector_cell = self.fresh();
                writeln!(self.out, "  %{} = mul i64 {}, {}", row_offset, row, cols).unwrap();
                writeln!(
                    self.out,
                    "  %{} = add i64 %{}, {}",
                    flat_idx, row_offset, col_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
                    matrix_cell_ptr, elem_ll, data, flat_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr %{}",
                    matrix_cell, elem_ll, matrix_cell_ptr
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    vector_cell, vec_ll, vec_val, col_idx
                )
                .unwrap();
                let product = self.emit_matrix_elem_binop(
                    elem_ty,
                    &format!("%{matrix_cell}"),
                    &format!("%{vector_cell}"),
                    "*",
                );
                acc = self.emit_matrix_elem_binop(elem_ty, &acc, &product, "+");
            }
            let next = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next, out_ll, agg, elem_ll, acc, row
            )
            .unwrap();
            agg = format!("%{next}");
        }
        (out_ty, agg)
    }

    fn emit_vector_matrix_product(
        &mut self,
        vec_ty: &Ty,
        vec_val: &str,
        matrix: &str,
        out_ty: Ty,
        elem_ty: &Ty,
    ) -> (Ty, String) {
        let (_, in_len, vec_ll) = self
            .vector_value_meta(vec_ty)
            .expect("typechecked vector @ Matrix left operand");
        let (_, out_len, out_ll) = self
            .vector_value_meta(&out_ty)
            .expect("typechecked vector @ Matrix result");
        let (_, cols, data) =
            self.emit_matrix_vector_shape_check(matrix, in_len, out_len, "vector.matrix.matmul");
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for col_idx in 0..out_len {
            let mut acc = matrix_zero_value(elem_ty).to_string();
            for row in 0..in_len {
                let row_offset = self.fresh();
                let flat_idx = self.fresh();
                let matrix_cell_ptr = self.fresh();
                let matrix_cell = self.fresh();
                let vector_cell = self.fresh();
                writeln!(self.out, "  %{} = mul i64 {}, {}", row_offset, row, cols).unwrap();
                writeln!(
                    self.out,
                    "  %{} = add i64 %{}, {}",
                    flat_idx, row_offset, col_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
                    matrix_cell_ptr, elem_ll, data, flat_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr %{}",
                    matrix_cell, elem_ll, matrix_cell_ptr
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    vector_cell, vec_ll, vec_val, row
                )
                .unwrap();
                let product = self.emit_matrix_elem_binop(
                    elem_ty,
                    &format!("%{vector_cell}"),
                    &format!("%{matrix_cell}"),
                    "*",
                );
                acc = self.emit_matrix_elem_binop(elem_ty, &acc, &product, "+");
            }
            let next = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next, out_ll, agg, elem_ll, acc, col_idx
            )
            .unwrap();
            agg = format!("%{next}");
        }
        (out_ty, agg)
    }

    fn emit_outer(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (left_ty, left_val) = self.emit_expr(&args[0], locals, None);
        let (right_ty, right_val) = self.emit_expr(&args[1], locals, None);
        let (elem_ty, rows, left_ll) = self
            .vector_value_meta(&left_ty)
            .expect("typechecked outer left vector");
        let (right_elem_ty, cols, right_ll) = self
            .vector_value_meta(&right_ty)
            .expect("typechecked outer right vector");
        debug_assert!(types_match(&elem_ty, &right_elem_ty));

        let len = rows * cols;
        let bytes = len * matrix_elem_size(&elem_ty);
        let data = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        let matrix = self.emit_matrix_header_alloc(
            &format!("%{data}"),
            &rows.to_string(),
            &cols.to_string(),
        );

        let ll = llvm_ty(&elem_ty, self.structs);
        for i in 0..rows {
            let left_cell = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left_cell, left_ll, left_val, i
            )
            .unwrap();
            for j in 0..cols {
                let right_cell = self.fresh();
                let out_cell_ptr = self.fresh();
                let idx = i * cols + j;
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    right_cell, right_ll, right_val, j
                )
                .unwrap();
                let product = self.emit_scalar_vec_mul_pair(&elem_ty, &left_cell, &right_cell);
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr %{}, i64 {}",
                    out_cell_ptr, ll, data, idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store {} %{}, ptr %{}",
                    ll, product, out_cell_ptr
                )
                .unwrap();
            }
        }

        (Ty::Matrix(Box::new(elem_ty), Some((rows, cols))), matrix)
    }

    /// Emits one full LLVM function definition.
    ///
    /// ## Strategy
    /// - materialize params into stack slots for uniform local-variable handling,
    /// - lower statements in order, supporting early termination on `return`,
    /// - emit implicit final return from block tail for non-void functions.
    ///
    /// The function body is emitted in SSA-with-stack-slots style (simple and explicit).
    fn emit_fn(mut self, f: &FnDef) -> String {
        self.out.clear();
        let local_sig;
        let sig = if let Some(sig) = self.fn_sigs.get(&f.name) {
            sig
        } else {
            local_sig = FnSig {
                params: f.params.iter().map(|(_, t)| t.clone()).collect(),
                ret: f.ret.clone(),
                is_quantum: f.is_quantum,
            };
            &local_sig
        };
        let ret_ll = match &sig.ret {
            None => "void".into(),
            Some(t) => llvm_ty(t, self.structs),
        };
        let mut params = Vec::new();
        for ((pname, _), pty) in f.params.iter().zip(&sig.params) {
            let ll = llvm_ty(pty, self.structs);
            let ps = sanitize(pname);
            params.push(format!("{ll} %{ps}"));
        }
        let linkage = if f.is_extern || f.name == "main" {
            ""
        } else {
            "internal "
        };
        writeln!(
            self.out,
            "define {}{} @{}({}) {{",
            linkage,
            ret_ll,
            sanitize(&f.name),
            params.join(", ")
        )
        .unwrap();
        writeln!(self.out, "entry:").unwrap();

        // Allocate and initialize stack slots for all parameters to unify load/store path
        // with local variables.
        let mut local_ptr: HashMap<String, (Ty, String)> = HashMap::new();
        let mut drop_flags: HashMap<String, String> = HashMap::new();
        let mut visible_order: Vec<String> = Vec::new();
        for ((pname, _), pty) in f.params.iter().zip(&sig.params) {
            let ps = sanitize(pname);
            let ptr = format!("%{ps}.addr");
            writeln!(
                self.out,
                "  {} = alloca {}",
                ptr,
                llvm_ty(pty, self.structs)
            )
            .unwrap();
            writeln!(
                self.out,
                "  store {} %{ps}, ptr {}",
                llvm_ty(pty, self.structs),
                ptr
            )
            .unwrap();
            local_ptr.insert(pname.clone(), (pty.clone(), ptr));
            let is_drop_self = pname == "self"
                && f.name.ends_with("__drop")
                && self.has_custom_drop_ty(pty)
                && sig.params.len() == 1;
            if self.is_language_drop_ty(pty) && !is_drop_self {
                let flag = self.emit_alloc_drop_flag(pname, true);
                drop_flags.insert(pname.clone(), flag);
                visible_order.push(pname.clone());
            }
        }
        if !f.closure_captures.is_empty() {
            let (_, env_slot) = local_ptr
                .get(CLOSURE_ENV_PARAM)
                .expect("closure env param is emitted for captured closures")
                .clone();
            let env = self.fresh();
            writeln!(self.out, "  %{} = load ptr, ptr {}", env, env_slot).unwrap();
            let env_ll = Self::closure_env_ll_ty(&f.closure_captures);
            for (idx, (capture_name, capture_ty)) in f.closure_captures.iter().enumerate() {
                let field_ptr = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr %{}, i32 0, i32 {}",
                    field_ptr, env_ll, env, idx
                )
                .unwrap();
                local_ptr.insert(
                    capture_name.clone(),
                    (capture_ty.clone(), format!("%{field_ptr}")),
                );
            }
        }

        for st in &f.body.stmts {
            self.emit_stmt(
                st,
                &mut local_ptr,
                &mut drop_flags,
                &mut visible_order,
                sig.ret.as_ref(),
            );
            if self.terminated {
                break;
            }
        }

        if self.terminated {
            writeln!(self.out, "}}").unwrap();
            return self.out;
        }

        if let Some(ret_ty) = &sig.ret {
            let tail = f.body.tail.as_ref().unwrap();
            let (t, v) = self.emit_expr(tail, &local_ptr, Some(ret_ty));
            debug_assert!(types_match(&t, ret_ty));
            self.clear_consumed_custom_drop_locals_expr(tail, &local_ptr, &drop_flags, true);
            self.emit_drop_scope_from(0, &visible_order, &local_ptr, &drop_flags);
            if matches!(ret_ty, Ty::Unit) {
                writeln!(self.out, "  ret void").unwrap();
            } else {
                writeln!(self.out, "  ret {} {}", llvm_ty(ret_ty, self.structs), v).unwrap();
            }
        } else {
            if let Some(tail) = &f.body.tail {
                let (t, _) = self.emit_expr(tail, &local_ptr, Some(&Ty::Unit));
                debug_assert!(types_match(&t, &Ty::Unit));
                self.clear_consumed_custom_drop_locals_expr(tail, &local_ptr, &drop_flags, true);
            }
            self.emit_drop_scope_from(0, &visible_order, &local_ptr, &drop_flags);
            writeln!(self.out, "  ret void").unwrap();
        }

        writeln!(self.out, "}}").unwrap();
        self.out
    }

    /// Emits one statement into current function body.
    ///
    /// `locals` maps source names to `(type, stack_ptr)` and is cloned for `if` branch
    /// lowering to keep branch-local mutations isolated.
    fn emit_stmt(
        &mut self,
        st: &Stmt,
        locals: &mut HashMap<String, (Ty, String)>,
        drop_flags: &mut HashMap<String, String>,
        visible_order: &mut Vec<String>,
        fn_ret: Option<&Ty>,
    ) {
        match st {
            Stmt::Let { name, ty, init } => {
                let normalized_hint = ty.as_ref().map(normalize_codegen_ty);
                let t = if let Some(init) = init {
                    let (t, v) = self.emit_expr(init, locals, normalized_hint.as_ref());
                    self.clear_consumed_custom_drop_locals_expr(init, locals, drop_flags, true);
                    let ptr = format!("%{}.addr", sanitize(name));
                    writeln!(self.out, "  {} = alloca {}", ptr, llvm_ty(&t, self.structs)).unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr {}",
                        llvm_ty(&t, self.structs),
                        v,
                        ptr
                    )
                    .unwrap();
                    locals.insert(name.clone(), (t.clone(), ptr));
                    if self.is_language_drop_ty(&t) {
                        let flag = self.emit_alloc_drop_flag(name, true);
                        drop_flags.insert(name.clone(), flag);
                        visible_order.push(name.clone());
                    }
                    return;
                } else {
                    normalized_hint.expect("typechecked uninitialized let")
                };
                let ptr = format!("%{}.addr", sanitize(name));
                writeln!(self.out, "  {} = alloca {}", ptr, llvm_ty(&t, self.structs)).unwrap();
                locals.insert(name.clone(), (t.clone(), ptr));
                if self.is_language_drop_ty(&t) {
                    let flag = self.emit_alloc_drop_flag(name, false);
                    drop_flags.insert(name.clone(), flag);
                    visible_order.push(name.clone());
                }
            }
            Stmt::Expr(e) => {
                self.emit_expr(e, locals, None);
                self.clear_consumed_custom_drop_locals_expr(e, locals, drop_flags, true);
            }
            Stmt::Assign { target, value } => {
                let (tt, ptr_v) = self.emit_assign_ptr(target, locals);
                if let Expr::Ident(name) = target {
                    self.emit_drop_local_if_live(name, locals, drop_flags);
                }
                let (vt, vv) = self.emit_expr(value, locals, Some(&tt));
                self.clear_consumed_custom_drop_locals_expr(value, locals, drop_flags, true);
                debug_assert!(types_match(&tt, &vt));
                writeln!(
                    self.out,
                    "  store {} {}, ptr {}",
                    llvm_ty(&tt, self.structs),
                    vv,
                    ptr_v
                )
                .unwrap();
                if let Expr::Ident(name) = target {
                    if let Some(flag) = drop_flags.get(name) {
                        self.emit_set_drop_flag(flag, true);
                    }
                }
            }
            Stmt::Return(e) => {
                let Some(ret_ty) = fn_ret else {
                    unreachable!("typechecked")
                };
                let (t, v) = self.emit_expr(e, locals, Some(ret_ty));
                debug_assert!(types_match(&t, ret_ty));
                self.clear_consumed_custom_drop_locals_expr(e, locals, drop_flags, true);
                self.emit_drop_scope_from(0, visible_order, locals, drop_flags);
                if matches!(ret_ty, Ty::Unit) {
                    writeln!(self.out, "  ret void").unwrap();
                } else {
                    writeln!(self.out, "  ret {} {}", llvm_ty(ret_ty, self.structs), v).unwrap();
                }
                self.terminated = true;
            }
            Stmt::Break => {
                let Some(exit) = self.loop_exit_stack.last() else {
                    panic!("internal: `break` should be rejected by typecheck outside `loop`");
                };
                let exit = exit.clone();
                let start = *self.loop_scope_start_stack.last().unwrap_or(&0);
                self.emit_drop_scope_from(start, visible_order, locals, drop_flags);
                writeln!(self.out, "  br label %{}", exit).unwrap();
                self.terminated = true;
            }
            Stmt::If { cond, then_block } => {
                let (ct, cv) = self.emit_expr(cond, locals, Some(&Ty::Bool));
                debug_assert!(matches!(ct, Ty::Bool));
                let then_lbl = self.fresh_label("if.then");
                let cont_lbl = self.fresh_label("if.cont");
                writeln!(
                    self.out,
                    "  br i1 {}, label %{}, label %{}",
                    cv, then_lbl, cont_lbl
                )
                .unwrap();
                writeln!(self.out, "{}:", then_lbl).unwrap();
                let mut then_locals = locals.clone();
                let mut then_drop_flags = drop_flags.clone();
                let mut then_visible_order = visible_order.clone();
                let then_scope_start = then_visible_order.len();
                self.terminated = false;
                for st in &then_block.stmts {
                    self.emit_stmt(
                        st,
                        &mut then_locals,
                        &mut then_drop_flags,
                        &mut then_visible_order,
                        fn_ret,
                    );
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &then_block.tail {
                        self.emit_expr(tail, &then_locals, None);
                        self.clear_consumed_custom_drop_locals_expr(
                            tail,
                            &then_locals,
                            &then_drop_flags,
                            true,
                        );
                    }
                    self.emit_drop_scope_from(
                        then_scope_start,
                        &then_visible_order,
                        &then_locals,
                        &then_drop_flags,
                    );
                    writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
                }
                self.terminated = false;
                writeln!(self.out, "{}:", cont_lbl).unwrap();
            }
            Stmt::While { cond, body } => {
                if self.terminated {
                    return;
                }
                let cond_lbl = self.fresh_label("while.cond");
                let body_lbl = self.fresh_label("while.body");
                let exit_lbl = self.fresh_label("while.exit");
                writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

                writeln!(self.out, "{}:", cond_lbl).unwrap();
                let (ct, cv) = self.emit_expr(cond, locals, Some(&Ty::Bool));
                debug_assert!(matches!(ct, Ty::Bool));
                writeln!(
                    self.out,
                    "  br i1 {}, label %{}, label %{}",
                    cv, body_lbl, exit_lbl
                )
                .unwrap();

                writeln!(self.out, "{}:", body_lbl).unwrap();
                let mut body_locals = locals.clone();
                let mut body_drop_flags = drop_flags.clone();
                let mut body_visible_order = visible_order.clone();
                let body_scope_start = body_visible_order.len();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        fn_ret,
                    );
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                        self.clear_consumed_custom_drop_locals_expr(
                            tail,
                            &body_locals,
                            &body_drop_flags,
                            true,
                        );
                    }
                    self.emit_drop_scope_from(
                        body_scope_start,
                        &body_visible_order,
                        &body_locals,
                        &body_drop_flags,
                    );
                    writeln!(self.out, "  br label %{}", cond_lbl).unwrap();
                }
                self.terminated = false;
                writeln!(self.out, "{}:", exit_lbl).unwrap();
            }
            Stmt::Loop { body } => {
                if self.terminated {
                    return;
                }
                let iter_lbl = self.fresh_label("loop.iter");
                let exit_lbl = self.fresh_label("loop.exit");
                self.loop_exit_stack.push(exit_lbl.clone());

                writeln!(self.out, "  br label %{}", iter_lbl).unwrap();
                writeln!(self.out, "{}:", iter_lbl).unwrap();
                let mut body_locals = locals.clone();
                let mut body_drop_flags = drop_flags.clone();
                let mut body_visible_order = visible_order.clone();
                let body_scope_start = body_visible_order.len();
                self.loop_scope_start_stack.push(body_scope_start);
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        fn_ret,
                    );
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                        self.clear_consumed_custom_drop_locals_expr(
                            tail,
                            &body_locals,
                            &body_drop_flags,
                            true,
                        );
                    }
                    self.emit_drop_scope_from(
                        body_scope_start,
                        &body_visible_order,
                        &body_locals,
                        &body_drop_flags,
                    );
                    writeln!(self.out, "  br label %{}", iter_lbl).unwrap();
                }
                self.loop_scope_start_stack.pop();
                self.loop_exit_stack.pop();
                self.terminated = false;
                writeln!(self.out, "{}:", exit_lbl).unwrap();
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                if self.terminated {
                    return;
                }
                let pre = self.fresh_label("for.pre");
                let header = self.fresh_label("for.header");
                let body_lbl = self.fresh_label("for.body");
                let latch = self.fresh_label("for.latch");
                let exit = self.fresh_label("for.exit");

                writeln!(self.out, "  br label %{}", pre).unwrap();
                writeln!(self.out, "{}:", pre).unwrap();
                let (t_ev, v_start) = self.emit_expr(start, locals, None);
                let (_, v_end) = self.emit_expr(end, locals, Some(&t_ev));
                let ll = llvm_ty(&t_ev, self.structs);
                let slot = self.fresh();
                let var_ptr = format!("%{}.{}", sanitize(var), slot);
                writeln!(self.out, "  {} = alloca {}", var_ptr, ll).unwrap();
                writeln!(self.out, "  br label %{}", header).unwrap();

                writeln!(self.out, "{}:", header).unwrap();
                let iv = self.fresh();
                let iv_next = self.fresh();
                let cmp = self.fresh();
                let icmp_op = if int_ty_signed(&t_ev) { "slt" } else { "ult" };
                writeln!(
                    self.out,
                    "  %{} = phi {} [ {}, %{} ], [ %{}, %{} ]",
                    iv, ll, v_start, pre, iv_next, latch
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = icmp {} {} %{}, {}",
                    cmp, icmp_op, ll, iv, v_end
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  br i1 %{}, label %{}, label %{}",
                    cmp, body_lbl, exit
                )
                .unwrap();

                writeln!(self.out, "{}:", body_lbl).unwrap();
                writeln!(self.out, "  store {} %{}, ptr {}", ll, iv, var_ptr).unwrap();
                let mut body_locals = locals.clone();
                let mut body_drop_flags = drop_flags.clone();
                let mut body_visible_order = visible_order.clone();
                let body_scope_start = body_visible_order.len();
                body_locals.insert(var.clone(), (t_ev.clone(), var_ptr));
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        fn_ret,
                    );
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                        self.clear_consumed_custom_drop_locals_expr(
                            tail,
                            &body_locals,
                            &body_drop_flags,
                            true,
                        );
                    }
                    self.emit_drop_scope_from(
                        body_scope_start,
                        &body_visible_order,
                        &body_locals,
                        &body_drop_flags,
                    );
                    writeln!(self.out, "  br label %{}", latch).unwrap();
                }
                let body_term = self.terminated;
                self.terminated = false;
                if body_term {
                    panic!("internal: `return` inside `for` should be rejected by typecheck");
                }

                writeln!(self.out, "{}:", latch).unwrap();
                match &t_ev {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = add i8 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = add i16 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = add nsw i32 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::U32 => {
                        writeln!(self.out, "  %{} = add i32 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = add i64 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I128 => {
                        writeln!(self.out, "  %{} = add i128 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::U128 => {
                        writeln!(self.out, "  %{} = add i128 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        unreachable!("typechecked for range")
                    }
                    Ty::Array(_, _)
                    | Ty::AtomicBool
                    | Ty::AtomicI8
                    | Ty::AtomicU8
                    | Ty::AtomicI16
                    | Ty::AtomicU16
                    | Ty::AtomicI32
                    | Ty::AtomicU32
                    | Ty::AtomicI64
                    | Ty::AtomicU64
                    | Ty::AtomicI128
                    | Ty::AtomicU128
                    | Ty::AtomicIsize
                    | Ty::AtomicUsize
                    | Ty::AtomicPtr(_)
                    | Ty::Thread
                    | Ty::Struct(_)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::Qubit
                    | Ty::Result
                    | Ty::String
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::HeapVector(_)
                    | Ty::List(_)
                    | Ty::Arc(_)
                    | Ty::Mutex(_)
                    | Ty::MutexGuard(_)
                    | Ty::Option(_)
                    | Ty::ResultType(_, _)
                    | Ty::Matrix(_, _)
                    | Ty::Fn(_, _) => unreachable!("typechecked for range"),
                }
                writeln!(self.out, "  br label %{}", header).unwrap();

                writeln!(self.out, "{}:", exit).unwrap();
            }
            Stmt::Quant { body } => {
                if self.terminated {
                    return;
                }
                let mut body_locals = locals.clone();
                let mut body_drop_flags = drop_flags.clone();
                let mut body_visible_order = visible_order.clone();
                let body_scope_start = body_visible_order.len();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        fn_ret,
                    );
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                        self.clear_consumed_custom_drop_locals_expr(
                            tail,
                            &body_locals,
                            &body_drop_flags,
                            true,
                        );
                    }
                    self.emit_drop_scope_from(
                        body_scope_start,
                        &body_visible_order,
                        &body_locals,
                        &body_drop_flags,
                    );
                }
            }
            Stmt::Gpu { body } => {
                if self.terminated {
                    return;
                }
                let mut body_locals = locals.clone();
                let mut body_drop_flags = drop_flags.clone();
                let mut body_visible_order = visible_order.clone();
                let body_scope_start = body_visible_order.len();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        fn_ret,
                    );
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                        self.clear_consumed_custom_drop_locals_expr(
                            tail,
                            &body_locals,
                            &body_drop_flags,
                            true,
                        );
                    }
                    self.emit_drop_scope_from(
                        body_scope_start,
                        &body_visible_order,
                        &body_locals,
                        &body_drop_flags,
                    );
                }
            }
        }
    }

    /// Emits expression and returns `(type, value)` where value is either:
    /// - immediate literal text, or
    /// - SSA reference like `%t7`.
    ///
    /// Caller decides whether returned value should be stored, returned, printed, etc.
    fn emit_expr(
        &mut self,
        e: &Expr,
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        match e {
            Expr::Int(n) => match hint {
                Some(Ty::F16 | Ty::F32 | Ty::F64) => {
                    let tmp = self.fresh();
                    let t = hint.unwrap().clone();
                    writeln!(
                        self.out,
                        "  %{} = sitofp i32 {} to {}",
                        tmp,
                        n,
                        llvm_ty(&t, self.structs)
                    )
                    .unwrap();
                    (t, format!("%{tmp}"))
                }
                Some(Ty::I8) => (Ty::I8, format!("{n}")),
                Some(Ty::U8) => (Ty::U8, format!("{n}")),
                Some(Ty::I16) => (Ty::I16, format!("{n}")),
                Some(Ty::U16) => (Ty::U16, format!("{n}")),
                Some(Ty::I32) | None => (Ty::I32, format!("{n}")),
                Some(Ty::U32) => (Ty::U32, format!("{n}")),
                Some(Ty::I64) => (Ty::I64, format!("{n}")),
                Some(Ty::U64) => (Ty::U64, format!("{n}")),
                Some(Ty::I128) => (Ty::I128, format!("{n}")),
                Some(Ty::Isize) => (Ty::Isize, format!("{n}")),
                Some(Ty::Usize) => (Ty::Usize, format!("{n}")),
                Some(Ty::U128) => (Ty::U128, format!("{n}")),
                Some(Ty::Struct(_))
                | Some(Ty::Vector(_, _))
                | Some(Ty::AnonVector(_, _))
                | Some(Ty::HeapVector(_))
                | Some(Ty::List(_))
                | Some(Ty::Arc(_))
                | Some(Ty::Mutex(_))
                | Some(Ty::MutexGuard(_))
                | Some(Ty::Option(_))
                | Some(Ty::ResultType(_, _))
                | Some(Ty::Enum(_))
                | Some(Ty::Unit)
                | Some(Ty::Ptr(_))
                | Some(Ty::Bool)
                | Some(Ty::AtomicBool)
                | Some(Ty::AtomicI8)
                | Some(Ty::AtomicU8)
                | Some(Ty::AtomicI16)
                | Some(Ty::AtomicU16)
                | Some(Ty::AtomicI32)
                | Some(Ty::AtomicU32)
                | Some(Ty::AtomicI64)
                | Some(Ty::AtomicU64)
                | Some(Ty::AtomicI128)
                | Some(Ty::AtomicU128)
                | Some(Ty::AtomicIsize)
                | Some(Ty::AtomicUsize)
                | Some(Ty::AtomicPtr(_))
                | Some(Ty::Thread)
                | Some(Ty::Qubit)
                | Some(Ty::Result)
                | Some(Ty::String)
                | Some(Ty::Array(_, _))
                | Some(Ty::Matrix(_, _))
                | Some(Ty::Fn(_, _)) => {
                    unreachable!("typechecked")
                }
            },
            Expr::Float(v) => {
                let lit = format!("{:.17e}", v);
                let target = hint.cloned().unwrap_or(Ty::F64);
                let dbl = self.fresh();
                writeln!(self.out, "  %{} = fadd double {}, 0.0", dbl, lit).unwrap();
                match target {
                    Ty::F64 => (Ty::F64, format!("%{dbl}")),
                    Ty::F32 => {
                        let tmp = self.fresh();
                        writeln!(self.out, "  %{} = fptrunc double %{} to float", tmp, dbl)
                            .unwrap();
                        (Ty::F32, format!("%{tmp}"))
                    }
                    Ty::F16 => {
                        let tmp = self.fresh();
                        writeln!(self.out, "  %{} = fptrunc double %{} to half", tmp, dbl).unwrap();
                        (Ty::F16, format!("%{tmp}"))
                    }
                    _ => unreachable!("typechecked float literal"),
                }
            }
            Expr::Bool(b) => (Ty::Bool, if *b { "1".into() } else { "0".into() }),
            Expr::String(s) => {
                let sym = self
                    .str_lit_syms
                    .get(s)
                    .unwrap_or_else(|| panic!("missing string literal global for {s:?}"));
                let nbytes = s.len() + 1;
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds [{} x i8], ptr @{}, i64 0, i64 0",
                    tmp, nbytes, sym
                )
                .unwrap();
                (Ty::String, format!("%{tmp}"))
            }
            Expr::Ident(name) => {
                if name == OPTION_NONE {
                    return self.emit_option_none_value(hint);
                }
                let Some((ty, ptr)) = locals.get(name) else {
                    if name == PI {
                        return (Ty::F64, self.emit_pi_value());
                    }
                    if let Some(sig) = self.fn_sigs.get(name) {
                        let params = sig.params.clone();
                        let ret = sig.ret.clone().unwrap_or(Ty::Unit);
                        let wrapper = self.ensure_function_value_wrapper(name);
                        let value = self.emit_fn_value(
                            &format!("@{}", sanitize(&wrapper)),
                            "null",
                            "null",
                            "null",
                        );
                        return (Ty::Fn(params, Box::new(ret)), value);
                    }
                    if let Some((enum_name, variant)) = split_variant_path(name) {
                        return self.emit_enum_unit_variant(enum_name, variant);
                    }
                    unreachable!("checked var");
                };
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr {}",
                    tmp,
                    llvm_ty(ty, self.structs),
                    ptr
                )
                .unwrap();
                (ty.clone(), format!("%{tmp}"))
            }
            Expr::Neg(inner) => {
                let (t, v) = self.emit_expr(inner, locals, None);
                let tmp = self.fresh();
                match t {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = sub i8 0, {}", tmp, v).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = sub i16 0, {}", tmp, v).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = sub nsw i32 0, {}", tmp, v).unwrap();
                    }
                    Ty::U32 => {
                        writeln!(self.out, "  %{} = sub i32 0, {}", tmp, v).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = sub i64 0, {}", tmp, v).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = sub i128 0, {}", tmp, v).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&t, self.structs);
                        writeln!(self.out, "  %{} = fneg {} {}", tmp, ll, v).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::AtomicBool
                    | Ty::AtomicI8
                    | Ty::AtomicU8
                    | Ty::AtomicI16
                    | Ty::AtomicU16
                    | Ty::AtomicI32
                    | Ty::AtomicU32
                    | Ty::AtomicI64
                    | Ty::AtomicU64
                    | Ty::AtomicI128
                    | Ty::AtomicU128
                    | Ty::AtomicIsize
                    | Ty::AtomicUsize
                    | Ty::AtomicPtr(_)
                    | Ty::Thread
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::HeapVector(_)
                    | Ty::List(_)
                    | Ty::Arc(_)
                    | Ty::Mutex(_)
                    | Ty::MutexGuard(_)
                    | Ty::Option(_)
                    | Ty::ResultType(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::Qubit
                    | Ty::Result
                    | Ty::String
                    | Ty::Matrix(_, _)
                    | Ty::Fn(_, _) => {
                        unreachable!("typechecked neg")
                    }
                }
                (t, format!("%{tmp}"))
            }
            Expr::Not(inner) => {
                let (t, v) = self.emit_expr(inner, locals, Some(&Ty::Bool));
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = xor i1 {}, true", tmp, v).unwrap();
                (t, format!("%{tmp}"))
            }
            Expr::BitNot(inner) => {
                let (t, v) = self.emit_expr(inner, locals, hint);
                let tmp = self.fresh();
                let ll = llvm_ty(&t, self.structs);
                writeln!(self.out, "  %{} = xor {} {}, -1", tmp, ll, v).unwrap();
                (t, format!("%{tmp}"))
            }
            Expr::Add(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                if let Some(vname) = self.as_nia_vector_name(&tl).map(|s| s.to_string()) {
                    let out_v = self.emit_nia_vector_binop(&vname, &vl, &vr, true);
                    return (tl, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    let out_v = self.emit_anon_vector_binop(elem_ty, *n, &vl, &vr, true);
                    return (tl, out_v);
                }
                if let Ty::HeapVector(elem_ty) = &tl {
                    return self.emit_heap_vector_binop(elem_ty, &vl, &vr, true);
                }
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "+");
                }
                let tmp = self.fresh();
                match tl {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = add i8 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = add i16 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = add nsw i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::U32 => {
                        writeln!(self.out, "  %{} = add i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = add i64 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = add i128 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fadd {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::AtomicBool
                    | Ty::AtomicI8
                    | Ty::AtomicU8
                    | Ty::AtomicI16
                    | Ty::AtomicU16
                    | Ty::AtomicI32
                    | Ty::AtomicU32
                    | Ty::AtomicI64
                    | Ty::AtomicU64
                    | Ty::AtomicI128
                    | Ty::AtomicU128
                    | Ty::AtomicIsize
                    | Ty::AtomicUsize
                    | Ty::AtomicPtr(_)
                    | Ty::Thread
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::HeapVector(_)
                    | Ty::List(_)
                    | Ty::Arc(_)
                    | Ty::Mutex(_)
                    | Ty::MutexGuard(_)
                    | Ty::Option(_)
                    | Ty::ResultType(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::Qubit
                    | Ty::Result
                    | Ty::String
                    | Ty::Matrix(_, _)
                    | Ty::Fn(_, _) => {
                        unreachable!("add on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::Sub(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                if let Some(vname) = self.as_nia_vector_name(&tl).map(|s| s.to_string()) {
                    let out_v = self.emit_nia_vector_binop(&vname, &vl, &vr, false);
                    return (tl, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    let out_v = self.emit_anon_vector_binop(elem_ty, *n, &vl, &vr, false);
                    return (tl, out_v);
                }
                if let Ty::HeapVector(elem_ty) = &tl {
                    return self.emit_heap_vector_binop(elem_ty, &vl, &vr, false);
                }
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "-");
                }
                let tmp = self.fresh();
                match tl {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = sub i8 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = sub i16 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = sub nsw i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::U32 => {
                        writeln!(self.out, "  %{} = sub i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = sub i64 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = sub i128 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fsub {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::AtomicBool
                    | Ty::AtomicI8
                    | Ty::AtomicU8
                    | Ty::AtomicI16
                    | Ty::AtomicU16
                    | Ty::AtomicI32
                    | Ty::AtomicU32
                    | Ty::AtomicI64
                    | Ty::AtomicU64
                    | Ty::AtomicI128
                    | Ty::AtomicU128
                    | Ty::AtomicIsize
                    | Ty::AtomicUsize
                    | Ty::AtomicPtr(_)
                    | Ty::Thread
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::HeapVector(_)
                    | Ty::List(_)
                    | Ty::Arc(_)
                    | Ty::Mutex(_)
                    | Ty::MutexGuard(_)
                    | Ty::Option(_)
                    | Ty::ResultType(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::Qubit
                    | Ty::Result
                    | Ty::String
                    | Ty::Matrix(_, _)
                    | Ty::Fn(_, _) => {
                        unreachable!("sub on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::Mul(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                if let Some(vname) = self.as_nia_vector_name(&tl).map(|s| s.to_string()) {
                    let et = self.vector_axis_ty_cloned(&vname);
                    let (tr, vr) = self.emit_expr(r, locals, Some(&et));
                    if self.as_nia_vector_name(&tr).is_some() && types_match(&tl, &tr) {
                        let out_v = self.emit_nia_vector_component_mul(&vname, &vl, &vr);
                        return (tl, out_v);
                    }
                    let out_v = self.emit_nia_vector_scalar_mul(&vname, &vl, &vr);
                    return (tl, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    let (tr, vr) = match r.as_ref() {
                        Expr::AnonVectorLit(_) => self.emit_expr(r, locals, Some(&tl)),
                        _ => self.emit_expr(r, locals, Some(elem_ty)),
                    };
                    if matches!(tr, Ty::AnonVector(_, _)) {
                        debug_assert!(types_match(&tl, &tr));
                        let out_v = self.emit_anon_vector_component_mul(elem_ty, *n, &vl, &vr);
                        return (tl, out_v);
                    }
                    debug_assert!(types_match(&tr, elem_ty));
                    let out_v = self.emit_anon_vector_scalar_mul(elem_ty, *n, &vl, &vr);
                    return (tl, out_v);
                }
                if let Ty::HeapVector(elem_ty) = &tl {
                    let (tr, vr) = match r.as_ref() {
                        Expr::AnonVectorLit(_) => self.emit_expr(r, locals, Some(&tl)),
                        _ => self.emit_expr(r, locals, Some(elem_ty)),
                    };
                    if matches!(tr, Ty::HeapVector(_)) {
                        debug_assert!(types_match(&tl, &tr));
                        return self.emit_heap_vector_component_mul(elem_ty, &vl, &vr);
                    }
                    debug_assert!(types_match(&tr, elem_ty));
                    return self.emit_heap_vector_scalar_mul(elem_ty, &vl, &vr);
                }
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    let (tr, vr) = self.emit_expr(r, locals, None);
                    if let Ty::Matrix(_, _) = tr {
                        debug_assert!(types_match(&tl, &tr));
                        return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "*");
                    }
                    debug_assert!(types_match(&tr, elem_ty));
                    return self.emit_matrix_scalar_mul(&vl, &vr, elem_ty, *shape);
                }
                let (tr, vr) = match r.as_ref() {
                    Expr::AnonVectorLit(_) => self.emit_expr(r, locals, None),
                    _ => self.emit_expr(r, locals, Some(&tl)),
                };
                if let Some(vname) = self.as_nia_vector_name(&tr).map(|s| s.to_string()) {
                    let et = self.vector_axis_ty_cloned(&vname);
                    let (tl2, vl2) = self.emit_expr(l, locals, Some(&et));
                    if self.as_nia_vector_name(&tl2).is_some() && types_match(&tl2, &tr) {
                        let out_v = self.emit_nia_vector_component_mul(&vname, &vl2, &vr);
                        return (tr, out_v);
                    }
                    let out_v = self.emit_nia_vector_scalar_mul(&vname, &vr, &vl2);
                    return (tr, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tr {
                    debug_assert!(types_match(&tl, elem_ty));
                    let out_v = self.emit_anon_vector_scalar_mul(elem_ty, *n, &vr, &vl);
                    return (tr, out_v);
                }
                if let Ty::HeapVector(elem_ty) = &tr {
                    if matches!(tl, Ty::HeapVector(_)) {
                        debug_assert!(types_match(&tl, &tr));
                        return self.emit_heap_vector_component_mul(elem_ty, &vl, &vr);
                    }
                    debug_assert!(types_match(&tl, elem_ty));
                    return self.emit_heap_vector_scalar_mul(elem_ty, &vr, &vl);
                }
                if let Ty::Matrix(elem_ty, shape) = &tr {
                    debug_assert!(types_match(&tl, elem_ty));
                    return self.emit_matrix_scalar_mul(&vr, &vl, elem_ty, *shape);
                }
                assert!(types_match(&tl, &tr));
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "*");
                }
                let tmp = self.fresh();
                match tl {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = mul i8 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = mul i16 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = mul nsw i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::U32 => {
                        writeln!(self.out, "  %{} = mul i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = mul i64 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = mul i128 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fmul {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::AtomicBool
                    | Ty::AtomicI8
                    | Ty::AtomicU8
                    | Ty::AtomicI16
                    | Ty::AtomicU16
                    | Ty::AtomicI32
                    | Ty::AtomicU32
                    | Ty::AtomicI64
                    | Ty::AtomicU64
                    | Ty::AtomicI128
                    | Ty::AtomicU128
                    | Ty::AtomicIsize
                    | Ty::AtomicUsize
                    | Ty::AtomicPtr(_)
                    | Ty::Thread
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::HeapVector(_)
                    | Ty::List(_)
                    | Ty::Arc(_)
                    | Ty::Mutex(_)
                    | Ty::MutexGuard(_)
                    | Ty::Option(_)
                    | Ty::ResultType(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::Qubit
                    | Ty::Result
                    | Ty::String
                    | Ty::Matrix(_, _)
                    | Ty::Fn(_, _) => {
                        unreachable!("mul on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::VecDot(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                if let Ty::Matrix(elem_ty, left_shape) = &tl {
                    let right_hint =
                        left_shape.map(|(_, cols)| Ty::AnonVector(elem_ty.clone(), cols));
                    let (tr, vr) = match (r.as_ref(), right_hint.as_ref()) {
                        (Expr::AnonVectorLit(_), Some(hint_ty)) => {
                            self.emit_expr(r, locals, Some(hint_ty))
                        }
                        _ => self.emit_expr(r, locals, None),
                    };
                    if let Ty::Matrix(_, right_shape) = &tr {
                        debug_assert!(types_match(&tl, &tr));
                        let shape = match (left_shape, right_shape) {
                            (Some((rows, _)), Some((_, cols))) => Some((*rows, *cols)),
                            _ => None,
                        };
                        return self.emit_matrix_matmul(&vl, &vr, elem_ty, shape);
                    }
                    let (_, in_len, _) = self
                        .vector_value_meta(&tr)
                        .expect("typechecked Matrix @ vector right operand");
                    let out_ty = if let Some(hint_ty) = hint {
                        if self.vector_value_meta(hint_ty).is_some() {
                            hint_ty.clone()
                        } else {
                            unreachable!("typechecked Matrix @ vector result hint")
                        }
                    } else if let Some((rows, _)) = left_shape {
                        if *rows == in_len && self.as_nia_vector_name(&tr).is_some() {
                            tr.clone()
                        } else {
                            Ty::AnonVector(elem_ty.clone(), *rows)
                        }
                    } else {
                        unreachable!("typechecked Matrix @ vector result length")
                    };
                    return self.emit_matrix_vector_product(&vl, &tr, &vr, out_ty, elem_ty);
                }
                if let Ty::HeapVector(elem_ty) = &tl {
                    let (tr, vr) = match r.as_ref() {
                        Expr::AnonVectorLit(_) => self.emit_expr(r, locals, Some(&tl)),
                        _ => self.emit_expr(r, locals, None),
                    };
                    debug_assert!(types_match(&tl, &tr));
                    return self.emit_heap_vector_dot(elem_ty, &vl, &vr);
                }
                let (tr, vr) = self.emit_expr(r, locals, None);
                if let Ty::Matrix(elem_ty, matrix_shape) = &tr {
                    let (_, in_len, _) = self
                        .vector_value_meta(&tl)
                        .expect("typechecked vector @ Matrix left operand");
                    let out_ty = if let Some(hint_ty) = hint {
                        if self.vector_value_meta(hint_ty).is_some() {
                            hint_ty.clone()
                        } else {
                            unreachable!("typechecked vector @ Matrix result hint")
                        }
                    } else if let Some((_, cols)) = matrix_shape {
                        if *cols == in_len && self.as_nia_vector_name(&tl).is_some() {
                            tl.clone()
                        } else {
                            Ty::AnonVector(elem_ty.clone(), *cols)
                        }
                    } else {
                        unreachable!("typechecked vector @ Matrix result length")
                    };
                    return self.emit_vector_matrix_product(&tl, &vl, &vr, out_ty, elem_ty);
                }
                assert!(types_match(&tl, &tr));
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    return self.emit_anon_vector_dot(elem_ty, *n, &vl, &vr);
                }
                let vname = self
                    .as_nia_vector_name(&tl)
                    .expect("typechecked `@` on vectors")
                    .to_string();
                self.emit_nia_vector_dot(&vname, &vl, &vr)
            }
            Expr::Div(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                let signed = int_ty_signed(&tl);
                match tl {
                    Ty::I8 | Ty::U8 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i8 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i8 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I16 | Ty::U16 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i16 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i16 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I32 | Ty::U32 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i32 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i32 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i64 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i64 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I128 | Ty::U128 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i128 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i128 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fdiv {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::AtomicBool
                    | Ty::AtomicI8
                    | Ty::AtomicU8
                    | Ty::AtomicI16
                    | Ty::AtomicU16
                    | Ty::AtomicI32
                    | Ty::AtomicU32
                    | Ty::AtomicI64
                    | Ty::AtomicU64
                    | Ty::AtomicI128
                    | Ty::AtomicU128
                    | Ty::AtomicIsize
                    | Ty::AtomicUsize
                    | Ty::AtomicPtr(_)
                    | Ty::Thread
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::HeapVector(_)
                    | Ty::List(_)
                    | Ty::Arc(_)
                    | Ty::Mutex(_)
                    | Ty::MutexGuard(_)
                    | Ty::Option(_)
                    | Ty::ResultType(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::Qubit
                    | Ty::Result
                    | Ty::String
                    | Ty::Matrix(_, _)
                    | Ty::Fn(_, _) => {
                        unreachable!("div on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::Rem(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, hint);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                let ll = llvm_ty(&tl, self.structs);
                let op = if int_ty_signed(&tl) { "srem" } else { "urem" };
                writeln!(self.out, "  %{} = {} {} {}, {}", tmp, op, ll, vl, vr).unwrap();
                (tl, format!("%{tmp}"))
            }
            Expr::BitAnd(l, r) | Expr::BitOr(l, r) | Expr::BitXor(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, hint);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                let ll = llvm_ty(&tl, self.structs);
                let op = match e {
                    Expr::BitAnd(_, _) => "and",
                    Expr::BitOr(_, _) => "or",
                    Expr::BitXor(_, _) => "xor",
                    _ => unreachable!(),
                };
                writeln!(self.out, "  %{} = {} {} {}, {}", tmp, op, ll, vl, vr).unwrap();
                (tl, format!("%{tmp}"))
            }
            Expr::Shl(l, r) | Expr::Shr(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, hint);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                let ll = llvm_ty(&tl, self.structs);
                let op = match e {
                    Expr::Shl(_, _) => "shl",
                    Expr::Shr(_, _) if int_ty_signed(&tl) => "ashr",
                    Expr::Shr(_, _) => "lshr",
                    _ => unreachable!(),
                };
                writeln!(self.out, "  %{} = {} {} {}, {}", tmp, op, ll, vl, vr).unwrap();
                (tl, format!("%{tmp}"))
            }
            Expr::Eq(l, r)
            | Expr::Ne(l, r)
            | Expr::Lt(l, r)
            | Expr::Le(l, r)
            | Expr::Gt(l, r)
            | Expr::Ge(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                if matches!(tl, Ty::String) {
                    let cmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = call i32 @strcmp(ptr {}, ptr {})",
                        cmp, vl, vr
                    )
                    .unwrap();
                    let pred = match e {
                        Expr::Eq(_, _) => "eq",
                        Expr::Ne(_, _) => "ne",
                        _ => unreachable!("typechecked string ordering"),
                    };
                    writeln!(self.out, "  %{} = icmp {} i32 %{}, 0", tmp, pred, cmp).unwrap();
                } else if is_float_ty(&tl) {
                    let pred = match e {
                        Expr::Eq(_, _) => "oeq",
                        Expr::Ne(_, _) => "one",
                        Expr::Lt(_, _) => "olt",
                        Expr::Le(_, _) => "ole",
                        Expr::Gt(_, _) => "ogt",
                        Expr::Ge(_, _) => "oge",
                        _ => unreachable!(),
                    };
                    writeln!(
                        self.out,
                        "  %{} = fcmp {} {} {}, {}",
                        tmp,
                        pred,
                        llvm_ty(&tl, self.structs),
                        vl,
                        vr
                    )
                    .unwrap();
                } else {
                    let pred = match e {
                        Expr::Eq(_, _) => "eq",
                        Expr::Ne(_, _) => "ne",
                        Expr::Lt(_, _) => {
                            if int_ty_signed(&tl) {
                                "slt"
                            } else {
                                "ult"
                            }
                        }
                        Expr::Le(_, _) => {
                            if int_ty_signed(&tl) {
                                "sle"
                            } else {
                                "ule"
                            }
                        }
                        Expr::Gt(_, _) => {
                            if int_ty_signed(&tl) {
                                "sgt"
                            } else {
                                "ugt"
                            }
                        }
                        Expr::Ge(_, _) => {
                            if int_ty_signed(&tl) {
                                "sge"
                            } else {
                                "uge"
                            }
                        }
                        _ => unreachable!(),
                    };
                    writeln!(
                        self.out,
                        "  %{} = icmp {} {} {}, {}",
                        tmp,
                        pred,
                        llvm_ty(&tl, self.structs),
                        vl,
                        vr
                    )
                    .unwrap();
                }
                (Ty::Bool, format!("%{tmp}"))
            }
            Expr::GenericCall {
                name,
                ty_args,
                args,
            } => {
                if name == ARC_NEW {
                    let [elem_ty] = ty_args.as_slice() else {
                        unreachable!("typechecked arc_new type args")
                    };
                    let (value_ty, value) = self.emit_expr(&args[0], locals, Some(elem_ty));
                    debug_assert!(types_match(&value_ty, elem_ty));
                    return (
                        Ty::Arc(Box::new(elem_ty.clone())),
                        self.emit_arc_new_value(elem_ty, &value),
                    );
                }
                if name == MUTEX_NEW {
                    let [elem_ty] = ty_args.as_slice() else {
                        unreachable!("typechecked mutex_new type args")
                    };
                    let (value_ty, value) = self.emit_expr(&args[0], locals, Some(elem_ty));
                    debug_assert!(types_match(&value_ty, elem_ty));
                    return (
                        Ty::Mutex(Box::new(elem_ty.clone())),
                        self.emit_mutex_new_value(elem_ty, &value),
                    );
                }
                if name == LIST_NEW {
                    let [elem_ty] = ty_args.as_slice() else {
                        unreachable!("typechecked list_new type args")
                    };
                    debug_assert!(args.is_empty());
                    return (
                        Ty::List(Box::new(elem_ty.clone())),
                        self.emit_list_alloc(elem_ty, "0"),
                    );
                }
                if name == LIST_WITH_CAPACITY {
                    let [elem_ty] = ty_args.as_slice() else {
                        unreachable!("typechecked list_with_capacity type args")
                    };
                    let (_, cap32) = self.emit_expr(&args[0], locals, Some(&Ty::I32));
                    let cap64 = self.fresh();
                    writeln!(self.out, "  %{} = sext i32 {} to i64", cap64, cap32).unwrap();
                    let cap_non_negative = self.fresh();
                    let ok_lbl = self.fresh_label("list.capacity.ok");
                    let abort_lbl = self.fresh_label("list.capacity.abort");
                    writeln!(
                        self.out,
                        "  %{} = icmp sge i64 %{}, 0",
                        cap_non_negative, cap64
                    )
                    .unwrap();
                    writeln!(
                        self.out,
                        "  br i1 %{}, label %{}, label %{}",
                        cap_non_negative, ok_lbl, abort_lbl
                    )
                    .unwrap();
                    writeln!(self.out, "{}:", abort_lbl).unwrap();
                    writeln!(self.out, "  call void @abort()").unwrap();
                    writeln!(self.out, "  unreachable").unwrap();
                    writeln!(self.out, "{}:", ok_lbl).unwrap();
                    return (
                        Ty::List(Box::new(elem_ty.clone())),
                        self.emit_list_alloc(elem_ty, &format!("%{cap64}")),
                    );
                }
                unreachable!("typechecked generic call")
            }
            Expr::Closure {
                is_move,
                params,
                ret,
                body,
            } => self.emit_closure_literal(*is_move, params, ret.as_ref(), body, locals, hint),
            Expr::Spawn { target } => {
                let target_expr = Expr::Ident(target.clone());
                self.emit_thread_spawn(&target_expr, locals)
            }
            Expr::SpawnClosure { closure } => self.emit_thread_spawn(closure, locals),
            Expr::CallExpr { callee, args } => {
                let (callee_ty, callee_val) = self.emit_expr(callee, locals, None);
                let Ty::Fn(params, ret) = callee_ty else {
                    unreachable!("typechecked function value call")
                };
                self.emit_fn_value_call(&callee_val, &params, &ret, args, locals)
            }
            Expr::Call { name, args } => {
                if name == OPTION_SOME {
                    return self.emit_option_some_value(args, locals, hint);
                }
                if name == RESULT_OK {
                    return self.emit_result_value(0, args, locals, hint);
                }
                if name == RESULT_ERR {
                    return self.emit_result_value(1, args, locals, hint);
                }
                if let Some((Ty::Fn(params, ret), ptr)) = locals.get(name) {
                    let params = params.clone();
                    let ret = ret.as_ref().clone();
                    let ptr = ptr.clone();
                    let callee = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr {}",
                        callee,
                        fn_value_ll_ty(),
                        ptr
                    )
                    .unwrap();
                    return self.emit_fn_value_call(
                        &format!("%{callee}"),
                        &params,
                        &ret,
                        args,
                        locals,
                    );
                }
                if name == DROP_METHOD {
                    return self.emit_language_drop(args, locals);
                }
                if name == ATOMIC_BOOL {
                    let (_, value) = self.emit_expr(&args[0], locals, Some(&Ty::Bool));
                    return (Ty::AtomicBool, value);
                }
                if let Some((atomic_ty, value_ty)) = atomic_int_constructor_tys(name) {
                    let (_, value) = self.emit_expr(&args[0], locals, Some(&value_ty));
                    return (atomic_ty, value);
                }
                if name == ATOMIC_PTR {
                    let ptr_hint = match hint {
                        Some(Ty::AtomicPtr(pointee)) => Some(Ty::Ptr(pointee.clone())),
                        _ => None,
                    };
                    let (ptr_ty, value) = self.emit_expr(&args[0], locals, ptr_hint.as_ref());
                    let Ty::Ptr(pointee) = ptr_ty else {
                        unreachable!("typechecked atomic_ptr argument")
                    };
                    return (Ty::AtomicPtr(pointee), value);
                }
                if name == ARC_NEW {
                    let elem_hint = match hint {
                        Some(Ty::Arc(elem_ty)) => Some(elem_ty.as_ref().clone()),
                        _ => None,
                    };
                    let (value_ty, value) = self.emit_expr(&args[0], locals, elem_hint.as_ref());
                    let elem_ty = elem_hint.unwrap_or_else(|| value_ty.clone());
                    debug_assert!(types_match(&value_ty, &elem_ty));
                    return (
                        Ty::Arc(Box::new(elem_ty.clone())),
                        self.emit_arc_new_value(&elem_ty, &value),
                    );
                }
                if name == MUTEX_NEW {
                    let elem_hint = match hint {
                        Some(Ty::Mutex(elem_ty)) => Some(elem_ty.as_ref().clone()),
                        _ => None,
                    };
                    let (value_ty, value) = self.emit_expr(&args[0], locals, elem_hint.as_ref());
                    let elem_ty = elem_hint.unwrap_or_else(|| value_ty.clone());
                    debug_assert!(types_match(&value_ty, &elem_ty));
                    return (
                        Ty::Mutex(Box::new(elem_ty.clone())),
                        self.emit_mutex_new_value(&elem_ty, &value),
                    );
                }
                if name == ATOMIC_FENCE {
                    let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[0]));
                    writeln!(self.out, "  fence {}", ordering).unwrap();
                    return (Ty::Unit, String::new());
                }
                if name == JOIN {
                    return self.emit_thread_join(args, locals);
                }
                if let Some(result) = self.emit_crypto_builtin_call(name, args, locals) {
                    return result;
                }
                if let Some(result) = self.emit_qir_builtin_call(name, args, locals) {
                    return result;
                }
                if name == PRINTLN {
                    let (at, av) = self.emit_expr(&args[0], locals, None);
                    self.emit_print_value(&at, &av, true);
                    return (Ty::Unit, String::new());
                }
                if name == LEN {
                    let (at, av) = self.emit_expr(&args[0], locals, None);
                    if let Ty::Array(_, n) = at {
                        return (Ty::I32, format!("{n}"));
                    }
                    if matches!(at, Ty::HeapVector(_)) {
                        let len64 = self.heap_vector_load_i64_field(&av, 1);
                        let out = self.fresh();
                        writeln!(self.out, "  %{} = trunc i64 {} to i32", out, len64).unwrap();
                        return (Ty::I32, format!("%{out}"));
                    }
                    unreachable!("typechecked len")
                }
                if name == SIN || name == COS {
                    let (_, av) = self.emit_expr(&args[0], locals, Some(&Ty::F64));
                    return (Ty::F64, self.emit_f64_unary_lib_call(name, &av));
                }
                if name == COMPLEX_NEW {
                    let (_, re) = self.emit_expr(&args[0], locals, Some(&Ty::F64));
                    let (_, im) = self.emit_expr(&args[1], locals, Some(&Ty::F64));
                    return (
                        crate::nia_std::complex_ty(),
                        self.emit_complex_construct(&re, &im),
                    );
                }
                if name == COMPLEX_ADD
                    || name == COMPLEX_SUB
                    || name == COMPLEX_MUL
                    || name == COMPLEX_DIV
                {
                    let complex_ty = crate::nia_std::complex_ty();
                    let (_, left) = self.emit_expr(&args[0], locals, Some(&complex_ty));
                    let (_, right) = self.emit_expr(&args[1], locals, Some(&complex_ty));
                    let value = if name == COMPLEX_ADD {
                        self.emit_complex_add_sub(&left, &right, true)
                    } else if name == COMPLEX_SUB {
                        self.emit_complex_add_sub(&left, &right, false)
                    } else if name == COMPLEX_MUL {
                        self.emit_complex_mul_value(&left, &right)
                    } else {
                        self.emit_complex_div_value(&left, &right)
                    };
                    return (complex_ty, value);
                }
                if name == COMPLEX_SCALE {
                    let complex_ty = crate::nia_std::complex_ty();
                    let (_, value) = self.emit_expr(&args[0], locals, Some(&complex_ty));
                    let (_, scale) = self.emit_expr(&args[1], locals, Some(&Ty::F64));
                    return (complex_ty, self.emit_complex_scale_value(&value, &scale));
                }
                if name == CIS {
                    let (_, theta) = self.emit_expr(&args[0], locals, Some(&Ty::F64));
                    let re = self.emit_f64_unary_lib_call(COS, &theta);
                    let im = self.emit_f64_unary_lib_call(SIN, &theta);
                    return (
                        crate::nia_std::complex_ty(),
                        self.emit_complex_construct(&re, &im),
                    );
                }
                if name == MATRIX_NEW {
                    return self.emit_matrix_new(args, locals);
                }
                if name == MATRIX_GET {
                    return self.emit_matrix_get(args, locals);
                }
                if name == MATRIX_SET {
                    return self.emit_matrix_set(args, locals);
                }
                if name == MATRIX_ROWS || name == MATRIX_COLS || name == MATRIX_LEN {
                    return self.emit_matrix_info(name, args, locals);
                }
                if name == MATRIX_CLONE {
                    return self.emit_matrix_clone(args, locals);
                }
                if name == MATRIX_DROP {
                    return self.emit_matrix_drop(args, locals);
                }
                if name == VECTOR_GET {
                    return self.emit_heap_vector_get(args, locals);
                }
                if name == VECTOR_SET {
                    return self.emit_heap_vector_set(args, locals);
                }
                if name == VECTOR_LEN {
                    return self.emit_heap_vector_info(name, args, locals);
                }
                if name == VECTOR_CLONE {
                    return self.emit_heap_vector_clone(args, locals);
                }
                if name == VECTOR_DROP {
                    return self.emit_heap_vector_drop(args, locals);
                }
                if name == OUTER {
                    return self.emit_outer(args, locals);
                }
                if name == ALLOC {
                    let (at, av) = self.emit_expr(&args[0], locals, None);
                    let sz = self.emit_sizeof_i64(&at);
                    let raw = self.fresh();
                    writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", raw, sz).unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr %{}",
                        llvm_ty(&at, self.structs),
                        av,
                        raw
                    )
                    .unwrap();
                    return (Ty::Ptr(Box::new(at)), format!("%{raw}"));
                }
                if name == DEALLOC {
                    let (_, pv) = self.emit_expr(&args[0], locals, None);
                    writeln!(self.out, "  call void @free(ptr {})", pv).unwrap();
                    return (Ty::Unit, String::new());
                }
                if name == REALLOC {
                    let (pt, pv) = self.emit_expr(&args[0], locals, None);
                    let Ty::Ptr(pointee) = pt else {
                        unreachable!("typechecked")
                    };
                    let (vt, vv) = self.emit_expr(&args[1], locals, Some(&pointee));
                    debug_assert!(types_match(&vt, &pointee));
                    let sz = self.emit_sizeof_i64(&pointee);
                    let raw = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = call ptr @realloc(ptr {}, i64 {})",
                        raw, pv, sz
                    )
                    .unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr %{}",
                        llvm_ty(&pointee, self.structs),
                        vv,
                        raw
                    )
                    .unwrap();
                    return (Ty::Ptr(pointee), format!("%{raw}"));
                }
                if let Some(sdef) = self.structs.iter().find(|s| s.name == *name && s.is_tuple) {
                    let llvm_st = format!("%struct.{}", sanitize(name));
                    let mut agg = "poison".to_string();
                    for (i, (_, fty)) in sdef.fields.iter().enumerate() {
                        let (at, av) = self.emit_expr(&args[i], locals, Some(fty));
                        debug_assert!(types_match(&at, fty));
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = insertvalue {} {}, {} {}, {}",
                            tmp,
                            llvm_st,
                            agg,
                            llvm_ty(&at, self.structs),
                            av,
                            i
                        )
                        .unwrap();
                        agg = format!("%{tmp}");
                    }
                    let sname = name.clone();
                    let tmp = self.fresh();
                    writeln!(self.out, "  %{} = alloca {}", tmp, llvm_st).unwrap();
                    writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, tmp).unwrap();
                    let loadt = self.fresh();
                    writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, tmp).unwrap();
                    return (Ty::Struct(sname), format!("%{loadt}"));
                }
                if let Some((enum_name, variant)) = split_variant_path(name) {
                    if self
                        .enum_variant_def(enum_name, variant)
                        .is_some_and(|v| matches!(&v.fields, EnumVariantFields::Tuple(_)))
                    {
                        return self.emit_enum_tuple_variant(enum_name, variant, args, locals);
                    }
                }
                let sig = self.fn_sigs.get(name).expect("unknown fn");
                let mut arg_strs = Vec::new();
                for (a, pt) in args.iter().zip(&sig.params) {
                    let (at, av) = self.emit_expr(a, locals, Some(pt));
                    debug_assert!(types_match(&at, pt));
                    arg_strs.push(format!("{} {}", llvm_ty(pt, self.structs), av));
                }
                match &sig.ret {
                    Some(ret_ty) => {
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = call {} @{}({})",
                            tmp,
                            llvm_ty(ret_ty, self.structs),
                            sanitize(name),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (ret_ty.clone(), format!("%{tmp}"))
                    }
                    None => {
                        writeln!(
                            self.out,
                            "  call void @{}({})",
                            sanitize(name),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (Ty::Unit, String::new())
                    }
                }
            }
            Expr::MethodCall {
                receiver,
                name,
                args,
            } => {
                if is_atomic_method_name(name) {
                    let recv_ty = self.expr_codegen_ty(receiver, locals);
                    if matches!(recv_ty, Ty::AtomicPtr(_)) {
                        return self.emit_atomic_ptr_method(receiver, name, args, locals);
                    }
                    if atomic_int_value_ty(&recv_ty).is_some() {
                        return self.emit_atomic_int_method(receiver, name, args, locals);
                    }
                    return self.emit_atomic_bool_method(receiver, name, args, locals);
                }
                if name == TO_MATRIX {
                    debug_assert!(args.is_empty());
                    let (recv_ty, recv_val) = self.emit_expr(receiver, locals, None);
                    return self.emit_matrix_from_array(recv_ty, &recv_val);
                }
                if name == TO_ARRAY {
                    debug_assert!(args.is_empty());
                    let receiver_hint = match hint {
                        Some(Ty::Array(row_ty, rows))
                            if matches!(row_ty.as_ref(), Ty::Array(_, _)) =>
                        {
                            let Ty::Array(cell_ty, cols) = row_ty.as_ref() else {
                                unreachable!("guarded above")
                            };
                            Some(Ty::Matrix(cell_ty.clone(), Some((*rows, *cols))))
                        }
                        Some(Ty::Array(elem_ty, n)) => Some(Ty::AnonVector(elem_ty.clone(), *n)),
                        _ => None,
                    };
                    let (recv_ty, recv_val) =
                        self.emit_expr(receiver, locals, receiver_hint.as_ref());
                    return match recv_ty {
                        Ty::Matrix(elem_ty, Some((rows, cols))) => {
                            self.emit_matrix_to_array(&elem_ty, rows, cols, &recv_val)
                        }
                        Ty::AnonVector(elem_ty, n) => (Ty::Array(elem_ty, n), recv_val),
                        _ => unreachable!("typechecked to_array receiver"),
                    };
                }
                if name == TO_VEC {
                    debug_assert!(args.is_empty());
                    let receiver_hint = match hint {
                        Some(Ty::AnonVector(elem_ty, n)) => Some(Ty::Array(elem_ty.clone(), *n)),
                        _ => None,
                    };
                    let (recv_ty, recv_val) =
                        self.emit_expr(receiver, locals, receiver_hint.as_ref());
                    let Ty::Array(elem_ty, n) = recv_ty else {
                        unreachable!("typechecked to_vec receiver")
                    };
                    return (Ty::AnonVector(elem_ty, n), recv_val);
                }
                let (recv_ty, recv_val) = self.emit_expr(receiver, locals, None);
                if name == CLONE_METHOD {
                    debug_assert!(args.is_empty());
                    debug_assert!(supports_clone_method_ty(
                        &recv_ty,
                        self.structs,
                        self.enums,
                        self.vectors
                    ));
                    let cloned = self.emit_clone_value(&recv_ty, &recv_val);
                    return (recv_ty, cloned);
                }
                if let Ty::Mutex(elem_ty) = &recv_ty {
                    debug_assert!(args.is_empty());
                    if name == MUTEX_LOCK {
                        let guard = self.emit_mutex_lock_value(elem_ty, &recv_val);
                        return (Ty::MutexGuard(elem_ty.clone()), guard);
                    }
                    if name == MUTEX_TRY_LOCK {
                        let value = self.emit_mutex_try_lock_value(elem_ty, &recv_val);
                        return (Ty::Option(Box::new(Ty::MutexGuard(elem_ty.clone()))), value);
                    }
                }
                if let Ty::List(elem_ty) = &recv_ty {
                    let elem_ty = elem_ty.as_ref();
                    if name == LIST_LEN || name == LIST_CAPACITY {
                        debug_assert!(args.is_empty());
                        let field = if name == LIST_LEN { 1 } else { 2 };
                        let value64 = self.list_load_i64_field(&recv_val, field);
                        let out = self.fresh();
                        writeln!(self.out, "  %{} = trunc i64 {} to i32", out, value64).unwrap();
                        return (Ty::I32, format!("%{out}"));
                    }
                    if name == LIST_PUSH {
                        debug_assert!(args.len() == 1);
                        let (value_ty, value) = self.emit_expr(&args[0], locals, Some(elem_ty));
                        debug_assert!(types_match(&value_ty, elem_ty));
                        self.emit_list_push_value(&recv_val, elem_ty, &value);
                        return (Ty::Unit, String::new());
                    }
                    if name == LIST_GET {
                        debug_assert!(args.len() == 1);
                        let (_, idx) = self.emit_expr(&args[0], locals, Some(&Ty::I32));
                        let cell = self.list_cell_ptr(&recv_val, &idx, elem_ty);
                        let out = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = load {}, ptr {}",
                            out,
                            llvm_ty(elem_ty, self.structs),
                            cell
                        )
                        .unwrap();
                        return (elem_ty.clone(), format!("%{out}"));
                    }
                }
                if name == "det" {
                    if let Ty::Matrix(elem_ty, _) = method_receiver_owner_ty(&recv_ty) {
                        debug_assert!(args.is_empty());
                        let matrix = match &recv_ty {
                            Ty::Matrix(_, _) => recv_val,
                            Ty::Ptr(_) => {
                                let tmp = self.fresh();
                                writeln!(self.out, "  %{} = load ptr, ptr {}", tmp, recv_val)
                                    .unwrap();
                                format!("%{tmp}")
                            }
                            _ => unreachable!("typechecked Matrix.det receiver"),
                        };
                        return self.emit_matrix_det_value(elem_ty.as_ref().clone(), &matrix);
                    }
                }
                let symbol = method_symbol(method_receiver_owner_ty(&recv_ty), name);
                let (params, ret) = {
                    let sig = self.fn_sigs.get(&symbol).expect("typechecked method");
                    (sig.params.clone(), sig.ret.clone())
                };
                debug_assert!(!params.is_empty());
                let self_param = &params[0];
                let (self_arg_ty, self_arg_val) = if types_match(&recv_ty, self_param) {
                    (self_param.clone(), recv_val)
                } else if let Ty::Ptr(pointee) = self_param {
                    debug_assert!(types_match(&recv_ty, pointee));
                    let slot = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = alloca {}",
                        slot,
                        llvm_ty(pointee, self.structs)
                    )
                    .unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr %{}",
                        llvm_ty(pointee, self.structs),
                        recv_val,
                        slot
                    )
                    .unwrap();
                    (self_param.clone(), format!("%{slot}"))
                } else if let Ty::Ptr(pointee) = &recv_ty {
                    debug_assert!(types_match(pointee, self_param));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr {}",
                        tmp,
                        llvm_ty(self_param, self.structs),
                        recv_val
                    )
                    .unwrap();
                    (self_param.clone(), format!("%{tmp}"))
                } else {
                    unreachable!("typechecked method receiver")
                };
                let mut arg_strs = vec![format!(
                    "{} {}",
                    llvm_ty(&self_arg_ty, self.structs),
                    self_arg_val
                )];
                for (a, pt) in args.iter().zip(params.iter().skip(1)) {
                    let (at, av) = self.emit_expr(a, locals, Some(pt));
                    debug_assert!(types_match(&at, pt));
                    arg_strs.push(format!("{} {}", llvm_ty(pt, self.structs), av));
                }
                match ret {
                    Some(ret_ty) => {
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = call {} @{}({})",
                            tmp,
                            llvm_ty(&ret_ty, self.structs),
                            sanitize(&symbol),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (ret_ty, format!("%{tmp}"))
                    }
                    None => {
                        writeln!(
                            self.out,
                            "  call void @{}({})",
                            sanitize(&symbol),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (Ty::Unit, String::new())
                    }
                }
            }
            Expr::StructLit { name, fields } => {
                let Some(sdef) = self.structs.iter().find(|s| s.name == *name) else {
                    if let Some((enum_name, variant)) = split_variant_path(name) {
                        if self
                            .enum_variant_def(enum_name, variant)
                            .is_some_and(|v| matches!(&v.fields, EnumVariantFields::Struct(_)))
                        {
                            return self
                                .emit_enum_struct_variant(enum_name, variant, fields, locals);
                        }
                    }
                    unreachable!("typechecked struct literal");
                };
                let llvm_st = format!("%struct.{}", sanitize(name));
                let mut agg = "poison".to_string();
                for (i, (fname, _)) in sdef.fields.iter().enumerate() {
                    let (_, fe) = fields.iter().find(|(n, _)| n == fname).unwrap();
                    let fty = sdef.fields[i].1.clone();
                    let (ft, fv) = self.emit_expr(fe, locals, Some(&fty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        llvm_st,
                        agg,
                        llvm_ty(&ft, self.structs),
                        fv,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let sname = name.clone();
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = alloca {}", tmp, llvm_st).unwrap();
                writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, tmp).unwrap();
                let loadt = self.fresh();
                writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, tmp).unwrap();
                (Ty::Struct(sname), format!("%{loadt}"))
            }
            Expr::VectorLit { name, fields } => {
                let vdef = self.vectors.iter().find(|s| s.name == *name).unwrap();
                let llvm_st = format!("%struct.{}", sanitize(name));
                let mut agg = "poison".to_string();
                for (i, fname) in vdef.fields.iter().enumerate() {
                    let (_, fe) = fields.iter().find(|(n, _)| n == fname).unwrap();
                    // !!!!!type of fields is the same as the vector type!!!!!
                    let (ft, fv) = self.emit_expr(fe, locals, Some(&vdef.ty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        llvm_st,
                        agg,
                        llvm_ty(&ft, self.structs),
                        fv,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let sname = name.clone();
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = alloca {}", tmp, llvm_st).unwrap();
                writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, tmp).unwrap();
                let loadt = self.fresh();
                writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, tmp).unwrap();
                (Ty::Struct(sname), format!("%{loadt}"))
            }
            Expr::AnonVectorLit(elems) => self.emit_anon_vector_lit(elems, locals, hint),
            Expr::EnumVariant { enum_name, variant } => {
                let tag = self
                    .enum_tag(enum_name, variant)
                    .expect("typechecked enum variant");
                let enum_ll = format!("%enum.{}", sanitize(enum_name));
                let idx = self
                    .enum_variant_index(enum_name, variant)
                    .expect("typechecked enum idx");
                let vdef = self
                    .enum_variant_def(enum_name, variant)
                    .expect("typechecked enum variant")
                    .clone();
                let payload_ll = enum_variant_payload_ty(&vdef, self.structs);
                let with_tag = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} poison, i32 {}, 0",
                    with_tag, enum_ll, tag
                )
                .unwrap();
                let with_payload = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} %{}, {} zeroinitializer, {}",
                    with_payload,
                    enum_ll,
                    with_tag,
                    payload_ll,
                    idx + 1
                )
                .unwrap();
                (Ty::Enum(enum_name.clone()), format!("%{with_payload}"))
            }
            Expr::EnumTuple {
                enum_name,
                variant,
                args,
            } => {
                let tag = self
                    .enum_tag(enum_name, variant)
                    .expect("typechecked enum variant");
                let idx = self
                    .enum_variant_index(enum_name, variant)
                    .expect("typechecked enum idx");
                let vdef = self
                    .enum_variant_def(enum_name, variant)
                    .expect("typechecked enum variant")
                    .clone();
                let EnumVariantFields::Tuple(ts) = vdef.fields else {
                    unreachable!("typechecked tuple variant")
                };
                let enum_ll = format!("%enum.{}", sanitize(enum_name));
                let with_tag = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} poison, i32 {}, 0",
                    with_tag, enum_ll, tag
                )
                .unwrap();
                let payload_val = if ts.len() == 1 {
                    let (_, v) = self.emit_expr(&args[0], locals, Some(&ts[0]));
                    (llvm_ty(&ts[0], self.structs), v)
                } else {
                    let payload_ll = if ts.len() == 1 {
                        llvm_ty(&ts[0], self.structs)
                    } else {
                        let inner = ts
                            .iter()
                            .map(|t| llvm_ty(t, self.structs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ {inner} }}")
                    };
                    let mut agg = "poison".to_string();
                    for (i, (a, t)) in args.iter().zip(ts.iter()).enumerate() {
                        let (_, av) = self.emit_expr(a, locals, Some(t));
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = insertvalue {} {}, {} {}, {}",
                            tmp,
                            payload_ll,
                            agg,
                            llvm_ty(t, self.structs),
                            av,
                            i
                        )
                        .unwrap();
                        agg = format!("%{tmp}");
                    }
                    (payload_ll, agg)
                };
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} %{}, {} {}, {}",
                    out,
                    enum_ll,
                    with_tag,
                    payload_val.0,
                    payload_val.1,
                    idx + 1
                )
                .unwrap();
                (Ty::Enum(enum_name.clone()), format!("%{out}"))
            }
            Expr::EnumStruct {
                enum_name,
                variant,
                fields,
            } => {
                let tag = self
                    .enum_tag(enum_name, variant)
                    .expect("typechecked enum variant");
                let idx = self
                    .enum_variant_index(enum_name, variant)
                    .expect("typechecked enum idx");
                let vdef = self
                    .enum_variant_def(enum_name, variant)
                    .expect("typechecked enum variant")
                    .clone();
                let EnumVariantFields::Struct(fs) = vdef.fields else {
                    unreachable!("typechecked struct variant")
                };
                let enum_ll = format!("%enum.{}", sanitize(enum_name));
                let with_tag = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} poison, i32 {}, 0",
                    with_tag, enum_ll, tag
                )
                .unwrap();
                let payload_ll = {
                    let inner = fs
                        .iter()
                        .map(|(_, t)| llvm_ty(t, self.structs))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {inner} }}")
                };
                let mut agg = "poison".to_string();
                for (i, (fname, fty)) in fs.iter().enumerate() {
                    let (_, fe) = fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .expect("typechecked");
                    let (_, fv) = self.emit_expr(fe, locals, Some(fty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        payload_ll,
                        agg,
                        llvm_ty(fty, self.structs),
                        fv,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} %{}, {} {}, {}",
                    out,
                    enum_ll,
                    with_tag,
                    payload_ll,
                    agg,
                    idx + 1
                )
                .unwrap();
                (Ty::Enum(enum_name.clone()), format!("%{out}"))
            }
            Expr::Match { scrutinee, arms } => {
                let (st, sv) = self.emit_expr(scrutinee, locals, None);
                if let Ty::Option(elem_ty) = &st {
                    return self.emit_builtin_sum_match(
                        &sv,
                        &[
                            (OPTION_NONE, None, 0),
                            (OPTION_SOME, Some(elem_ty.as_ref().clone()), 1),
                        ],
                        arms,
                        locals,
                        hint,
                    );
                }
                if let Ty::ResultType(ok_ty, err_ty) = &st {
                    return self.emit_builtin_sum_match(
                        &sv,
                        &[
                            (RESULT_OK, Some(ok_ty.as_ref().clone()), 0),
                            (RESULT_ERR, Some(err_ty.as_ref().clone()), 1),
                        ],
                        arms,
                        locals,
                        hint,
                    );
                }
                let Ty::Enum(enum_name) = st else {
                    unreachable!("typechecked match enum")
                };
                let tag_tmp = self.fresh();
                let enum_ll = format!("%enum.{}", sanitize(&enum_name));
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, 0",
                    tag_tmp, enum_ll, sv
                )
                .unwrap();
                let cont_lbl = self.fresh_label("match.cont");
                let default_lbl = self.fresh_label("match.default");
                let mut arm_labels = Vec::new();
                for _ in arms {
                    arm_labels.push(self.fresh_label("match.arm"));
                }
                writeln!(
                    self.out,
                    "  switch i32 %{}, label %{} [",
                    tag_tmp, default_lbl
                )
                .unwrap();
                for ((pat, _), lbl) in arms.iter().zip(&arm_labels) {
                    let variant = match pat {
                        MatchPattern::Unit { variant, .. } => variant,
                        MatchPattern::Tuple { variant, .. } => variant,
                        MatchPattern::Struct { variant, .. } => variant,
                    };
                    let tag = self
                        .enum_tag(&enum_name, variant)
                        .expect("typechecked enum tag");
                    writeln!(self.out, "    i32 {}, label %{}", tag, lbl).unwrap();
                }
                writeln!(self.out, "  ]").unwrap();
                writeln!(self.out, "{}:", default_lbl).unwrap();
                writeln!(self.out, "  unreachable").unwrap();

                let mut arm_vals: Vec<(String, String)> = Vec::new();
                let mut out_ty: Option<Ty> = None;
                for ((pat, arm_expr), lbl) in arms.iter().zip(&arm_labels) {
                    writeln!(self.out, "{}:", lbl).unwrap();
                    let mut arm_locals = locals.clone();
                    let variant = match pat {
                        MatchPattern::Unit { variant, .. } => variant,
                        MatchPattern::Tuple { variant, .. } => variant,
                        MatchPattern::Struct { variant, .. } => variant,
                    };
                    let vidx = self
                        .enum_variant_index(&enum_name, variant)
                        .expect("typechecked enum idx");
                    let vdef = self
                        .enum_variant_def(&enum_name, variant)
                        .expect("typechecked enum variant")
                        .clone();
                    match (pat, vdef.fields) {
                        (MatchPattern::Unit { .. }, EnumVariantFields::Unit) => {}
                        (MatchPattern::Tuple { bindings, .. }, EnumVariantFields::Tuple(ts)) => {
                            let payload_ll = if ts.len() == 1 {
                                llvm_ty(&ts[0], self.structs)
                            } else {
                                let inner = ts
                                    .iter()
                                    .map(|t| llvm_ty(t, self.structs))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("{{ {inner} }}")
                            };
                            let pl = self.fresh();
                            writeln!(
                                self.out,
                                "  %{} = extractvalue {} {}, {}",
                                pl,
                                enum_ll,
                                sv,
                                vidx + 1
                            )
                            .unwrap();
                            if ts.len() == 1 {
                                let ptr = format!("%{}.addr", sanitize(&bindings[0]));
                                writeln!(
                                    self.out,
                                    "  {} = alloca {}",
                                    ptr,
                                    llvm_ty(&ts[0], self.structs)
                                )
                                .unwrap();
                                writeln!(
                                    self.out,
                                    "  store {} %{}, ptr {}",
                                    llvm_ty(&ts[0], self.structs),
                                    pl,
                                    ptr
                                )
                                .unwrap();
                                arm_locals.insert(bindings[0].clone(), (ts[0].clone(), ptr));
                            } else {
                                for (i, (b, t)) in bindings.iter().zip(ts.iter()).enumerate() {
                                    let ev = self.fresh();
                                    writeln!(
                                        self.out,
                                        "  %{} = extractvalue {} %{}, {}",
                                        ev, payload_ll, pl, i
                                    )
                                    .unwrap();
                                    let ptr = format!("%{}.addr", sanitize(b));
                                    writeln!(
                                        self.out,
                                        "  {} = alloca {}",
                                        ptr,
                                        llvm_ty(t, self.structs)
                                    )
                                    .unwrap();
                                    writeln!(
                                        self.out,
                                        "  store {} %{}, ptr {}",
                                        llvm_ty(t, self.structs),
                                        ev,
                                        ptr
                                    )
                                    .unwrap();
                                    arm_locals.insert(b.clone(), (t.clone(), ptr));
                                }
                            }
                        }
                        (MatchPattern::Struct { bindings, .. }, EnumVariantFields::Struct(fs)) => {
                            let payload_ll = {
                                let inner = fs
                                    .iter()
                                    .map(|(_, t)| llvm_ty(t, self.structs))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("{{ {inner} }}")
                            };
                            let pl = self.fresh();
                            writeln!(
                                self.out,
                                "  %{} = extractvalue {} {}, {}",
                                pl,
                                enum_ll,
                                sv,
                                vidx + 1
                            )
                            .unwrap();
                            for b in bindings {
                                let (i, (_, t)) = fs
                                    .iter()
                                    .enumerate()
                                    .find(|(_, (n, _))| n == b)
                                    .expect("typechecked field");
                                let ev = self.fresh();
                                writeln!(
                                    self.out,
                                    "  %{} = extractvalue {} %{}, {}",
                                    ev, payload_ll, pl, i
                                )
                                .unwrap();
                                let ptr = format!("%{}.addr", sanitize(b));
                                writeln!(
                                    self.out,
                                    "  {} = alloca {}",
                                    ptr,
                                    llvm_ty(t, self.structs)
                                )
                                .unwrap();
                                writeln!(
                                    self.out,
                                    "  store {} %{}, ptr {}",
                                    llvm_ty(t, self.structs),
                                    ev,
                                    ptr
                                )
                                .unwrap();
                                arm_locals.insert(b.clone(), (t.clone(), ptr));
                            }
                        }
                        _ => unreachable!("typechecked pattern kind"),
                    }
                    let arm_hint = hint.or(out_ty.as_ref());
                    let (at, av) = self.emit_expr(arm_expr, &arm_locals, arm_hint);
                    if !matches!(at, Ty::Unit) {
                        arm_vals.push((av.clone(), lbl.clone()));
                    }
                    if out_ty.is_none() {
                        out_ty = Some(at.clone());
                    }
                    writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
                }
                writeln!(self.out, "{}:", cont_lbl).unwrap();
                let out_ty = out_ty.expect("typechecked non-empty match");
                if matches!(out_ty, Ty::Unit) {
                    (Ty::Unit, String::new())
                } else {
                    let phi = self.fresh();
                    let ll = llvm_ty(&out_ty, self.structs);
                    let incoming = arm_vals
                        .iter()
                        .map(|(v, l)| format!("[ {}, %{} ]", v, l))
                        .collect::<Vec<_>>()
                        .join(", ");
                    writeln!(self.out, "  %{} = phi {} {}", phi, ll, incoming).unwrap();
                    (out_ty, format!("%{phi}"))
                }
            }
            Expr::Quant { body } => {
                let mut body_locals = locals.clone();
                let mut body_drop_flags = HashMap::new();
                let mut body_visible_order = Vec::new();
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        None,
                    );
                    debug_assert!(
                        !self.terminated,
                        "typecheck rejects terminating statements in quant expressions"
                    );
                }
                let result = if let Some(tail) = &body.tail {
                    self.emit_expr(tail, &body_locals, hint)
                } else {
                    (Ty::Unit, String::new())
                };
                self.emit_drop_scope_from(0, &body_visible_order, &body_locals, &body_drop_flags);
                result
            }
            Expr::Gpu { body } => {
                let mut body_locals = locals.clone();
                let mut body_drop_flags = HashMap::new();
                let mut body_visible_order = Vec::new();
                for st in &body.stmts {
                    self.emit_stmt(
                        st,
                        &mut body_locals,
                        &mut body_drop_flags,
                        &mut body_visible_order,
                        None,
                    );
                    debug_assert!(
                        !self.terminated,
                        "typecheck rejects terminating statements in gpu expressions"
                    );
                }
                let result = if let Some(tail) = &body.tail {
                    self.emit_expr(tail, &body_locals, hint)
                } else {
                    (Ty::Unit, String::new())
                };
                self.emit_drop_scope_from(0, &body_visible_order, &body_locals, &body_drop_flags);
                result
            }
            Expr::ArrayLit(elems) => {
                let (elem_ty, n) = match hint {
                    Some(Ty::Array(elem, n)) => (elem.as_ref().clone(), *n),
                    _ => {
                        let first = elems.first().expect("typechecked non-empty array");
                        let (t, _) = self.emit_expr(first, locals, None);
                        (t, elems.len())
                    }
                };
                let llvm_arr = format!("[{} x {}]", n, llvm_ty(&elem_ty, self.structs));
                let mut agg = "poison".to_string();
                for (i, e) in elems.iter().enumerate() {
                    let (et, ev) = self.emit_expr(e, locals, Some(&elem_ty));
                    debug_assert!(
                        types_match(&et, &elem_ty),
                        "array literal element type mismatch: got {et:?}, expected {elem_ty:?}"
                    );
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        llvm_arr,
                        agg,
                        llvm_ty(&et, self.structs),
                        ev,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = alloca {}", tmp, llvm_arr).unwrap();
                writeln!(self.out, "  store {} {}, ptr %{}", llvm_arr, agg, tmp).unwrap();
                let loadt = self.fresh();
                writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_arr, tmp).unwrap();
                (Ty::Array(Box::new(elem_ty), n), format!("%{loadt}"))
            }
            Expr::Field(obj, fname) => {
                let (bt, bv) = self.emit_expr(obj, locals, None);
                let (sname, aggregate_val) = match bt {
                    Ty::Struct(sname) | Ty::Vector(sname, _) => (sname, bv),
                    Ty::Ptr(inner) => match inner.as_ref() {
                        Ty::Struct(sname) | Ty::Vector(sname, _) => {
                            let tmp = self.fresh();
                            writeln!(
                                self.out,
                                "  %{} = load {}, ptr {}",
                                tmp,
                                llvm_ty(inner.as_ref(), self.structs),
                                bv
                            )
                            .unwrap();
                            (sname.clone(), format!("%{tmp}"))
                        }
                        _ => unreachable!("typechecked field base type"),
                    },
                    _ => unreachable!("typechecked field base type"),
                };
                let idx = self
                    .struct_idx(&sname, fname)
                    .or_else(|| self.vector_idx(&sname, fname))
                    .unwrap();
                let llvm_st = format!("%struct.{}", sanitize(&sname));
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    tmp, llvm_st, aggregate_val, idx
                )
                .unwrap();
                let fty = if let Some(sdef) = self.structs.iter().find(|s| s.name == sname) {
                    sdef.fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .unwrap()
                        .1
                        .clone()
                } else if let Some(vdef) = self.vectors.iter().find(|v| v.name == sname) {
                    assert!(vdef.fields.iter().any(|n| n == fname));
                    vdef.ty.clone()
                } else {
                    unreachable!("typechecked field base type")
                };
                (fty, format!("%{tmp}"))
            }
            Expr::Index(arr, idx) => {
                let full = Expr::Index(arr.clone(), idx.clone());
                if let Some((root, idxs)) = collect_array_index_chain(&full) {
                    let (elem_ty, gep_ptr) = self.emit_array_gep_chain(root, &idxs, locals);
                    let val = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr {}",
                        val,
                        llvm_ty(&elem_ty, self.structs),
                        gep_ptr
                    )
                    .unwrap();
                    (elem_ty, format!("%{val}"))
                } else {
                    let (at, av) = self.emit_expr(arr, locals, None);
                    let (it, iv) = self.emit_expr(idx, locals, Some(&Ty::I32));
                    debug_assert!(matches!(it, Ty::I32));
                    let Ty::Array(elem_ty, n) = at else {
                        unreachable!("typechecked")
                    };
                    let llvm_arr = format!("[{} x {}]", n, llvm_ty(&elem_ty, self.structs));
                    let ptr = self.fresh();
                    writeln!(self.out, "  %{} = alloca {}", ptr, llvm_arr).unwrap();
                    writeln!(self.out, "  store {} {}, ptr %{}", llvm_arr, av, ptr).unwrap();
                    let idx64 = self.fresh();
                    writeln!(self.out, "  %{} = sext i32 {} to i64", idx64, iv).unwrap();
                    let gep = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = getelementptr inbounds {}, ptr %{}, i64 0, i64 %{}",
                        gep, llvm_arr, ptr, idx64
                    )
                    .unwrap();
                    let val = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr %{}",
                        val,
                        llvm_ty(&elem_ty, self.structs),
                        gep
                    )
                    .unwrap();
                    ((*elem_ty).clone(), format!("%{val}"))
                }
            }
            Expr::AddrOf(inner) => {
                let Expr::Ident(name) = inner.as_ref() else {
                    unreachable!("typechecked")
                };
                let (ty, ptr) = locals.get(name).expect("checked var");
                (Ty::Ptr(Box::new(ty.clone())), ptr.clone())
            }
            Expr::Deref(inner) => {
                let (ti, v) = self.emit_expr(inner, locals, None);
                let (pointee, ptr) = match ti {
                    Ty::Ptr(pointee) => ((*pointee).clone(), v),
                    Ty::Arc(pointee) => {
                        let pointee_ty = (*pointee).clone();
                        let ptr = self.arc_value_ptr(&v, &pointee_ty);
                        (pointee_ty, ptr)
                    }
                    Ty::MutexGuard(pointee) => {
                        let pointee_ty = (*pointee).clone();
                        let ptr = self.mutex_value_ptr(&v, &pointee_ty);
                        (pointee_ty, ptr)
                    }
                    other => {
                        if let Some((target_ty, ptr)) = self.emit_custom_deref_ptr(&other, v) {
                            (target_ty, ptr)
                        } else {
                            unreachable!("typechecked")
                        }
                    }
                };
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr {}",
                    tmp,
                    llvm_ty(&pointee, self.structs),
                    ptr
                )
                .unwrap();
                (pointee, format!("%{tmp}"))
            }
        }
    }

    /// Follow `root` through `indices` with GEP; returns element type and `ptr` to the slot.
    fn emit_array_gep_chain(
        &mut self,
        root: &Expr,
        indices: &[&Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (mut cur_ty, mut llvm_ptr) = match root {
            Expr::Ident(name) => locals.get(name).expect("indexed assign var").clone(),
            Expr::Deref(inner) => {
                let (pt, pv) = self.emit_expr(inner, locals, None);
                let Ty::Ptr(pointee) = pt else {
                    unreachable!("typechecked indexed deref root")
                };
                ((*pointee).clone(), pv)
            }
            _ => unreachable!("typechecked index chain root"),
        };
        for idx_expr in indices {
            let Ty::Array(elem_ty, n) = &cur_ty else {
                unreachable!("typechecked index chain");
            };
            let (_, iv) = self.emit_expr(idx_expr, locals, Some(&Ty::I32));
            let llvm_arr = format!("[{} x {}]", n, llvm_ty(elem_ty, self.structs));
            let idx64 = self.fresh();
            writeln!(self.out, "  %{} = sext i32 {} to i64", idx64, iv).unwrap();
            let gep = self.fresh();
            writeln!(
                self.out,
                "  %{} = getelementptr inbounds {}, ptr {}, i64 0, i64 %{}",
                gep, llvm_arr, llvm_ptr, idx64
            )
            .unwrap();
            llvm_ptr = format!("%{gep}");
            cur_ty = *elem_ty.clone();
        }
        (cur_ty, llvm_ptr)
    }

    #[allow(dead_code)]
    fn emit_atomic_lvalue_ptr(
        &mut self,
        receiver: &Expr,
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        match receiver {
            Expr::Ident(name) => locals.get(name).expect("checked atomic local").clone(),
            Expr::Deref(inner) => {
                let (ptr_ty, ptr) = self.emit_expr(inner, locals, None);
                let (pointee, ptr) = match ptr_ty {
                    Ty::Ptr(pointee) => ((*pointee).clone(), ptr),
                    Ty::Arc(pointee) => {
                        let pointee_ty = (*pointee).clone();
                        let ptr = self.arc_value_ptr(&ptr, &pointee_ty);
                        (pointee_ty, ptr)
                    }
                    _ => unreachable!("typechecked atomic dereference receiver"),
                };
                (pointee, ptr)
            }
            _ => unreachable!("typechecked atomic lvalue receiver"),
        }
    }

    fn emit_atomic_bool_method(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (recv_ty, cell_ptr) = self.emit_atomic_lvalue_ptr(receiver, locals);
        debug_assert!(matches!(recv_ty, Ty::AtomicBool));
        let align = atomic_storage_align_bytes(&Ty::AtomicBool);
        match name {
            ATOMIC_LOAD_METHOD => {
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[0]));
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load atomic i1, ptr {} {}, align {}",
                    out, cell_ptr, ordering, align
                )
                .unwrap();
                (Ty::Bool, format!("%{out}"))
            }
            ATOMIC_STORE_METHOD => {
                let (_, value) = self.emit_expr(&args[0], locals, Some(&Ty::Bool));
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[1]));
                writeln!(
                    self.out,
                    "  store atomic i1 {}, ptr {} {}, align {}",
                    value, cell_ptr, ordering, align
                )
                .unwrap();
                (Ty::Unit, String::new())
            }
            ATOMIC_SWAP_METHOD
            | ATOMIC_FETCH_AND_METHOD
            | ATOMIC_FETCH_OR_METHOD
            | ATOMIC_FETCH_XOR_METHOD => {
                let (_, value) = self.emit_expr(&args[0], locals, Some(&Ty::Bool));
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[1]));
                let op = match name {
                    ATOMIC_SWAP_METHOD => "xchg",
                    ATOMIC_FETCH_AND_METHOD => "and",
                    ATOMIC_FETCH_OR_METHOD => "or",
                    ATOMIC_FETCH_XOR_METHOD => "xor",
                    _ => unreachable!("guarded atomic rmw method"),
                };
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = atomicrmw {} ptr {}, i1 {} {}",
                    out, op, cell_ptr, value, ordering
                )
                .unwrap();
                (Ty::Bool, format!("%{out}"))
            }
            ATOMIC_COMPARE_EXCHANGE_METHOD => {
                let (_, current) = self.emit_expr(&args[0], locals, Some(&Ty::Bool));
                let (_, new) = self.emit_expr(&args[1], locals, Some(&Ty::Bool));
                let success = llvm_atomic_ordering(atomic_ordering_from_expr(&args[2]));
                let failure = llvm_atomic_ordering(atomic_ordering_from_expr(&args[3]));
                let pair = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = cmpxchg ptr {}, i1 {}, i1 {} {} {}",
                    pair, cell_ptr, current, new, success, failure
                )
                .unwrap();
                let ok = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {{ i1, i1 }} %{}, 1",
                    ok, pair
                )
                .unwrap();
                (Ty::Bool, format!("%{ok}"))
            }
            _ => unreachable!("guarded atomic bool method"),
        }
    }

    fn emit_atomic_int_method(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (recv_ty, cell_ptr) = self.emit_atomic_lvalue_ptr(receiver, locals);
        let value_ty = atomic_int_value_ty(&recv_ty).expect("typechecked atomic integer receiver");
        let ll_ty = llvm_ty(&value_ty, self.structs);
        let align = atomic_storage_align_bytes(&recv_ty);
        match name {
            ATOMIC_LOAD_METHOD => {
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[0]));
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load atomic {}, ptr {} {}, align {}",
                    out, ll_ty, cell_ptr, ordering, align
                )
                .unwrap();
                (value_ty, format!("%{out}"))
            }
            ATOMIC_STORE_METHOD => {
                let (_, value) = self.emit_expr(&args[0], locals, Some(&value_ty));
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[1]));
                writeln!(
                    self.out,
                    "  store atomic {} {}, ptr {} {}, align {}",
                    ll_ty, value, cell_ptr, ordering, align
                )
                .unwrap();
                (Ty::Unit, String::new())
            }
            ATOMIC_SWAP_METHOD
            | ATOMIC_FETCH_ADD_METHOD
            | ATOMIC_FETCH_SUB_METHOD
            | ATOMIC_FETCH_AND_METHOD
            | ATOMIC_FETCH_OR_METHOD
            | ATOMIC_FETCH_XOR_METHOD => {
                let (_, value) = self.emit_expr(&args[0], locals, Some(&value_ty));
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[1]));
                let op = match name {
                    ATOMIC_SWAP_METHOD => "xchg",
                    ATOMIC_FETCH_ADD_METHOD => "add",
                    ATOMIC_FETCH_SUB_METHOD => "sub",
                    ATOMIC_FETCH_AND_METHOD => "and",
                    ATOMIC_FETCH_OR_METHOD => "or",
                    ATOMIC_FETCH_XOR_METHOD => "xor",
                    _ => unreachable!("guarded atomic integer rmw method"),
                };
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = atomicrmw {} ptr {}, {} {} {}",
                    out, op, cell_ptr, ll_ty, value, ordering
                )
                .unwrap();
                (value_ty, format!("%{out}"))
            }
            ATOMIC_COMPARE_EXCHANGE_METHOD => {
                let (_, current) = self.emit_expr(&args[0], locals, Some(&value_ty));
                let (_, new) = self.emit_expr(&args[1], locals, Some(&value_ty));
                let success = llvm_atomic_ordering(atomic_ordering_from_expr(&args[2]));
                let failure = llvm_atomic_ordering(atomic_ordering_from_expr(&args[3]));
                let pair = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = cmpxchg ptr {}, {} {}, {} {} {} {}",
                    pair, cell_ptr, ll_ty, current, ll_ty, new, success, failure
                )
                .unwrap();
                let ok = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {{ {}, i1 }} %{}, 1",
                    ok, ll_ty, pair
                )
                .unwrap();
                (Ty::Bool, format!("%{ok}"))
            }
            _ => unreachable!("guarded atomic integer method"),
        }
    }

    fn emit_atomic_ptr_method(
        &mut self,
        receiver: &Expr,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (recv_ty, cell_ptr) = self.emit_atomic_lvalue_ptr(receiver, locals);
        let Ty::AtomicPtr(pointee) = recv_ty else {
            unreachable!("typechecked atomic pointer receiver")
        };
        let ptr_ty = Ty::Ptr(pointee.clone());
        let align = atomic_storage_align_bytes(&ptr_ty);
        match name {
            ATOMIC_LOAD_METHOD => {
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[0]));
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load atomic ptr, ptr {} {}, align {}",
                    out, cell_ptr, ordering, align
                )
                .unwrap();
                (ptr_ty, format!("%{out}"))
            }
            ATOMIC_STORE_METHOD => {
                let (_, value) = self.emit_expr(&args[0], locals, Some(&ptr_ty));
                let ordering = llvm_atomic_ordering(atomic_ordering_from_expr(&args[1]));
                writeln!(
                    self.out,
                    "  store atomic ptr {}, ptr {} {}, align {}",
                    value, cell_ptr, ordering, align
                )
                .unwrap();
                (Ty::Unit, String::new())
            }
            ATOMIC_SWAP_METHOD => {
                let (_, value) = self.emit_expr(&args[0], locals, Some(&ptr_ty));
                let ordering = atomic_ordering_from_expr(&args[1]);
                let success = llvm_atomic_ordering(ordering);
                let failure = llvm_atomic_ordering(atomic_exchange_failure_ordering(ordering));
                let expected_addr = self.fresh();
                let initial = self.fresh();
                let loop_lbl = self.fresh_label("atomic.ptr.swap.loop");
                let done_lbl = self.fresh_label("atomic.ptr.swap.done");
                writeln!(self.out, "  %{} = alloca ptr", expected_addr).unwrap();
                writeln!(
                    self.out,
                    "  %{} = load atomic ptr, ptr {} {}, align {}",
                    initial, cell_ptr, failure, align
                )
                .unwrap();
                writeln!(self.out, "  store ptr %{}, ptr %{}", initial, expected_addr).unwrap();
                writeln!(self.out, "  br label %{}", loop_lbl).unwrap();
                writeln!(self.out, "{}:", loop_lbl).unwrap();
                let expected = self.fresh();
                let pair = self.fresh();
                let observed = self.fresh();
                let ok = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load ptr, ptr %{}",
                    expected, expected_addr
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = cmpxchg ptr {}, ptr %{}, ptr {} {} {}",
                    pair, cell_ptr, expected, value, success, failure
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {{ ptr, i1 }} %{}, 0",
                    observed, pair
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {{ ptr, i1 }} %{}, 1",
                    ok, pair
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store ptr %{}, ptr %{}",
                    observed, expected_addr
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  br i1 %{}, label %{}, label %{}",
                    ok, done_lbl, loop_lbl
                )
                .unwrap();
                writeln!(self.out, "{}:", done_lbl).unwrap();
                (ptr_ty, format!("%{observed}"))
            }
            ATOMIC_COMPARE_EXCHANGE_METHOD => {
                let (_, current) = self.emit_expr(&args[0], locals, Some(&ptr_ty));
                let (_, new) = self.emit_expr(&args[1], locals, Some(&ptr_ty));
                let success = llvm_atomic_ordering(atomic_ordering_from_expr(&args[2]));
                let failure = llvm_atomic_ordering(atomic_ordering_from_expr(&args[3]));
                let pair = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = cmpxchg ptr {}, ptr {}, ptr {} {} {}",
                    pair, cell_ptr, current, new, success, failure
                )
                .unwrap();
                let ok = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {{ ptr, i1 }} %{}, 1",
                    ok, pair
                )
                .unwrap();
                (Ty::Bool, format!("%{ok}"))
            }
            _ => unreachable!("guarded atomic pointer method"),
        }
    }

    fn emit_assign_ptr(
        &mut self,
        target: &Expr,
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        match target {
            Expr::Ident(name) => {
                let (ty, ptr) = locals.get(name).expect("typechecked assign ident");
                (ty.clone(), ptr.clone())
            }
            Expr::Deref(inner) => {
                let (pt, pv) = self.emit_expr(inner, locals, None);
                match pt {
                    Ty::Ptr(pointee) => ((*pointee).clone(), pv),
                    Ty::MutexGuard(pointee) => {
                        let pointee_ty = (*pointee).clone();
                        let ptr = self.mutex_value_ptr(&pv, &pointee_ty);
                        (pointee_ty, ptr)
                    }
                    other => {
                        if let Some((target_ty, ptr)) = self.emit_custom_deref_ptr(&other, pv) {
                            (target_ty, ptr)
                        } else {
                            unreachable!("typechecked assign deref")
                        }
                    }
                }
            }
            Expr::Index(arr, idx) => {
                let full = Expr::Index(arr.clone(), idx.clone());
                if let Some((root, idxs)) = collect_array_index_chain(&full) {
                    self.emit_array_gep_chain(root, &idxs, locals)
                } else {
                    unreachable!("typechecked indexed assign")
                }
            }
            _ => unreachable!("typechecked assign lvalue"),
        }
    }
}

/// Lightweight type compatibility used by codegen assertions.
fn types_match(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::I8, Ty::I8)
        | (Ty::U8, Ty::U8)
        | (Ty::I16, Ty::I16)
        | (Ty::U16, Ty::U16)
        | (Ty::I32, Ty::I32)
        | (Ty::U32, Ty::U32)
        | (Ty::I64, Ty::I64)
        | (Ty::U64, Ty::U64)
        | (Ty::I128, Ty::I128)
        | (Ty::Isize, Ty::Isize)
        | (Ty::Usize, Ty::Usize)
        | (Ty::U128, Ty::U128)
        | (Ty::F16, Ty::F16)
        | (Ty::F32, Ty::F32)
        | (Ty::F64, Ty::F64)
        | (Ty::Bool, Ty::Bool)
        | (Ty::AtomicBool, Ty::AtomicBool)
        | (Ty::AtomicI8, Ty::AtomicI8)
        | (Ty::AtomicU8, Ty::AtomicU8)
        | (Ty::AtomicI16, Ty::AtomicI16)
        | (Ty::AtomicU16, Ty::AtomicU16)
        | (Ty::AtomicI32, Ty::AtomicI32)
        | (Ty::AtomicU32, Ty::AtomicU32)
        | (Ty::AtomicI64, Ty::AtomicI64)
        | (Ty::AtomicU64, Ty::AtomicU64)
        | (Ty::AtomicI128, Ty::AtomicI128)
        | (Ty::AtomicU128, Ty::AtomicU128)
        | (Ty::AtomicIsize, Ty::AtomicIsize)
        | (Ty::AtomicUsize, Ty::AtomicUsize)
        | (Ty::String, Ty::String)
        | (Ty::Qubit, Ty::Qubit)
        | (Ty::Result, Ty::Result)
        | (Ty::Unit, Ty::Unit) => true,
        (Ty::Array(ax, an), Ty::Array(bx, bn)) => an == bn && types_match(ax, bx),
        (Ty::Struct(x), Ty::Struct(y)) => x == y,
        (Ty::Vector(xn, xt), Ty::Vector(yn, yt)) => xn == yn && types_match(xt, yt),
        (Ty::AnonVector(xt, xn), Ty::AnonVector(yt, yn)) => xn == yn && types_match(xt, yt),
        (Ty::HeapVector(xt), Ty::HeapVector(yt)) => types_match(xt, yt),
        (Ty::List(xt), Ty::List(yt)) => types_match(xt, yt),
        (Ty::Arc(xt), Ty::Arc(yt)) => types_match(xt, yt),
        (Ty::Mutex(xt), Ty::Mutex(yt)) => types_match(xt, yt),
        (Ty::MutexGuard(xt), Ty::MutexGuard(yt)) => types_match(xt, yt),
        (Ty::Option(xt), Ty::Option(yt)) => types_match(xt, yt),
        (Ty::ResultType(xok, xerr), Ty::ResultType(yok, yerr)) => {
            types_match(xok, yok) && types_match(xerr, yerr)
        }
        (Ty::AtomicPtr(xt), Ty::AtomicPtr(yt)) => types_match(xt, yt),
        (Ty::Struct(x), Ty::Vector(y, _)) | (Ty::Vector(y, _), Ty::Struct(x)) => x == y,
        (Ty::Enum(x), Ty::Enum(y)) => x == y,
        (Ty::Ptr(x), Ty::Ptr(y)) => types_match(x, y),
        (Ty::Matrix(x, _), Ty::Matrix(y, _)) => {
            matches!(x.as_ref(), Ty::Unit) || matches!(y.as_ref(), Ty::Unit) || types_match(x, y)
        }
        (Ty::Fn(xp, xr), Ty::Fn(yp, yr)) => {
            xp.len() == yp.len()
                && xp.iter().zip(yp).all(|(x, y)| types_match(x, y))
                && types_match(xr, yr)
        }
        _ => false,
    }
}

/// Convenience wrapper creating fresh generator per function.
fn emit_fn(
    f: &FnDef,
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fn_sigs: &HashMap<String, FnSig>,
    str_lit_syms: &HashMap<String, String>,
    mode: CodegenMode,
    closures: Rc<RefCell<ClosureState>>,
) -> String {
    Gen::new(
        structs,
        enums,
        vectors,
        fn_sigs,
        str_lit_syms,
        mode,
        closures,
    )
    .emit_fn(f)
}

#[cfg(test)]
mod tests;
