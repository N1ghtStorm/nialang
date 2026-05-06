//! Built-in "std" surface: `println` is always available (reserved name).

pub const PRINTLN: &str = "println";

/// LLVM IR prelude used by builtin `println` codegen.
pub fn llvm_prelude() -> &'static str {
    r#"; --- nialang std ---
@nialang.std.fmt.i32 = private unnamed_addr constant [4 x i8] c"%d\0A\00", align 1
@nialang.std.fmt.u32 = private unnamed_addr constant [4 x i8] c"%u\0A\00", align 1
@nialang.std.fmt.i64 = private unnamed_addr constant [6 x i8] c"%lld\0A\00", align 1
@nialang.std.fmt.u64 = private unnamed_addr constant [6 x i8] c"%llu\0A\00", align 1
@nialang.std.fmt.i128hex = private unnamed_addr constant [18 x i8] c"0x%016llx%016llx\0A\00", align 1

declare i32 @printf(ptr nocapture, ...)

"#
}
