//! Built-in "std" surface: `println`, `len`, heap helpers, matrix helpers (reserved names).

use crate::ast::{StructDef, Ty};

pub const PRINTLN: &str = "println";
/// Array length: `len(arr)` → `i32` (compile-time size of `[T; N]`).
pub const LEN: &str = "len";
pub const ALLOC: &str = "alloc";
pub const DEALLOC: &str = "dealloc";
pub const REALLOC: &str = "realloc";
pub const MATRIX_TYPE: &str = "Matrix";
pub const MATRIX_NEW: &str = "matrix";
pub const MATRIX_GET: &str = "matrix_get";
pub const MATRIX_SET: &str = "matrix_set";
pub const MATRIX_ROWS: &str = "matrix_rows";
pub const MATRIX_COLS: &str = "matrix_cols";
pub const MATRIX_LEN: &str = "matrix_len";
pub const MATRIX_CLONE: &str = "matrix_clone";
pub const MATRIX_REFCOUNT: &str = "matrix_refcount";
pub const MATRIX_DROP: &str = "matrix_drop";
pub const VECTOR_GET: &str = "vector_get";
pub const VECTOR_SET: &str = "vector_set";
pub const VECTOR_LEN: &str = "vector_len";
pub const VECTOR_CLONE: &str = "vector_clone";
pub const VECTOR_REFCOUNT: &str = "vector_refcount";
pub const VECTOR_DROP: &str = "vector_drop";
pub const LIST_TYPE: &str = "List";
pub const LIST_NEW: &str = "list_new";
pub const LIST_WITH_CAPACITY: &str = "list_with_capacity";
pub const LIST_LEN: &str = "len";
pub const LIST_CAPACITY: &str = "capacity";
pub const LIST_PUSH: &str = "push";
pub const LIST_GET: &str = "get";
pub const OUTER: &str = "outer";
pub const TO_ARRAY: &str = "to_array";
pub const TO_MATRIX: &str = "to_matrix";
pub const TO_VEC: &str = "to_vec";
pub const COMPLEX_TYPE: &str = "Complex";
pub const COMPLEX_NEW: &str = "complex";
pub const COMPLEX_ADD: &str = "complex_add";
pub const COMPLEX_SUB: &str = "complex_sub";
pub const COMPLEX_MUL: &str = "complex_mul";
pub const COMPLEX_SCALE: &str = "complex_scale";
pub const COMPLEX_DIV: &str = "complex_div";
pub const SIN: &str = "sin";
pub const COS: &str = "cos";
pub const PI: &str = "PI";
pub const CIS: &str = "cis";

pub fn complex_ty() -> Ty {
    Ty::Struct(COMPLEX_TYPE.into())
}

pub fn builtin_structs() -> Vec<StructDef> {
    vec![StructDef {
        name: COMPLEX_TYPE.into(),
        is_tuple: false,
        fields: vec![("re".into(), Ty::F64), ("im".into(), Ty::F64)],
    }]
}

pub fn is_reserved_type_name(name: &str) -> bool {
    matches!(name, MATRIX_TYPE | COMPLEX_TYPE | LIST_TYPE)
}

pub fn is_reserved_fn_name(name: &str) -> bool {
    matches!(
        name,
        PRINTLN
            | LEN
            | ALLOC
            | DEALLOC
            | REALLOC
            | MATRIX_NEW
            | MATRIX_GET
            | MATRIX_SET
            | MATRIX_ROWS
            | MATRIX_COLS
            | MATRIX_LEN
            | MATRIX_CLONE
            | MATRIX_REFCOUNT
            | MATRIX_DROP
            | VECTOR_GET
            | VECTOR_SET
            | VECTOR_LEN
            | VECTOR_CLONE
            | VECTOR_REFCOUNT
            | VECTOR_DROP
            | LIST_NEW
            | LIST_WITH_CAPACITY
            | OUTER
            | COMPLEX_NEW
            | COMPLEX_ADD
            | COMPLEX_SUB
            | COMPLEX_MUL
            | COMPLEX_SCALE
            | COMPLEX_DIV
            | SIN
            | COS
            | PI
            | CIS
    )
}

/// LLVM IR prelude used by builtin `println` codegen.
///
/// Contains:
/// - all static format strings/text fragments used by generated `printf` calls,
/// - external declaration of `printf`.
/// The returned string is embedded at the top of every generated module.
pub fn llvm_prelude() -> &'static str {
    r#"; --- nialang std ---
@nialang.std.fmt.i32 = private unnamed_addr constant [4 x i8] c"%d\0A\00", align 1
@nialang.std.fmt.u32 = private unnamed_addr constant [4 x i8] c"%u\0A\00", align 1
@nialang.std.fmt.i64 = private unnamed_addr constant [6 x i8] c"%lld\0A\00", align 1
@nialang.std.fmt.u64 = private unnamed_addr constant [6 x i8] c"%llu\0A\00", align 1
@nialang.std.fmt.i128hex = private unnamed_addr constant [18 x i8] c"0x%016llx%016llx\0A\00", align 1
@nialang.std.fmt.ptrhex = private unnamed_addr constant [8 x i8] c"0x%llx\0A\00", align 1
@nialang.std.fmt.matrix = private unnamed_addr constant [41 x i8] c"Matrix(rows=%lld, cols=%lld, refs=%lld)\0A\00", align 1
@nialang.std.fmt.matrix.nn = private unnamed_addr constant [40 x i8] c"Matrix(rows=%lld, cols=%lld, refs=%lld)\00", align 1
@nialang.std.fmt.f64 = private unnamed_addr constant [4 x i8] c"%f\0A\00", align 1
@nialang.std.fmt.f64.nn = private unnamed_addr constant [3 x i8] c"%f\00", align 1
@nialang.std.fmt.str = private unnamed_addr constant [4 x i8] c"%s\0A\00", align 1
@nialang.std.fmt.str.nn = private unnamed_addr constant [3 x i8] c"%s\00", align 1
@nialang.std.fmt.i32.nn = private unnamed_addr constant [3 x i8] c"%d\00", align 1
@nialang.std.fmt.u32.nn = private unnamed_addr constant [3 x i8] c"%u\00", align 1
@nialang.std.fmt.i64.nn = private unnamed_addr constant [5 x i8] c"%lld\00", align 1
@nialang.std.fmt.u64.nn = private unnamed_addr constant [5 x i8] c"%llu\00", align 1
@nialang.std.fmt.i128hex.nn = private unnamed_addr constant [17 x i8] c"0x%016llx%016llx\00", align 1
@nialang.std.fmt.ptrhex.nn = private unnamed_addr constant [7 x i8] c"0x%llx\00", align 1
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

declare i32 @printf(ptr nocapture, ...)
declare i32 @strcmp(ptr, ptr)
declare ptr @malloc(i64)
declare void @free(ptr)
declare ptr @realloc(ptr, i64)
declare void @abort()
declare double @sin(double)
declare double @cos(double)

"#
}
