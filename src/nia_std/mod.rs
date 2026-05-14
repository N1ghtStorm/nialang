//! Built-in "std" surface: `println`, `len`, heap helpers, matrix helpers (reserved names).

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
pub const OUTER: &str = "outer";

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

declare i32 @printf(ptr nocapture, ...)
declare ptr @malloc(i64)
declare void @free(ptr)
declare ptr @realloc(ptr, i64)
declare void @abort()

"#
}
