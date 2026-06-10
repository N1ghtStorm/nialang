use std::collections::HashMap;

use crate::core::globals::{prim, GlobalEnv};
use crate::core::term::{Level, Term};

/// Metavariable identifier for type inference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MetaId(pub u32);

/// Tracks metavariable solutions during type inference.
#[derive(Debug, Clone, Default)]
pub struct MetaEnv {
    next: u32,
    solutions: HashMap<MetaId, Term>,
    implicit_names: HashMap<MetaId, String>,
}

impl MetaEnv {
    pub fn fresh(&mut self) -> MetaId {
        let id = MetaId(self.next);
        self.next += 1;
        id
    }

    pub fn fresh_implicit(&mut self, name: impl Into<String>) -> MetaId {
        let id = self.fresh();
        self.implicit_names.insert(id, name.into());
        id
    }

    pub fn implicit_name(&self, id: MetaId) -> Option<&str> {
        self.implicit_names.get(&id).map(String::as_str)
    }

    pub fn solve(&mut self, id: MetaId, term: Term) -> Result<(), String> {
        if term.mentions_meta(id) {
            return Err(self.implicit_error(
                id,
                "occurs check failed while inferring implicit argument",
            ));
        }
        let normalized = self.normalize(&term);
        if let Some(existing) = self.solutions.get(&id) {
            if existing != &normalized {
                return Err(self.implicit_error(
                    id,
                    &format!(
                        "ambiguous implicit argument: cannot unify `{}` with `{}`",
                        display_term(existing),
                        display_term(&normalized)
                    ),
                ));
            }
            return Ok(());
        }
        self.solutions.insert(id, normalized);
        Ok(())
    }

    /// Returns an error if any implicit metavariable was left unsolved.
    pub fn ensure_solved(&self) -> Result<(), String> {
        for (id, name) in &self.implicit_names {
            if self.solutions.get(id).is_none() {
                return Err(format!("failed to infer implicit argument `{name}`"));
            }
        }
        Ok(())
    }

    pub fn solved_implicit_types(&self) -> Vec<(&str, &Term)> {
        self.implicit_names
            .iter()
            .filter_map(|(id, name)| {
                self.solutions
                    .get(id)
                    .map(|term| (name.as_str(), term))
            })
            .collect()
    }

    fn implicit_error(&self, id: MetaId, detail: &str) -> String {
        match self.implicit_name(id) {
            Some(name) => format!("failed to infer implicit argument `{name}`: {detail}"),
            None => detail.to_string(),
        }
    }

    /// When unification fails between ground types while an implicit is already solved,
    /// produce a clearer diagnostic for conflicting uses of the same type parameter.
    pub fn implicit_unify_hint(
        &self,
        inferred: &Term,
        expected: &Term,
        globals: &GlobalEnv,
    ) -> Option<String> {
        if !is_ground_type(inferred) || !is_ground_type(expected) {
            return None;
        }
        let (name, _solution) = self.solved_implicit_types().into_iter().next()?;
        Some(format!(
            "ambiguous implicit argument `{name}`: cannot unify `{}` with `{}`",
            format_type(inferred, globals),
            format_type(expected, globals)
        ))
    }

    pub fn lookup(&self, id: MetaId) -> Option<&Term> {
        self.solutions.get(&id)
    }

