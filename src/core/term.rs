use crate::core::effect::Effect;
use crate::core::quant::QuantKind;
use crate::core::meta::MetaId;
use crate::frontend::resolve::DefId;

/// de Bruijn level (absolute binding depth).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Level(pub u32);

/// Universe level: `Universe(0)` is `Type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct UniverseLevel(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Explicitness {
    Explicit,
    Implicit,
    Instance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Relevance {
    Runtime,
    Erased,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Binder {
    pub name_hint: String,
    pub level: Level,
    pub ty: Box<Term>,
    pub explicitness: Explicitness,
    pub relevance: Relevance,
}

impl Binder {
    pub fn new(
        name_hint: impl Into<String>,
        level: Level,
        ty: Term,
    ) -> Self {
        Self {
            name_hint: name_hint.into(),
            level,
            ty: Box::new(ty),
            explicitness: Explicitness::Explicit,
            relevance: Relevance::Runtime,
        }
    }

    pub fn ty(&self) -> &Term {
        &self.ty
    }
}

/// One arm of a non-dependent `DataMatch`.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchArm {
    pub variant_index: u32,
    pub body: Term,
}

/// Minimal dependently-typed core term.
#[derive(Debug, Clone, PartialEq)]
pub enum Term {
    Var(Level),
    Global(DefId),
    Universe(UniverseLevel),
    Pi {
        binder: Binder,
        body: Box<Term>,
    },
    Lam {
        binder: Binder,
        body: Box<Term>,
    },
    App {
        fun: Box<Term>,
        arg: Box<Term>,
    },
    Let {
        binder: Binder,
        value: Box<Term>,
        body: Box<Term>,
    },
    I32(i32),
    Bool(bool),
    /// Typed integer literal for non-i32 primitives.
      LitInt {
        value: i128,
        ty: DefId,
    },
    LitFloat {
        value: f64,
        ty: DefId,
    },
    LitStr(String),
    Unit,
    /// Nominal struct or enum constructor application.
    DataCtor {
        type_def: DefId,
        variant: u32,
        args: Vec<Term>,
    },
    /// Field projection on a nominal data value.
    DataProj {
        value: Box<Term>,
        type_def: DefId,
        field: u32,
    },
    /// Non-dependent pattern match on an enum scrutinee.
    DataMatch {
        scrutinee: Box<Term>,
        enum_def: DefId,
        arms: Vec<MatchArm>,
    },
    /// Unsolved metavariable during inference.
    Meta(MetaId),
    /// Fixed-size array literal `[T; N]`.
    ArrayLit {
        elem_ty: DefId,
        elems: Vec<Term>,
    },
    ArrayGet {
        elem_ty: DefId,
        len: u32,
        arr: Box<Term>,
        index: Box<Term>,
    },
    ArraySet {
        elem_ty: DefId,
        len: u32,
        arr: Box<Term>,
        index: Box<Term>,
        value: Box<Term>,
    },
    AddrOf {
        inner_ty: DefId,
        value: Box<Term>,
    },
    Deref {
        inner_ty: DefId,
        ptr: Box<Term>,
    },
    Len {
        elem_ty: DefId,
        len: u32,
        arr: Box<Term>,
    },
    While {
        cond: Box<Term>,
        body: Box<Term>,
    },
    Loop {
        body: Box<Term>,
    },
    For {
        var: String,
        start: Box<Term>,
        end: Box<Term>,
        body: Box<Term>,
    },
    Break,
    Assign {
        target: Box<Term>,
        value: Box<Term>,
    },
    HeapAlloc {
        ptr_ty: DefId,
        value: Box<Term>,
    },
    HeapDealloc {
        ptr_ty: DefId,
        ptr: Box<Term>,
    },
    HeapRealloc {
        ptr_ty: DefId,
        ptr: Box<Term>,
        value: Box<Term>,
    },
    /// Heap matrix from a nested array `[[T; cols]; rows]`.
    MatrixNew {
        matrix_ty: DefId,
        rows: u32,
        cols: u32,
        row_array_ty: DefId,
        outer_array_ty: DefId,
        src: Box<Term>,
    },
    /// Nested array from a heap matrix.
    MatrixToArray {
        matrix_ty: DefId,
        rows: u32,
        cols: u32,
        row_array_ty: DefId,
        outer_array_ty: DefId,
        matrix: Box<Term>,
    },
    MatrixDrop {
        matrix_ty: DefId,
        matrix: Box<Term>,
    },
    /// Refinement type `binder.ty { pred }`.
    Refinement {
        binder: Binder,
        pred: Box<Term>,
    },
    /// Manual proof stub; erased to unit at runtime.
    Admit {
        prop: Box<Term>,
    },
    /// Computation type `effect result` (F*-style M A).
    Computation {
        effect: Effect,
        result: Box<Term>,
    },
    /// Quantum primitive (qubit allocation, gates, measure, record).
    Quant {
        kind: QuantKind,
        args: Vec<Term>,
    },
    Error,
}

