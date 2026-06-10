//! Inductive families and strict-positivity checking (phase 6).

use crate::core::data::{DataEnv, InductiveCtor, InductiveInfo};
use crate::core::globals::GlobalEnv;
use crate::core::term::{Binder, Explicitness, Level, Term};
use crate::frontend::resolve::DefId;

const INDUCTIVE_BASE: u32 = 0x0600_0000;

pub fn inductive_gid(index: u32) -> DefId {
    DefId(INDUCTIVE_BASE + index)
}

/// Declarative description of an inductive family before registration.
#[derive(Debug, Clone)]
pub struct InductiveDecl {
    pub family: DefId,
    pub params: Vec<Binder>,
    pub indices: Vec<Binder>,
    pub constructors: Vec<ConstructorDecl>,
}

#[derive(Debug, Clone)]
pub struct ConstructorDecl {
    pub name: String,
    pub ty: Term,
    pub result: Term,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Polarity {
    Positive,
    Negative,
}

/// Checks strict positivity of every constructor in a family declaration.
pub fn check_family(decl: &InductiveDecl) -> Result<(), String> {
    for ctor in &decl.constructors {
        let (arg_tys, result) = ctor_arg_types(&ctor.ty);
        for arg_ty in arg_tys {
            check_strict_positivity(decl.family, &arg_ty)?;
        }
        check_polarity(decl.family, &result, Polarity::Positive)?;
    }
    Ok(())
}

/// A family occurrence is legal only in positive position.
pub fn check_strict_positivity(family: DefId, ty: &Term) -> Result<(), String> {
    check_polarity(family, ty, Polarity::Positive)
}

fn check_polarity(family: DefId, ty: &Term, pol: Polarity) -> Result<(), String> {
    match ty {
        Term::Global(id) => {
            if *id == family && pol == Polarity::Negative {
                Err(format!(
                    "inductive family `{family:?}` occurs negatively (non-strict positivity)"
                ))
            } else {
                Ok(())
            }
        }
        Term::Pi { binder, body } => {
            check_polarity(family, binder.ty(), Polarity::Negative)?;
            check_polarity(family, body, Polarity::Positive)
        }
        Term::App { fun, arg } => {
            check_polarity(family, fun, pol)?;
            check_polarity(family, arg, pol)
        }
        Term::Let { binder, value, body } => {
            check_polarity(family, binder.ty(), Polarity::Negative)?;
            check_polarity(family, value, Polarity::Positive)?;
            check_polarity(family, body, Polarity::Positive)
        }
        Term::Var(_)
        | Term::Universe(_)
        | Term::I32(_)
        | Term::Bool(_)
        | Term::LitInt { .. }
        | Term::LitFloat { .. }
        | Term::LitStr(_)
        | Term::Unit
        | Term::Meta(_)
        | Term::Error
        | Term::Refinement { .. }
        | Term::Admit { .. }
        | Term::Computation { .. } => Ok(()),
        Term::Lam { binder, body } => {
            check_polarity(family, binder.ty(), Polarity::Negative)?;
            check_polarity(family, body, Polarity::Positive)
        }
        Term::DataCtor { type_def, args, .. } => {
            if *type_def == family && pol == Polarity::Negative {
                return Err(format!(
                    "inductive family `{family:?}` occurs negatively in constructor"
                ));
            }
            for arg in args {
                check_polarity(family, arg, Polarity::Positive)?;
            }
            Ok(())
        }
        Term::DataProj { value, .. } => check_polarity(family, value, Polarity::Positive),
        Term::DataMatch { scrutinee, arms, .. } => {
            check_polarity(family, scrutinee, Polarity::Positive)?;
            for arm in arms {
                check_polarity(family, &arm.body, Polarity::Positive)?;
            }
            Ok(())
        }
        Term::Break
        | Term::ArrayLit { .. }
        | Term::ArrayGet { .. }
        | Term::ArraySet { .. }
        | Term::AddrOf { .. }
        | Term::Deref { .. }
        | Term::Len { .. }
        | Term::While { .. }
        | Term::Loop { .. }
        | Term::For { .. }
        | Term::Assign { .. }
        | Term::HeapAlloc { .. }
        | Term::HeapDealloc { .. }
        | Term::HeapRealloc { .. }
        | Term::MatrixNew { .. }
        | Term::MatrixToArray { .. }
        | Term::MatrixDrop { .. }
        | Term::Quant { .. } => Ok(()),
    }
}

/// Splits `App…(Global family) args…` into parameters and indices.
pub fn family_instance_parts(
    ty: &Term,
    family: DefId,
    param_count: usize,
    index_count: usize,
) -> Option<(Vec<Term>, Vec<Term>)> {
    let mut args = Vec::new();
    let mut cur = ty.clone();
    while let Term::App { fun, arg } = cur {
        args.push(*arg);
        cur = *fun;
    }
    if cur != Term::Global(family) || args.len() != param_count + index_count {
        return None;
    }
    args.reverse();
    let (params, indices) = args.split_at(param_count);
    Some((params.to_vec(), indices.to_vec()))
}

/// `family` applied to parameters then indices: e.g. `Vec a n`.
pub fn apply_family(family: DefId, params: &[Term], indices: &[Term]) -> Term {
    let mut t = Term::Global(family);
    for arg in params.iter().chain(indices.iter()) {
        t = Term::App {
            fun: Box::new(t),
            arg: Box::new(arg.clone()),
        };
    }
    t
}

/// Substitute family parameter binders with concrete arguments from a family instance.
pub fn subst_family_params(info: &InductiveInfo, params: &[Term], ty: &Term) -> Term {
    let mut out = ty.clone();
    for (binder, param) in info.params.iter().zip(params.iter()) {
        out = out.subst(binder.level, param);
    }
    out
}

/// Explicit `Pi` domains of a constructor type before its result (implicit prefix skipped).
pub fn ctor_arg_types(ty: &Term) -> (Vec<Term>, Term) {
    let mut cur = ty.clone();
    loop {
        match &cur {
            Term::Pi { binder, body }
                if binder.explicitness != Explicitness::Explicit =>
            {
                cur = body.as_ref().clone();
            }
            _ => break,
        }
    }
    let mut args = Vec::new();
    loop {
        match cur {
            Term::Pi { binder, body }
                if binder.explicitness == Explicitness::Explicit =>
            {
                args.push(*binder.ty);
                cur = body.subst(binder.level, &Term::Var(binder.level));
            }
            other => return (args, other),
        }
    }
}

/// Registers an inductive family after positivity and metadata checks.
pub fn register_family(
    globals: &mut GlobalEnv,
    data: &mut DataEnv,
    decl: InductiveDecl,
) -> Result<(), String> {
    check_family(&decl)?;
    let family_ty = family_sort(&decl.params, &decl.indices);
    globals.insert_type(decl.family, family_ty);
    let info = InductiveInfo {
        params: decl.params,
        indices: decl.indices,
        constructors: decl
            .constructors
            .into_iter()
            .map(|c| InductiveCtor {
                name: c.name,
                ty: c.ty,
                result: c.result,
            })
            .collect(),
    };
    data.inductives.insert(decl.family, info);
    Ok(())
}

fn family_sort(params: &[Binder], indices: &[Binder]) -> Term {
    let mut ty = Term::ty();
    for binder in indices.iter().rev() {
        ty = Term::Pi {
            binder: binder.clone(),
            body: Box::new(ty),
        };
    }
    for binder in params.iter().rev() {
        ty = Term::Pi {
            binder: binder.clone(),
            body: Box::new(ty),
        };
    }
    ty
}

pub mod seed {
    use super::*;
    use crate::core::term::Relevance;
    pub struct NatSeed {
        pub family: DefId,
        pub add: DefId,
    }

