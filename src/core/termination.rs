//! Structural termination checking (phase 7).

use std::collections::HashSet;

use crate::core::data::DataEnv;
use crate::core::globals::GlobalEnv;
use crate::core::inductive::{ctor_arg_types, family_instance_parts};
use crate::core::term::{Binder, Level, Term};
use crate::frontend::resolve::DefId;

/// Whether a function is treated as total or partial (`Div` stub).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Partiality {
    Total,
    /// Skips termination checking; intended for `Div` effect functions.
    Partial,
}

/// Termination metadata for one function.
#[derive(Debug, Clone)]
pub struct FnTerminationSpec {
    pub id: DefId,
    /// de Bruijn level of the structurally decreasing parameter.
    pub decreases: Level,
    pub partiality: Partiality,
    /// Other functions that may appear in place of self in recursive calls.
    pub mutual_group: Vec<DefId>,
}

impl FnTerminationSpec {
    pub fn is_recursive_target(&self, id: DefId) -> bool {
        id == self.id || self.mutual_group.contains(&id)
    }
}

/// Checks that `body` is structurally recursive on `spec.decreases`.
pub fn check_structural_termination(
    spec: &FnTerminationSpec,
    body: &Term,
    data: &DataEnv,
) -> Result<(), String> {
    if spec.partiality == Partiality::Partial {
        return Ok(());
    }

    let (binders, inner) = peel_lambdas(body);
    if spec.decreases.0 as usize >= binders.len() {
        return Err(format!(
            "decreasing argument level {:?} is out of range for function `{:?}`",
            spec.decreases, spec.id
        ));
    }

    let (family, match_arms) = match_on_decreases(inner, spec.decreases).ok_or_else(|| {
        format!(
            "function `{:?}` must `match` on its decreasing argument (level {:?})",
            spec.id, spec.decreases
        )
    })?;

    let info = data.inductive(family).ok_or_else(|| {
        format!("match scrutinee for `{:?}` is not a known inductive family", spec.id)
    })?;

    for arm in match_arms {
        let allowed = allowed_recursive_vars(arm.variant_index, family, info);
        let calls = collect_recursive_calls(spec, &arm.body);
        for args in calls {
            if args.is_empty() {
                return Err(format!(
                    "function `{:?}` has an invalid recursive call",
                    spec.id
                ));
            }
            let decreasing_arg = &args[0];
            if !is_allowed_var(decreasing_arg, &allowed) {
                return Err(format!(
                    "function `{:?}` recurses on a non-structurally-smaller argument in match arm {}",
                    spec.id, arm.variant_index
                ));
            }
        }
    }
    Ok(())
}

/// Returns `true` when `body` contains a call to any member of `spec`'s recursion group.
pub fn has_recursive_calls(spec: &FnTerminationSpec, body: &Term) -> bool {
    !collect_recursive_calls(spec, body).is_empty()
}

/// Builds a termination spec from surface metadata, if checking is required.
pub fn termination_spec_for_fn(
    id: DefId,
    params: &[(String, bool)],
    decreases: Option<&str>,
    partial: bool,
    mutual_group: &[DefId],
) -> Result<Option<FnTerminationSpec>, String> {
    let partiality = if partial {
        Partiality::Partial
    } else {
        Partiality::Total
    };
    let decreases = match decreases {
        Some(name) => {
            let idx = params
                .iter()
                .position(|(n, _)| n == name)
                .ok_or_else(|| format!("unknown decreasing argument `{name}`"))?;
            Level(idx as u32)
        }
        None => return Ok(None),
    };
    Ok(Some(FnTerminationSpec {
        id,
        decreases,
        partiality,
        mutual_group: mutual_group.to_vec(),
    }))
}

/// Checks prelude seed functions (`add`, `append`).
pub fn check_seed_terminations(globals: &GlobalEnv, data: &DataEnv) -> Result<(), String> {
    use crate::core::inductive::inductive_gid;

    let add = inductive_gid(1);
    let append = inductive_gid(3);
    let add_body = globals
        .value_of(add)
        .ok_or_else(|| "missing seed `add` value".to_string())?;
    let append_body = globals
        .value_of(append)
        .ok_or_else(|| "missing seed `append` value".to_string())?;

    check_structural_termination(
        &FnTerminationSpec {
            id: add,
            decreases: Level(0),
            partiality: Partiality::Total,
            mutual_group: vec![],
        },
        add_body,
        data,
    )?;

    check_structural_termination(
        &FnTerminationSpec {
            id: append,
            decreases: Level(3),
            partiality: Partiality::Total,
            mutual_group: vec![],
        },
        append_body,
        data,
    )?;

    Ok(())
}

/// Verifies one elaborated function body against its termination metadata.
pub fn check_fn_termination(
    spec: &FnTerminationSpec,
    body: &Term,
    data: &DataEnv,
) -> Result<(), String> {
    if spec.partiality == Partiality::Partial {
        return Ok(());
    }
    if !has_recursive_calls(spec, body) {
        return Ok(());
    }
    check_structural_termination(spec, body, data)
}

