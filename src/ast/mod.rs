/// Core semantic type model shared by all compiler stages.
///
/// This enum is the single source of truth for "what type a value has" once
/// source text is parsed into AST. Every major phase depends on it:
/// - parser stores explicit type annotations as `Ty`,
/// - type checker infers and compares expression/statement types using `Ty`,
/// - codegen maps `Ty` to concrete LLVM IR types.
///
/// Why centralizing this matters:
/// - keeps parser/typecheck/codegen in sync on supported type forms,
/// - avoids ad-hoc string-based type handling,
/// - makes type equality/comparisons deterministic across passes.
///
/// Variant groups:
/// - integer primitives (`I8`..`U128`) and `Bool`,
/// - float primitives (`F16`, `F32`, `F64`, same names as Rust),
/// - composites (`Array`, `Struct`, `Enum`, `Option`, `ResultType`),
/// - quantum resources (`Qubit`, `Result`),
/// - indirection (`Ptr`),
/// - effect/absence type (`Unit`).
///
/// Notes:
/// - `Struct(String)` / `Enum(String)` store user-defined type names and are
///   resolved/validated against symbol tables during semantic analysis.
/// - `Ptr(Box<Ty>)` preserves pointee type for type safety (e.g. dereference and
///   assignment checks), even though LLVM lowering uses opaque `ptr`.
/// - `Unit` models "no value" results (void-like), allowing typechecker rules to
///   reject invalid contexts such as binding a void result in `let`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    I64,
    U64,
    I128,
    Isize,
    Usize,
    U128,
    Bool,
    F16,
    F32,
    F64,
    /// UTF-8 text; lowered as null-terminated `ptr` to bytes in LLVM.
    String,
    /// Quantum bit resource; valid only inside `quant` semantic scopes.
    Qubit,
    /// Quantum measurement result; valid only inside `quant` semantic scopes.
    Result,
    Array(Box<Ty>, usize),
    Struct(String),
    Enum(String),
    /// `&T` — LLVM opaque `ptr` to `T`.
    Ptr(Box<Ty>),
    /// Result of a void call or `println`; not storable in `let`.
    Unit,
    Vector(String, Box<Ty>),
    /// Anonymous fixed-size vector type, written as `T<N>` in source annotations.
    AnonVector(Box<Ty>, usize),
    /// Unique heap-owned vector with dynamic length, written as `T<>`.
    HeapVector(Box<Ty>),
    /// Heap-backed growable list, written as `List[T]`.
    List(Box<Ty>),
    /// Atomic reference-counted shared owner, written as `Arc[T]`.
    Arc(Box<Ty>),
    /// Built-in optional value type, written as `Option[T]`.
    Option(Box<Ty>),
    /// Built-in fallible value type, written as `Result[T, E]`.
    ResultType(Box<Ty>, Box<Ty>),
    /// Built-in atomic boolean storage cell, written as `AtomicBool`.
    AtomicBool,
    /// Built-in atomic 8-bit signed integer storage cell, written as `AtomicI8`.
    AtomicI8,
    /// Built-in atomic 8-bit unsigned integer storage cell, written as `AtomicU8`.
    AtomicU8,
    /// Built-in atomic 16-bit signed integer storage cell, written as `AtomicI16`.
    AtomicI16,
    /// Built-in atomic 16-bit unsigned integer storage cell, written as `AtomicU16`.
    AtomicU16,
    /// Built-in atomic 32-bit signed integer storage cell, written as `AtomicI32`.
    AtomicI32,
    /// Built-in atomic 32-bit unsigned integer storage cell, written as `AtomicU32`.
    AtomicU32,
    /// Built-in atomic 64-bit signed integer storage cell, written as `AtomicI64`.
    AtomicI64,
    /// Built-in atomic 64-bit unsigned integer storage cell, written as `AtomicU64`.
    AtomicU64,
    /// Built-in atomic 128-bit signed integer storage cell, written as `AtomicI128`.
    AtomicI128,
    /// Built-in atomic 128-bit unsigned integer storage cell, written as `AtomicU128`.
    AtomicU128,
    /// Built-in atomic pointer-width signed integer storage cell, written as `AtomicIsize`.
    AtomicIsize,
    /// Built-in atomic pointer-width unsigned integer storage cell, written as `AtomicUsize`.
    AtomicUsize,
    /// Built-in atomic pointer storage cell, written as `AtomicPtr[T]`.
    AtomicPtr(Box<Ty>),
    /// Built-in native thread handle, written as `Thread`.
    Thread,
    /// Built-in unique heap-owned matrix with one numeric cell type.
    ///
    /// The optional `(rows, cols)` shape is known for matrix literals and derived
    /// matrix expressions. A plain source annotation `Matrix` keeps it as `None`.
    Matrix(Box<Ty>, Option<(usize, usize)>),
    /// Function value type, written as `fn(T1, T2) -> Ret`.
    Fn(Vec<Ty>, Box<Ty>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Ability {
    Copy,
    Clone,
    Drop,
    Deref,
    Send,
    Sync,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub abilities: Vec<Ability>,
    pub is_tuple: bool,
    pub fields: Vec<(String, Ty)>,
}
#[derive(Debug, Clone)]
pub struct VectorDef {
    pub name: String,
    pub abilities: Vec<Ability>,
    pub ty: Ty,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    /// Source-level `extern fn` marker for top-level C ABI exports.
    pub is_extern: bool,
    /// Source-level `quant fn` marker for functions callable only in quantum scopes.
    pub is_quantum: bool,
    pub params: Vec<(String, Ty)>,
    pub ret: Option<Ty>,
    pub body: Block,
    pub closure_captures: Vec<(String, Ty)>,
}

pub fn method_symbol(owner: &Ty, method: &str) -> String {
    format!("{}__{}", ty_symbol_fragment(owner), method)
}

fn ty_symbol_fragment(t: &Ty) -> String {
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
        Ty::F16 => "f16".into(),
        Ty::F32 => "f32".into(),
        Ty::F64 => "f64".into(),
        Ty::String => "string".into(),
        Ty::Qubit => "qubit".into(),
        Ty::Result => "result".into(),
        Ty::Array(elem, n) => format!("array_{}_{}", ty_symbol_fragment(elem), n),
        Ty::Struct(n) | Ty::Enum(n) | Ty::Vector(n, _) => n.clone(),
        Ty::Ptr(inner) => format!("ptr_{}", ty_symbol_fragment(inner)),
        Ty::Unit => "unit".into(),
        Ty::AnonVector(elem, n) => format!("anonvec_{}_{}", ty_symbol_fragment(elem), n),
        Ty::HeapVector(elem) => format!("heapvec_{}", ty_symbol_fragment(elem)),
        Ty::List(elem) => format!("list_{}", ty_symbol_fragment(elem)),
        Ty::Arc(elem) => format!("arc_{}", ty_symbol_fragment(elem)),
        Ty::Option(elem) => format!("option_{}", ty_symbol_fragment(elem)),
        Ty::ResultType(ok, err) => {
            format!(
                "result_{}_{}",
                ty_symbol_fragment(ok),
                ty_symbol_fragment(err)
            )
        }
        Ty::AtomicBool => "atomic_bool".into(),
        Ty::AtomicI8 => "atomic_i8".into(),
        Ty::AtomicU8 => "atomic_u8".into(),
        Ty::AtomicI16 => "atomic_i16".into(),
        Ty::AtomicU16 => "atomic_u16".into(),
        Ty::AtomicI32 => "atomic_i32".into(),
        Ty::AtomicU32 => "atomic_u32".into(),
        Ty::AtomicI64 => "atomic_i64".into(),
        Ty::AtomicU64 => "atomic_u64".into(),
        Ty::AtomicI128 => "atomic_i128".into(),
        Ty::AtomicU128 => "atomic_u128".into(),
        Ty::AtomicIsize => "atomic_isize".into(),
        Ty::AtomicUsize => "atomic_usize".into(),
        Ty::AtomicPtr(elem) => format!("atomic_ptr_{}", ty_symbol_fragment(elem)),
        Ty::Thread => "thread".into(),
        Ty::Matrix(_, _) => "Matrix".into(),
        Ty::Fn(params, ret) => {
            let params = params
                .iter()
                .map(ty_symbol_fragment)
                .collect::<Vec<_>>()
                .join("_");
            format!("fn_{}_to_{}", params, ty_symbol_fragment(ret))
        }
    }
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
    pub abilities: Vec<Ability>,
    pub variants: Vec<EnumVariantDef>,
}

