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
    Field(Box<Expr>, String),
    Index(Box<Expr>, Box<Expr>),
    /// Address of an lvalue (currently only a local variable).
    AddrOf(Box<Expr>),
    /// Dereference `*e`.
    Deref(Box<Expr>),
}
