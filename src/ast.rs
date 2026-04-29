#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Ty {
    I32,
    U128,
    Struct(String),
    /// Result of a void call or `println`; not storable in `let`.
    Unit,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
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
}

#[derive(Debug, Clone)]
pub enum Expr {
    Int(i128),
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
    Field(Box<Expr>, String),
}
