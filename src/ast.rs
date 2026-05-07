#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
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
    Array(Box<Ty>, usize),
    Struct(String),
    Enum(String),
    /// `&T` — LLVM opaque `ptr` to `T`.
    Ptr(Box<Ty>),
    /// Result of a void call or `println`; not storable in `let`.
    Unit,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub is_tuple: bool,
    pub fields: Vec<(String, Ty)>,
}

#[derive(Debug, Clone)]
pub struct FnDef {
    pub name: String,
    pub params: Vec<(String, Ty)>,
    pub ret: Option<Ty>,
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
        init: Expr,
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
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i128),
    Bool(bool),
    Ident(String),
    Add(Box<Expr>, Box<Expr>),
    Call {
        name: String,
        args: Vec<Expr>,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
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
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// Address of an lvalue (currently only a local variable).
    AddrOf(Box<Expr>),
    /// Dereference `*e`.
    Deref(Box<Expr>),
}