#[derive(Debug, Clone)]
pub struct EnumVariantDef {
    pub name: String,
    pub fields: EnumVariantFields,
}

#[derive(Debug, Clone)]
pub enum EnumVariantFields {
    Unit,
    Tuple(Vec<Ty>),
    Struct(Vec<(String, Ty)>),
}

#[derive(Debug, Clone)]
pub enum MatchPattern {
    Unit {
        enum_name: String,
        variant: String,
    },
    Tuple {
        enum_name: String,
        variant: String,
        bindings: Vec<String>,
    },
    Struct {
        enum_name: String,
        variant: String,
        bindings: Vec<String>,
    },
}

#[derive(Debug, Clone)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    /// Trailing expression without `;` (return value when function has a return type).
    pub tail: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<Ty>,
        init: Option<Expr>,
    },
    /// Expression followed by `;` (e.g. `println(1);`).
    Expr(Expr),
    /// Assignment statement (e.g. `x = 1;` or `*p = v;`).
    Assign {
        target: Expr,
        value: Expr,
    },
    Return(Expr),
    If {
        cond: Expr,
        then_block: Block,
    },
    While {
        cond: Expr,
        body: Block,
    },
    /// Infinite loop; exit only via `break` (must appear at least once in body for codegen).
    Loop {
        body: Block,
    },
    /// Exit the innermost enclosing `loop` (Nia: only `loop`, not `while`/`for`).
    Break,
    /// `for name in start..end { ... }` — half-open numeric range (like Rust `..`).
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Block,
    },
    /// `quant { ... }` — reserved syntax that currently behaves like a block scope.
    Quant {
        body: Block,
    },
    /// `gpu { ... }` — reserved syntax that currently behaves like a block scope.
    Gpu {
        body: Block,
    },
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i128),
    /// Float literal; stored as `f64` and coerced in codegen to the target float type.
    Float(f64),
    Bool(bool),
    /// UTF-8 string literal (source escapes decoded).
    String(String),
    Ident(String),
    /// Unary `-` (integer and float).
    Neg(Box<Expr>),
    /// Unary logical negation `!` (booleans only).
    Not(Box<Expr>),
    /// Unary bitwise complement `~` (integers only).
    BitNot(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
    /// Dot product `u @ v` (same `vector` type; result has the axis scalar type).
    VecDot(Box<Expr>, Box<Expr>),
    Div(Box<Expr>, Box<Expr>),
    Rem(Box<Expr>, Box<Expr>),
    BitAnd(Box<Expr>, Box<Expr>),
    BitOr(Box<Expr>, Box<Expr>),
    BitXor(Box<Expr>, Box<Expr>),
    Shl(Box<Expr>, Box<Expr>),
    Shr(Box<Expr>, Box<Expr>),
    Eq(Box<Expr>, Box<Expr>),
    Ne(Box<Expr>, Box<Expr>),
    Lt(Box<Expr>, Box<Expr>),
    Le(Box<Expr>, Box<Expr>),
    Gt(Box<Expr>, Box<Expr>),
    Ge(Box<Expr>, Box<Expr>),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    GenericCall {
        name: String,
        ty_args: Vec<Ty>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        name: String,
        args: Vec<Expr>,
    },
    CallExpr {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Closure {
        is_move: bool,
        params: Vec<(String, Option<Ty>)>,
        ret: Option<Ty>,
        body: Box<Block>,
    },
    /// Built-in thread spawn expression, written as `spawn function_name`.
    Spawn {
        target: String,
    },
    /// Built-in thread spawn expression, written as `spawn move || { ... }`.
    SpawnClosure {
        closure: Box<Expr>,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    VectorLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    AnonVectorLit(Vec<Expr>),
    ArrayLit(Vec<Expr>),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    EnumTuple {
        enum_name: String,
        variant: String,
        args: Vec<Expr>,
    },
    EnumStruct {
        enum_name: String,
        variant: String,
        fields: Vec<(String, Expr)>,
    },
    Match {
        scrutinee: Box<Expr>,
        arms: Vec<(MatchPattern, Expr)>,
    },
    /// `quant { ... }` used as an expression; evaluates to the block tail or `()`.
    Quant {
        body: Box<Block>,
    },
    /// `gpu { ... }` used as an expression; evaluates to the block tail or `()`.
    Gpu {
        body: Box<Block>,
    },
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// Address of an lvalue (currently only a local variable).
    AddrOf(Box<Expr>),
    /// Dereference `*e`.
    Deref(Box<Expr>),
}