    pub struct VecSeed {
        pub family: DefId,
        pub append: DefId,
    }

    pub fn register_nat(globals: &mut GlobalEnv, data: &mut DataEnv) -> Result<NatSeed, String> {
        let family = inductive_gid(0);
        let nat = Term::Global(family);
        let zero_result = nat.clone();
        let succ_ty = Term::Pi {
            binder: Binder::new("n", Level(0), nat.clone()),
            body: Box::new(nat.clone()),
        };
        let decl = InductiveDecl {
            family,
            params: vec![],
            indices: vec![],
            constructors: vec![
                ConstructorDecl {
                    name: "Zero".into(),
                    ty: nat.clone(),
                    result: zero_result,
                },
                ConstructorDecl {
                    name: "Succ".into(),
                    ty: succ_ty,
                    result: nat.clone(),
                },
            ],
        };
        register_family(globals, data, decl)?;

        let add = inductive_gid(1);
        let add_ty = Term::Pi {
            binder: Binder::new("n", Level(0), nat.clone()),
            body: Box::new(Term::Pi {
                binder: Binder::new("m", Level(1), nat.clone()),
                body: Box::new(nat),
            }),
        };
        globals.insert_type(add, add_ty);

        Ok(NatSeed { family, add })
    }

    pub fn register_vec(globals: &mut GlobalEnv, data: &mut DataEnv) -> Result<VecSeed, String> {
        let nat_family = inductive_gid(0);
        let family = inductive_gid(2);
        let nat = Term::Global(nat_family);

        let mut a_binder = Binder::new("a", Level(0), Term::ty());
        a_binder.explicitness = Explicitness::Implicit;
        a_binder.relevance = Relevance::Erased;

        let zero_index = zero_ctor(nat_family);
        let vec_an = |a: Term, n: Term| apply_family(family, &[a], &[n]);

        let a = Term::Var(Level(0));
        let n = Term::Var(Level(1));

        let nil_result = vec_an(a.clone(), zero_index.clone());
        let nil_ty = Term::Pi {
            binder: a_binder.clone(),
            body: Box::new(nil_result.clone()),
        };

        let succ_index = Term::DataCtor {
            type_def: nat_family,
            variant: 1,
            args: vec![n.clone()],
        };
        let cons_result = vec_an(a.clone(), succ_index);
        let cons_ty = Term::Pi {
            binder: a_binder.clone(),
            body: Box::new(Term::Pi {
                binder: Binder::new("n", Level(1), nat.clone()),
                body: Box::new(Term::Pi {
                    binder: Binder::new("head", Level(2), a.clone()),
                    body: Box::new(Term::Pi {
                        binder: Binder::new("tail", Level(3), vec_an(a.clone(), n.clone())),
                        body: Box::new(cons_result.clone()),
                    }),
                }),
            }),
        };

        let decl = InductiveDecl {
            family,
            params: vec![a_binder],
            indices: vec![Binder::new("n", Level(1), nat.clone())],
            constructors: vec![
                ConstructorDecl {
                    name: "Nil".into(),
                    ty: nil_ty,
                    result: nil_result,
                },
                ConstructorDecl {
                    name: "Cons".into(),
                    ty: cons_ty,
                    result: cons_result,
                },
            ],
        };
        register_family(globals, data, decl)?;

        let append = inductive_gid(3);
        let a = Term::Var(Level(0));
        let n = Term::Var(Level(1));
        let m = Term::Var(Level(2));
        let add_nm = Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(inductive_gid(1))),
                arg: Box::new(n.clone()),
            }),
            arg: Box::new(m.clone()),
        };
        let mut n_binder = Binder::new("n", Level(1), nat.clone());
        n_binder.explicitness = Explicitness::Implicit;
        n_binder.relevance = Relevance::Erased;
        let mut m_binder = Binder::new("m", Level(2), nat.clone());
        m_binder.explicitness = Explicitness::Implicit;
        m_binder.relevance = Relevance::Erased;
        let append_ty = Term::Pi {
            binder: {
                let mut b = Binder::new("a", Level(0), Term::ty());
                b.explicitness = Explicitness::Implicit;
                b.relevance = Relevance::Erased;
                b
            },
            body: Box::new(Term::Pi {
                binder: n_binder,
                body: Box::new(Term::Pi {
                    binder: m_binder,
                    body: Box::new(Term::Pi {
                        binder: Binder::new("xs", Level(3), vec_an(a.clone(), n.clone())),
                        body: Box::new(Term::Pi {
                            binder: Binder::new("ys", Level(4), vec_an(a.clone(), m.clone())),
                            body: Box::new(vec_an(a, add_nm)),
                        }),
                    }),
                }),
            }),
        };
        globals.insert_type(append, append_ty);

        Ok(VecSeed { family, append })
    }

    pub fn cons_ctor(
        vec_family: DefId,
        nat_family: DefId,
        n_pred: Term,
        head: Term,
        tail: Term,
    ) -> Term {
        Term::DataCtor {
            type_def: vec_family,
            variant: 1,
            args: vec![n_pred, head, tail],
        }
    }

    pub fn nil_ctor(vec_family: DefId) -> Term {
        Term::DataCtor {
            type_def: vec_family,
            variant: 0,
            args: vec![],
        }
    }

    pub fn append_term(
        nat_family: DefId,
        vec_family: DefId,
        add_gid: DefId,
        append_gid: DefId,
    ) -> Term {
        let a = Term::Var(Level(0));
        let n = Term::Var(Level(1));
        let m = Term::Var(Level(2));
        let xs = Term::Var(Level(3));
        let ys = Term::Var(Level(4));
        let nat = Term::Global(nat_family);
        let vec_an = |a: Term, idx: Term| apply_family(vec_family, &[a], &[idx]);

        let append_tail_ys = Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(append_gid)),
                arg: Box::new(Term::Var(Level(2))),
            }),
            arg: Box::new(ys.clone()),
        };

        let cons_body = cons_ctor(
            vec_family,
            nat_family,
            Term::App {
                fun: Box::new(Term::App {
                    fun: Box::new(Term::Global(add_gid)),
                    arg: Box::new(Term::Var(Level(0))),
                }),
                arg: Box::new(m.clone()),
            },
            Term::Var(Level(1)),
            append_tail_ys,
        );

        let match_xs = Term::DataMatch {
            scrutinee: Box::new(xs),
            enum_def: vec_family,
            arms: vec![
                crate::core::term::MatchArm {
                    variant_index: 0,
                    body: ys,
                },
                crate::core::term::MatchArm {
                    variant_index: 1,
                    body: cons_body,
                },
            ],
        };

        let mut a_binder = Binder::new("a", Level(0), Term::ty());
        a_binder.explicitness = Explicitness::Implicit;
        a_binder.relevance = Relevance::Erased;

        Term::Lam {
            binder: a_binder,
            body: Box::new(Term::Lam {
                binder: Binder::new("n", Level(1), nat.clone()),
                body: Box::new(Term::Lam {
                    binder: Binder::new("m", Level(2), nat.clone()),
                    body: Box::new(Term::Lam {
                        binder: Binder::new("xs", Level(3), vec_an(a.clone(), n)),
                        body: Box::new(Term::Lam {
                            binder: Binder::new("ys", Level(4), vec_an(a, m)),
                            body: Box::new(match_xs),
                        }),
                    }),
                }),
            }),
        }
    }

    pub fn register_append_value(
        globals: &mut GlobalEnv,
        nat_family: DefId,
        vec_family: DefId,
        add_gid: DefId,
        append: DefId,
    ) {
        globals.insert_value(
            append,
            append_term(nat_family, vec_family, add_gid, append),
        );
    }

    pub fn zero_ctor(nat_family: DefId) -> Term {
        Term::DataCtor {
            type_def: nat_family,
            variant: 0,
            args: vec![],
        }
    }

    pub fn succ_ctor(nat_family: DefId, pred: Term) -> Term {
        Term::DataCtor {
            type_def: nat_family,
            variant: 1,
            args: vec![pred],
        }
    }

    pub fn nat_add_term(nat_family: DefId, add_gid: DefId) -> Term {
        let n = Term::Var(Level(0));
        let m = Term::Var(Level(1));
        let nat = Term::Global(nat_family);
        let zero = zero_ctor(nat_family);
        Term::Lam {
            binder: Binder::new("n", Level(0), nat.clone()),
            body: Box::new(Term::Lam {
                binder: Binder::new("m", Level(1), nat.clone()),
                body: Box::new(Term::DataMatch {
                    scrutinee: Box::new(n),
                    enum_def: nat_family,
                    arms: vec![
                        crate::core::term::MatchArm {
                            variant_index: 0,
                            body: m,
                        },
                        crate::core::term::MatchArm {
                            variant_index: 1,
                            body: succ_ctor(
                                nat_family,
                                Term::App {
                                    fun: Box::new(Term::Global(add_gid)),
                                    arg: Box::new(Term::Var(Level(0))),
                                },
                            ),
                        },
                    ],
                }),
            }),
        }
    }

    pub fn register_nat_add_value(globals: &mut GlobalEnv, nat_family: DefId, add: DefId) {
        let term = nat_add_term(nat_family, add);
        globals.insert_value(add, term);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::globals::{prim, GlobalEnv};

    #[test]
    fn rejects_negative_occurrence() {
        let family = inductive_gid(99);
        let bad = Term::Pi {
            binder: Binder::new("f", Level(0), Term::Pi {
                binder: Binder::new("x", Level(1), Term::Global(family)),
                body: Box::new(Term::Global(prim::I32)),
            }),
            body: Box::new(Term::Global(family)),
        };
        assert!(check_strict_positivity(family, &bad).is_err());
    }

    #[test]
    fn nat_family_registers() {
        let mut globals = GlobalEnv::with_primitives();
        let mut data = DataEnv::default();
        let nat = seed::register_nat(&mut globals, &mut data).expect("nat");
        assert!(data.inductive(nat.family).is_some());
        assert_eq!(data.inductive(nat.family).unwrap().constructors.len(), 2);
    }

    #[test]
    fn vec_family_registers() {
        let mut globals = GlobalEnv::with_primitives();
        let mut data = DataEnv::default();
        seed::register_nat(&mut globals, &mut data).expect("nat");
        let vec = seed::register_vec(&mut globals, &mut data).expect("vec");
        assert!(data.inductive(vec.family).is_some());
    }
}
