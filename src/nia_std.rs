//! Built-in "std" surface: `println` is always available (reserved name).

pub const PRINTLN: &str = "println";

/// LLVM IR prepended to every module: `printf`, format string, `define void @println(i32)`.
pub fn llvm_prelude() -> &'static str {
    r#"; --- nialang std ---
@nialang.std.println.fmt = private unnamed_addr constant [4 x i8] c"%d\0A\00", align 1

declare i32 @printf(ptr nocapture, ...)

define void @println(i32 %0) {
entry:
  %p = getelementptr inbounds [4 x i8], ptr @nialang.std.println.fmt, i64 0, i64 0
  %r = call i32 (ptr, ...) @printf(ptr %p, i32 %0)
  ret void
}

"#
}