fn peel_lambdas<'a>(term: &'a Term) -> (Vec<Binder>, &'a Term) {
    let mut binders = Vec::new();
    let mut cur = term;
    while let Term::Lam { binder, body } = cur {
        binders.push(binder.clone());
        cur = body;
    }
    (binders, cur)
}

fn match_on_decreases<'a>(
    term: &'a Term,
    decreases: Level,
) -> Option<(DefId, &'a [crate::core::term::MatchArm])> {
    match term {
        Term::DataMatch { scrutinee, enum_def, arms } => {
            if matches!(scrutinee.as_ref(), Term::Var(l) if *l == decreases) {
                Some((*enum_def, arms))
            } else {
                None
            }
        }
        Term::Let { body, .. } => match_on_decreases(body, decreases),
        _ => None,
    }
}

fn allowed_recursive_vars(
    variant: u32,
    family: DefId,
    info: &crate::core::data::InductiveInfo,
) -> HashSet<Level> {
    let ctor = match info.constructors.get(variant as usize) {
        Some(c) => c,
        None => return HashSet::new(),
    };
    let fields = ctor_arg_types(&ctor.ty).0;
    let mut allowed = HashSet::new();
    for (i, field_ty) in fields.iter().enumerate() {
        if mentions_family(field_ty, family, info) {
            allowed.insert(Level(i as u32));
        }
    }
    allowed
}

fn mentions_family(ty: &Term, family: DefId, info: &crate::core::data::InductiveInfo) -> bool {
    let param_count = info.params.len();
    let index_count = info.indices.len();
    if param_count + index_count == 0 {
        return ty == &Term::Global(family);
    }
    family_instance_parts(ty, family, param_count, index_count).is_some()
}

fn is_allowed_var(term: &Term, allowed: &HashSet<Level>) -> bool {
    match term {
        Term::Var(level) => allowed.contains(level),
        _ => false,
    }
}

fn collect_recursive_calls(spec: &FnTerminationSpec, term: &Term) -> Vec<Vec<Term>> {
    let mut out = Vec::new();
    collect_recursive_calls_inner(spec, term, &mut out);
    out
}

fn collect_recursive_calls_inner(
    spec: &FnTerminationSpec,
    term: &Term,
    out: &mut Vec<Vec<Term>>,
) {
    match term {
        Term::App { fun, arg } => {
            if let Some(args) = unfold_call(fun, arg) {
                if let Term::Global(id) = &args[0] {
                    if spec.is_recursive_target(*id) {
                        out.push(args[1..].to_vec());
                    }
                }
            }
            collect_recursive_calls_inner(spec, fun, out);
            collect_recursive_calls_inner(spec, arg, out);
        }
        Term::Lam { body, .. } => collect_recursive_calls_inner(spec, body, out),
        Term::Let { value, body, .. } => {
            collect_recursive_calls_inner(spec, value, out);
            collect_recursive_calls_inner(spec, body, out);
        }
        Term::DataMatch { scrutinee, arms, .. } => {
            collect_recursive_calls_inner(spec, scrutinee, out);
            for arm in arms {
                collect_recursive_calls_inner(spec, &arm.body, out);
            }
        }
        Term::DataCtor { args, .. } => {
            for arg in args {
                collect_recursive_calls_inner(spec, arg, out);
            }
        }
        Term::DataProj { value, .. } => collect_recursive_calls_inner(spec, value, out),
        Term::Pi { body, .. } => collect_recursive_calls_inner(spec, body, out),
        Term::While { cond, body } => {
            collect_recursive_calls_inner(spec, cond, out);
            collect_recursive_calls_inner(spec, body, out);
        }
        Term::Loop { body } => collect_recursive_calls_inner(spec, body, out),
        Term::For { start, end, body, .. } => {
            collect_recursive_calls_inner(spec, start, out);
            collect_recursive_calls_inner(spec, end, out);
            collect_recursive_calls_inner(spec, body, out);
        }
        Term::Assign { target, value } => {
            collect_recursive_calls_inner(spec, target, out);
            collect_recursive_calls_inner(spec, value, out);
        }
        Term::ArrayLit { elems, .. } => {
            for elem in elems {
                collect_recursive_calls_inner(spec, elem, out);
            }
        }
        Term::ArrayGet { arr, index, .. } => {
            collect_recursive_calls_inner(spec, arr, out);
            collect_recursive_calls_inner(spec, index, out);
        }
        Term::ArraySet { arr, index, value, .. } => {
            collect_recursive_calls_inner(spec, arr, out);
            collect_recursive_calls_inner(spec, index, out);
            collect_recursive_calls_inner(spec, value, out);
        }
        Term::AddrOf { value, .. } => collect_recursive_calls_inner(spec, value, out),
        Term::Deref { ptr, .. } => collect_recursive_calls_inner(spec, ptr, out),
        Term::Len { arr, .. } => collect_recursive_calls_inner(spec, arr, out),
        Term::HeapAlloc { value, .. } | Term::HeapRealloc { value, .. } => {
            collect_recursive_calls_inner(spec, value, out);
        }
        Term::HeapDealloc { ptr, .. } => collect_recursive_calls_inner(spec, ptr, out),
        Term::MatrixNew { src, .. } => collect_recursive_calls_inner(spec, src, out),
        Term::MatrixToArray { matrix, .. } => collect_recursive_calls_inner(spec, matrix, out),
        Term::MatrixDrop { matrix, .. } => collect_recursive_calls_inner(spec, matrix, out),
        Term::Quant { args, .. } => {
            for arg in args {
                collect_recursive_calls_inner(spec, arg, out);
            }
        }
        Term::Var(_)
        | Term::Global(_)
        | Term::Universe(_)
        | Term::I32(_)
        | Term::Bool(_)
        | Term::LitInt { .. }
        | Term::LitFloat { .. }
        | Term::LitStr(_)
        | Term::Unit
        | Term::Meta(_)
        | Term::Error
        | Term::Break
        | Term::Refinement { .. }
        | Term::Admit { .. }
        | Term::Computation { .. } => {}
    }
}