    /// Expand solved metas inside `term`.
    pub fn normalize(&self, term: &Term) -> Term {
        match term {
            Term::Meta(id) => {
                if let Some(sol) = self.solutions.get(id) {
                    self.normalize(sol)
                } else {
                    Term::Meta(*id)
                }
            }
            Term::Var(v) => Term::Var(*v),
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
            Term::Error => Term::Error,
            Term::Pi { binder, body } => Term::Pi {
                binder: Binder {
                    ty: Box::new(self.normalize(binder.ty())),
                    ..binder.clone()
                },
                body: Box::new(self.normalize(body)),
            },
            Term::Lam { binder, body } => Term::Lam {
                binder: Binder {
                    ty: Box::new(self.normalize(binder.ty())),
                    ..binder.clone()
                },
                body: Box::new(self.normalize(body)),
            },
            Term::App { fun, arg } => Term::App {
                fun: Box::new(self.normalize(fun)),
                arg: Box::new(self.normalize(arg)),
            },
            Term::Let { binder, value, body } => Term::Let {
                binder: Binder {
                    ty: Box::new(self.normalize(binder.ty())),
                    ..binder.clone()
                },
                value: Box::new(self.normalize(value)),
                body: Box::new(self.normalize(body)),
            },
            Term::DataCtor {
                type_def,
                variant,
                args,
            } => Term::DataCtor {
                type_def: *type_def,
                variant: *variant,
                args: args.iter().map(|a| self.normalize(a)).collect(),
            },
            Term::DataProj {
                value,
                type_def,
                field,
            } => Term::DataProj {
                value: Box::new(self.normalize(value)),
                type_def: *type_def,
                field: *field,
            },
            Term::DataMatch {
                scrutinee,
                enum_def,
                arms,
            } => Term::DataMatch {
                scrutinee: Box::new(self.normalize(scrutinee)),
                enum_def: *enum_def,
                arms: arms
                    .iter()
                    .map(|arm| crate::core::term::MatchArm {
                        variant_index: arm.variant_index,
                        body: self.normalize(&arm.body),
                    })
                    .collect(),
            },
            Term::ArrayLit { elem_ty, elems } => Term::ArrayLit {
                elem_ty: *elem_ty,
                elems: elems.iter().map(|e| self.normalize(e)).collect(),
            },
            Term::ArrayGet {
                elem_ty,
                len,
                arr,
                index,
            } => Term::ArrayGet {
                elem_ty: *elem_ty,
                len: *len,
                arr: Box::new(self.normalize(arr)),
                index: Box::new(self.normalize(index)),
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
                arr: Box::new(self.normalize(arr)),
                index: Box::new(self.normalize(index)),
                value: Box::new(self.normalize(value)),
            },
            Term::AddrOf { inner_ty, value } => Term::AddrOf {
                inner_ty: *inner_ty,
                value: Box::new(self.normalize(value)),
            },
            Term::Deref { inner_ty, ptr } => Term::Deref {
                inner_ty: *inner_ty,
                ptr: Box::new(self.normalize(ptr)),
            },
            Term::Len {
                elem_ty,
                len,
                arr,
            } => Term::Len {
                elem_ty: *elem_ty,
                len: *len,
                arr: Box::new(self.normalize(arr)),
            },
            Term::While { cond, body } => Term::While {
                cond: Box::new(self.normalize(cond)),
                body: Box::new(self.normalize(body)),
            },
            Term::Loop { body } => Term::Loop {
                body: Box::new(self.normalize(body)),
            },
            Term::For {
                var,
                start,
                end,
                body,
            } => Term::For {
                var: var.clone(),
                start: Box::new(self.normalize(start)),
                end: Box::new(self.normalize(end)),
                body: Box::new(self.normalize(body)),
            },
            Term::Break => Term::Break,
            Term::Assign { target, value } => Term::Assign {
                target: Box::new(self.normalize(target)),
                value: Box::new(self.normalize(value)),
            },
            Term::HeapAlloc { ptr_ty, value } => Term::HeapAlloc {
                ptr_ty: *ptr_ty,
                value: Box::new(self.normalize(value)),
            },
            Term::HeapDealloc { ptr_ty, ptr } => Term::HeapDealloc {
                ptr_ty: *ptr_ty,
                ptr: Box::new(self.normalize(ptr)),
            },
            Term::HeapRealloc {
                ptr_ty,
                ptr,
                value,
            } => Term::HeapRealloc {
                ptr_ty: *ptr_ty,
                ptr: Box::new(self.normalize(ptr)),
                value: Box::new(self.normalize(value)),
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
                src: Box::new(self.normalize(src)),
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
                matrix: Box::new(self.normalize(matrix)),
            },
            Term::MatrixDrop { matrix_ty, matrix } => Term::MatrixDrop {
                matrix_ty: *matrix_ty,
                matrix: Box::new(self.normalize(matrix)),
            },
            Term::Refinement { binder, pred } => Term::Refinement {
                binder: Binder {
                    ty: Box::new(self.normalize(binder.ty())),
                    ..binder.clone()
                },
                pred: Box::new(self.normalize(pred)),
            },
            Term::Admit { prop } => Term::Admit {
                prop: Box::new(self.normalize(prop)),
            },
            Term::Computation { effect, result } => Term::Computation {
                effect: *effect,
                result: Box::new(self.normalize(result)),
            },
            Term::Quant { kind, args } => Term::Quant {
                kind: *kind,
                args: args.iter().map(|a| self.normalize(a)).collect(),
            },
        }
    }

    pub fn subst_level(&self, term: &Term, level: Level, replacement: &Term) -> Term {
        self.normalize(&term.subst(level, replacement))
    }
}

use crate::core::term::Binder;

