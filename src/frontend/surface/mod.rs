//! User-facing syntax tree produced by the parser.
//!
//! This module is the surface contract for NiaLang source text. It is not the
//! dependently-typed Core and must not be passed to backends once the new
//! pipeline is in place.

/// Type syntax as written in source annotations and inferred surface forms.
///
/// During the compiler rewrite this remains the parser/typechecker surface
/// representation. The legacy typechecker still normalizes and compares these
/// types directly; the future Core will be elaborated from this layer.
#[derive(Debug, Clone)]
pub enum SurfaceTy {
    I8,
    U8,
    I16,
    U16,
    I32,
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
    Array(Box<SurfaceTy>, usize),
    Struct(String),
    Enum(String),
    /// `&T` — LLVM opaque `ptr` to `T`.
    Ptr(Box<SurfaceTy>),
    /// Result of a void call or `println`; not storable in `let`.
    Unit,
    Vector(String, Box<SurfaceTy>),
    /// Anonymous fixed-size vector type, written as `T<N>` in source annotations.
    AnonVector(Box<SurfaceTy>, usize),
    /// Reference-counted heap vector with dynamic length, written as `T<>`.
    HeapVector(Box<SurfaceTy>),
    /// Heap-backed growable list, written as `List[T]`.
    List(Box<SurfaceTy>),
    /// Built-in reference-counted heap matrix with one numeric cell type.
    Matrix(Box<SurfaceTy>, Option<(usize, usize)>),
    /// Refinement type `base { pred }` where `pred` may reference the bound name.
    Refined {
        base: Box<SurfaceTy>,
        pred: Box<Expr>,
    },
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub is_tuple: bool,
    pub fields: Vec<(String, SurfaceTy)>,
}

#[derive(Debug, Clone)]
pub struct VectorDef {
    pub name: String,
    pub ty: SurfaceTy,
    pub fields: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub is_extern: bool,
    pub is_quantum: bool,
    /// `(name, type, implicit)` — implicit params use surface syntax `#name: Type`.
    pub params: Vec<(String, SurfaceTy, bool)>,
    pub ret: Option<SurfaceTy>,
    /// Name of the structurally-decreasing parameter (`decreases xs`).
    pub decreases: Option<String>,
    /// Skips termination checking (`partial` / future `Div` effect).
    pub partial: bool,
    /// Function precondition (`requires P`).
    pub requires: Option<Expr>,
    /// Function postcondition (`ensures P`).
    pub ensures: Option<Expr>,
    pub body: Block,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: String,
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
    Tuple(Vec<SurfaceTy>),
    Struct(Vec<(String, SurfaceTy)>),
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
    pub tail: Option<Expr>,
}

#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Option<SurfaceTy>,
        init: Expr,
    },
    Expr(Expr),
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
    Loop {
        body: Block,
    },
    Break,
    For {
        var: String,
        start: Expr,
        end: Expr,
        body: Block,
    },
    Quant {
        body: Block,
    },
    Gpu {
        body: Block,
    },
    /// Manual proof stub (`admit P;`).
    Admit(Expr),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i128),
    Float(f64),
    Bool(bool),
    String(String),
    Ident(String),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    BitNot(Box<Expr>),
    Add(Box<Expr>, Box<Expr>),
    Sub(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
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
        ty_args: Vec<SurfaceTy>,
        args: Vec<Expr>,
    },
    MethodCall {
        receiver: Box<Expr>,
        name: String,
        args: Vec<Expr>,
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
    Quant {
        body: Box<Block>,
    },
    Gpu {
        body: Box<Block>,
    },
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    AddrOf(Box<Expr>),
    Deref(Box<Expr>),
}

/// Complete parsed surface module.
#[derive(Debug, Clone, Default)]
pub struct SurfaceModule {
    pub structs: Vec<StructDef>,
    pub enums: Vec<EnumDef>,
    pub fns: Vec<FnDef>,
    pub vectors: Vec<VectorDef>,
}