fn unfold_call(fun: &Term, last_arg: &Term) -> Option<Vec<Term>> {
    let mut args = vec![last_arg.clone()];
    let mut cur = fun;
    loop {
        match cur {
            Term::Global(id) => {
                let mut full = vec![Term::Global(*id)];
                full.extend(args.into_iter().rev());
                return Some(full);
            }
            Term::App { fun, arg } => {
                args.push(arg.as_ref().clone());
                cur = fun;
            }
            _ => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::globals::GlobalEnv;
    use crate::core::inductive::seed;

    fn add_spec(nat: DefId, add: DefId) -> FnTerminationSpec {
        FnTerminationSpec {
            id: add,
            decreases: Level(0),
            partiality: Partiality::Total,
            mutual_group: vec![],
        }
    }

    fn append_spec(vec: DefId, append: DefId) -> FnTerminationSpec {
        FnTerminationSpec {
            id: append,
            decreases: Level(3),
            partiality: Partiality::Total,
            mutual_group: vec![],
        }
    }

    #[test]
    fn nat_add_passes_termination() {
        let mut globals = GlobalEnv::with_primitives();
        let mut data = DataEnv::default();
        let nat = seed::register_nat(&mut globals, &mut data).expect("nat");
        seed::register_nat_add_value(&mut globals, nat.family, nat.add);
        let body = globals.value_of(nat.add).expect("add value");
        check_structural_termination(&add_spec(nat.family, nat.add), body, &data).expect("add");
    }

    #[test]
    fn vec_append_passes_termination() {
        let mut globals = GlobalEnv::with_primitives();
        let mut data = DataEnv::default();
        let nat = seed::register_nat(&mut globals, &mut data).expect("nat");
        let vec = seed::register_vec(&mut globals, &mut data).expect("vec");
        seed::register_append_value(
            &mut globals,
            nat.family,
            vec.family,
            nat.add,
            vec.append,
        );
        let body = globals.value_of(vec.append).expect("append value");
        check_structural_termination(&append_spec(vec.family, vec.append), body, &data)
            .expect("append");
    }

    #[test]
    fn nonstructural_self_call_fails() {
        let mut globals = GlobalEnv::with_primitives();
        let mut data = DataEnv::default();
        let nat = seed::register_nat(&mut globals, &mut data).expect("nat");
        let bad = DefId(0x0700_0001);
        let n = Term::Var(Level(0));
        let m = Term::Var(Level(1));
        let nat_ty = Term::Global(nat.family);
        let body = Term::Lam {
            binder: Binder::new("n", Level(0), nat_ty.clone()),
            body: Box::new(Term::Lam {
                binder: Binder::new("m", Level(1), nat_ty),
                body: Box::new(Term::DataMatch {
                    scrutinee: Box::new(n.clone()),
                    enum_def: nat.family,
                    arms: vec![
                        crate::core::term::MatchArm {
                            variant_index: 0,
                            body: Term::App {
                                fun: Box::new(Term::Global(bad)),
                                arg: Box::new(n.clone()),
                            },
                        },
                        crate::core::term::MatchArm {
                            variant_index: 1,
                            body: m,
                        },
                    ],
                }),
            }),
        };
        let spec = FnTerminationSpec {
            id: bad,
            decreases: Level(0),
            partiality: Partiality::Total,
            mutual_group: vec![],
        };
        let err = check_structural_termination(&spec, &body, &data).unwrap_err();
        assert!(
            err.contains("non-structurally-smaller"),
            "unexpected error: {err}"
        );
    }
}