pub fn format_type(term: &Term, globals: &GlobalEnv) -> String {
    match term {
        Term::Global(id) => primitive_name(*id)
            .or_else(|| globals.type_of(*id).map(|_| format!("{id:?}")))
            .unwrap_or_else(|| format!("{id:?}")),
        Term::Var(level) => format!("@{level:?}"),
        Term::Meta(id) => format!("?{id:?}"),
        Term::Universe(_) => "Type".into(),
        Term::I32(n) => format!("i32({n})"),
        Term::Bool(b) => format!("bool({b})"),
        Term::Unit => "unit".into(),
        other => format!("{other:?}"),
    }
}

fn display_term(term: &Term) -> String {
    format_type(term, &GlobalEnv::with_primitives())
}

fn primitive_name(id: crate::frontend::resolve::DefId) -> Option<String> {
    Some(match id {
        x if x == prim::I8 => "i8",
        x if x == prim::U8 => "u8",
        x if x == prim::I16 => "i16",
        x if x == prim::U16 => "u16",
        x if x == prim::I32 => "i32",
        x if x == prim::I64 => "i64",
        x if x == prim::U64 => "u64",
        x if x == prim::I128 => "i128",
        x if x == prim::U128 => "u128",
        x if x == prim::BOOL => "bool",
        x if x == prim::UNIT => "unit",
        x if x == prim::F16 => "f16",
        x if x == prim::F32 => "f32",
        x if x == prim::F64 => "f64",
        x if x == prim::STRING => "string",
        _ => return None,
    }
    .to_string())
}

fn is_ground_type(term: &Term) -> bool {
    matches!(
        term,
        Term::Global(_) | Term::I32(_) | Term::Bool(_) | Term::Unit | Term::Universe(_)
    )
}

impl Term {
    pub fn mentions_meta(&self, id: MetaId) -> bool {
        match self {
            Term::Meta(mid) => *mid == id,
            Term::Var(_) | Term::Global(_) | Term::Universe(_) | Term::I32(_) | Term::Bool(_)
            | Term::Unit | Term::Error | Term::Break => false,
            Term::LitInt { .. } | Term::LitFloat { .. } | Term::LitStr(_) => false,
            Term::Pi { binder, body } => binder.ty().mentions_meta(id) || body.mentions_meta(id),
            Term::Lam { binder, body } => binder.ty().mentions_meta(id) || body.mentions_meta(id),
            Term::App { fun, arg } => fun.mentions_meta(id) || arg.mentions_meta(id),
            Term::Let { binder, value, body } => {
                binder.ty().mentions_meta(id)
                    || value.mentions_meta(id)
                    || body.mentions_meta(id)
            }
            Term::DataCtor { args, .. } => args.iter().any(|a| a.mentions_meta(id)),
            Term::DataProj { value, .. } => value.mentions_meta(id),
            Term::DataMatch { scrutinee, arms, .. } => {
                scrutinee.mentions_meta(id) || arms.iter().any(|a| a.body.mentions_meta(id))
            }
            Term::ArrayLit { elems, .. } => elems.iter().any(|e| e.mentions_meta(id)),
            Term::ArrayGet { arr, index, .. } => arr.mentions_meta(id) || index.mentions_meta(id),
            Term::ArraySet {
                arr, index, value, ..
            } => arr.mentions_meta(id) || index.mentions_meta(id) || value.mentions_meta(id),
            Term::AddrOf { value, .. } => value.mentions_meta(id),
            Term::Deref { ptr, .. } => ptr.mentions_meta(id),
            Term::Len { arr, .. } => arr.mentions_meta(id),
            Term::While { cond, body } => cond.mentions_meta(id) || body.mentions_meta(id),
            Term::Loop { body } => body.mentions_meta(id),
            Term::For { start, end, body, .. } => {
                start.mentions_meta(id) || end.mentions_meta(id) || body.mentions_meta(id)
            }
            Term::Assign { target, value } => {
                target.mentions_meta(id) || value.mentions_meta(id)
            }
            Term::HeapAlloc { value, .. } => value.mentions_meta(id),
            Term::HeapDealloc { ptr, .. } => ptr.mentions_meta(id),
            Term::HeapRealloc { ptr, value, .. } => {
                ptr.mentions_meta(id) || value.mentions_meta(id)
            }
            Term::MatrixNew { src, .. } => src.mentions_meta(id),
            Term::MatrixToArray { matrix, .. } => matrix.mentions_meta(id),
            Term::MatrixDrop { matrix, .. } => matrix.mentions_meta(id),
            Term::Refinement { binder, pred } => {
                binder.ty().mentions_meta(id) || pred.mentions_meta(id)
            }
            Term::Admit { prop } => prop.mentions_meta(id),
            Term::Computation { result, .. } => result.mentions_meta(id),
            Term::Quant { args, .. } => args.iter().any(|a| a.mentions_meta(id)),
        }
    }
}