/// Strips refinement predicates for runtime typing and legacy codegen.
pub fn strip_refinement(ty: &SurfaceTy) -> &SurfaceTy {
    match ty {
        SurfaceTy::Refined { base, .. } => strip_refinement(base),
        other => other,
    }
}

impl PartialEq for SurfaceTy {
    fn eq(&self, other: &Self) -> bool {
        eq_surface_ty(strip_refinement(self), strip_refinement(other))
    }
}

impl Eq for SurfaceTy {}

fn eq_surface_ty(lhs: &SurfaceTy, rhs: &SurfaceTy) -> bool {
    use SurfaceTy::*;
    match (lhs, rhs) {
        (I8, I8) | (U8, U8) | (I16, I16) | (U16, U16) | (I32, I32) | (I64, I64) | (U64, U64)
        | (I128, I128) | (U128, U128) | (Isize, Isize) | (Usize, Usize) | (Bool, Bool)
        | (F16, F16) | (F32, F32) | (F64, F64) | (String, String) | (Qubit, Qubit)
        | (Result, Result) | (Unit, Unit) => true,
        (Ptr(l), Ptr(r)) => l == r,
        (Array(l, ln), Array(r, rn)) => ln == rn && l == r,
        (Struct(l), Struct(r)) | (Enum(l), Enum(r)) => l == r,
        (Vector(l, lt), Vector(r, rt)) => l == r && lt == rt,
        (AnonVector(l, ln), AnonVector(r, rn)) => ln == rn && l == r,
        (HeapVector(l), HeapVector(r)) | (List(l), List(r)) => l == r,
        (Matrix(l, ls), Matrix(r, rs)) => ls == rs && l == r,
        _ => false,
    }
}

pub fn method_symbol(owner: &SurfaceTy, method: &str) -> String {
    format!("{}__{}", ty_symbol_fragment(owner), method)
}

fn ty_symbol_fragment(t: &SurfaceTy) -> String {
    match t {
        SurfaceTy::I8 => "i8".into(),
        SurfaceTy::U8 => "u8".into(),
        SurfaceTy::I16 => "i16".into(),
        SurfaceTy::U16 => "u16".into(),
        SurfaceTy::I32 => "i32".into(),
        SurfaceTy::I64 => "i64".into(),
        SurfaceTy::U64 => "u64".into(),
        SurfaceTy::I128 => "i128".into(),
        SurfaceTy::Isize => "isize".into(),
        SurfaceTy::Usize => "usize".into(),
        SurfaceTy::U128 => "u128".into(),
        SurfaceTy::Bool => "bool".into(),
        SurfaceTy::F16 => "f16".into(),
        SurfaceTy::F32 => "f32".into(),
        SurfaceTy::F64 => "f64".into(),
        SurfaceTy::String => "string".into(),
        SurfaceTy::Qubit => "qubit".into(),
        SurfaceTy::Result => "result".into(),
        SurfaceTy::Array(elem, n) => format!("array_{}_{}", ty_symbol_fragment(elem), n),
        SurfaceTy::Struct(n) | SurfaceTy::Enum(n) | SurfaceTy::Vector(n, _) => n.clone(),
        SurfaceTy::Ptr(inner) => format!("ptr_{}", ty_symbol_fragment(inner)),
        SurfaceTy::Unit => "unit".into(),
        SurfaceTy::AnonVector(elem, n) => format!("anonvec_{}_{}", ty_symbol_fragment(elem), n),
        SurfaceTy::HeapVector(elem) => format!("heapvec_{}", ty_symbol_fragment(elem)),
        SurfaceTy::List(elem) => format!("list_{}", ty_symbol_fragment(elem)),
        SurfaceTy::Matrix(_, _) => "Matrix".into(),
        SurfaceTy::Refined { base, .. } => format!("refined_{}", ty_symbol_fragment(base)),
    }
}