impl Term {
    pub fn var(level: u32) -> Self {
        Term::Var(Level(level))
    }

    pub fn universe(level: u32) -> Self {
        Term::Universe(UniverseLevel(level))
    }

    pub fn ty() -> Self {
        Term::universe(0)
    }

    pub fn kind() -> Self {
        Term::universe(1)
    }

    pub fn arrow(a: Term, b: Term) -> Self {
        let level = Level(0);
        Term::Pi {
            binder: Binder::new("_", level, a),
            body: Box::new(b),
        }
    }

    pub fn computation(effect: Effect, result: Term) -> Self {
        Term::Computation {
            effect,
            result: Box::new(result),
        }
    }

    pub fn peel_computation_result(self) -> Self {
        match self {
            Term::Computation { result, .. } => *result,
            other => other,
        }
    }

    /// Replace `Var(level)` with `replacement` inside `self`.
    pub fn subst(&self, level: Level, replacement: &Term) -> Term {
        match self {
            Term::Var(v) => {
                if *v == level {
                    replacement.clone()
                } else {
                    Term::Var(*v)
                }
            }
            Term::Global(id) => Term::Global(*id),
            Term::Universe(u) => Term::Universe(*u),
            Term::I32(n) => Term::I32(*n),
            Term::Bool(b) => Term::Bool(*b),
            Term::LitInt { value, ty } => Term::LitInt {
                value: *value,
                ty: *ty,
            },
            Term::LitFloat { value, ty } => Term::LitFloat {
                value: *value,
                ty: *ty,
            },
            Term::LitStr(s) => Term::LitStr(s.clone()),
            Term::Unit => Term::Unit,
            Term::DataCtor {
                type_def,
                variant,
                args,
            } => Term::DataCtor {
                type_def: *type_def,
                variant: *variant,
                args: args
                    .iter()
                    .map(|a| a.subst(level, replacement))
                    .collect(),
            },
            Term::DataProj {
                value,
                type_def,
                field,
            } => Term::DataProj {
                value: Box::new(value.subst(level, replacement)),
                type_def: *type_def,
                field: *field,
            },
            Term::DataMatch {
                scrutinee,
                enum_def,
                arms,
            } => Term::DataMatch {
                scrutinee: Box::new(scrutinee.subst(level, replacement)),
                enum_def: *enum_def,
                arms: arms
                    .iter()
                    .map(|arm| MatchArm {
                        variant_index: arm.variant_index,
                        body: arm.body.subst(level, replacement),
                    })
                    .collect(),
            },
            Term::Meta(id) => Term::Meta(*id),
            Term::Error => Term::Error,
            Term::ArrayLit { elem_ty, elems } => Term::ArrayLit {
                elem_ty: *elem_ty,
                elems: elems
                    .iter()
                    .map(|e| e.subst(level, replacement))
                    .collect(),
            },
            Term::ArrayGet {
                elem_ty,
                len,
                arr,
                index,
            } => Term::ArrayGet {
                elem_ty: *elem_ty,
                len: *len,
                arr: Box::new(arr.subst(level, replacement)),
                index: Box::new(index.subst(level, replacement)),
            },
            Term::ArraySet {
                elem_ty,
                len,
                arr,
                index,
                value,
            } => Term::ArraySet {
                elem_ty: *elem_ty,
                len: *len,
                arr: Box::new(arr.subst(level, replacement)),
                index: Box::new(index.subst(level, replacement)),
                value: Box::new(value.subst(level, replacement)),
            },
            Term::AddrOf { inner_ty, value } => Term::AddrOf {
                inner_ty: *inner_ty,
                value: Box::new(value.subst(level, replacement)),
            },
            Term::Deref { inner_ty, ptr } => Term::Deref {
                inner_ty: *inner_ty,
                ptr: Box::new(ptr.subst(level, replacement)),
            },
            Term::Len {
                elem_ty,
                len,
                arr,
            } => Term::Len {
                elem_ty: *elem_ty,
                len: *len,
                arr: Box::new(arr.subst(level, replacement)),
            },
            Term::While { cond, body } => Term::While {
                cond: Box::new(cond.subst(level, replacement)),
                body: Box::new(body.subst(level, replacement)),
            },
            Term::Loop { body } => Term::Loop {
                body: Box::new(body.subst(level, replacement)),
            },
            Term::For {
                var,
                start,
                end,
                body,
            } => Term::For {
                var: var.clone(),
                start: Box::new(start.subst(level, replacement)),
                end: Box::new(end.subst(level, replacement)),
                body: Box::new(body.subst(level, replacement)),
            },
            Term::Break => Term::Break,
            Term::Assign { target, value } => Term::Assign {
                target: Box::new(target.subst(level, replacement)),
                value: Box::new(value.subst(level, replacement)),
            },
            Term::HeapAlloc { ptr_ty, value } => Term::HeapAlloc {
                ptr_ty: *ptr_ty,
                value: Box::new(value.subst(level, replacement)),
            },
            Term::HeapDealloc { ptr_ty, ptr } => Term::HeapDealloc {
                ptr_ty: *ptr_ty,
                ptr: Box::new(ptr.subst(level, replacement)),
            },
            Term::HeapRealloc {
                ptr_ty,
                ptr,
                value,
            } => Term::HeapRealloc {
                ptr_ty: *ptr_ty,
                ptr: Box::new(ptr.subst(level, replacement)),
                value: Box::new(value.subst(level, replacement)),
            },
            Term::MatrixNew {
                matrix_ty,
                rows,
                cols,
                row_array_ty,
                outer_array_ty,
                src,
            } => Term::MatrixNew {
                matrix_ty: *matrix_ty,
                rows: *rows,
                cols: *cols,
                row_array_ty: *row_array_ty,
                outer_array_ty: *outer_array_ty,
                src: Box::new(src.subst(level, replacement)),
            },
            Term::MatrixToArray {
                matrix_ty,
                rows,
                cols,
                row_array_ty,
                outer_array_ty,
                matrix,
            } => Term::MatrixToArray {
                matrix_ty: *matrix_ty,
                rows: *rows,
                cols: *cols,
                row_array_ty: *row_array_ty,
                outer_array_ty: *outer_array_ty,
                matrix: Box::new(matrix.subst(level, replacement)),
            },
            Term::MatrixDrop { matrix_ty, matrix } => Term::MatrixDrop {
                matrix_ty: *matrix_ty,
                matrix: Box::new(matrix.subst(level, replacement)),
            },
            Term::Refinement { binder, pred } => {
                if binder.level.0 <= level.0 {
                    Term::Refinement {
                        binder: binder.clone(),
                        pred: pred.clone(),
                    }
                } else {
                    Term::Refinement {
                        binder: Binder {
                            ty: Box::new(binder.ty().subst(level, replacement)),
                            ..binder.clone()
                        },
                        pred: Box::new(pred.subst(level, replacement)),
                    }
                }
            }
            Term::Admit { prop } => Term::Admit {
                prop: Box::new(prop.subst(level, replacement)),
            },
            Term::Computation { effect, result } => Term::Computation {
                effect: *effect,
                result: Box::new(result.subst(level, replacement)),
            },
            Term::Quant { kind, args } => Term::Quant {
                kind: *kind,
                args: args
                    .iter()
                    .map(|a| a.subst(level, replacement))
                    .collect(),
            },
            Term::Pi { binder, body } => {
                if binder.level.0 <= level.0 {
                    Term::Pi {
                        binder: binder.clone(),
                        body: body.clone(),
                    }
                } else {
                    Term::Pi {
                        binder: Binder {
                            ty: Box::new(binder.ty().subst(level, replacement)),
                            ..binder.clone()
                        },
                        body: Box::new(body.subst(level, replacement)),
                    }
                }
            }
            Term::Lam { binder, body } => {
                if binder.level.0 <= level.0 {
                    Term::Lam {
                        binder: binder.clone(),
                        body: body.clone(),
                    }
                } else {
                    Term::Lam {
                        binder: Binder {
                            ty: Box::new(binder.ty().subst(level, replacement)),
                            ..binder.clone()
                        },
                        body: Box::new(body.subst(level, replacement)),
                    }
                }
            }
            Term::Let { binder, value, body } => {
                if binder.level.0 <= level.0 {
                    Term::Let {
                        binder: binder.clone(),
                        value: value.clone(),
                        body: body.clone(),
                    }
                } else {
                    Term::Let {
                        binder: Binder {
                            ty: Box::new(binder.ty().subst(level, replacement)),
                            ..binder.clone()
                        },
                        value: Box::new(value.subst(level, replacement)),
                        body: Box::new(body.subst(level, replacement)),
                    }
                }
            }
            Term::App { fun, arg } => Term::App {
                fun: Box::new(fun.subst(level, replacement)),
                arg: Box::new(arg.subst(level, replacement)),
            },
        }
    }
}
