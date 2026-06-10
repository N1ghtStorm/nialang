//! VC → SMT-LIB encoding (phase 11).

use std::collections::{HashMap, HashSet};

use crate::core::globals::prim;
use crate::core::term::{Level, Term};
use crate::elab::{BinOp, CmpOp, RuntimeBuiltin, RuntimeTy};
use crate::frontend::resolve::DefId;

use super::VcAssumption;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtSort {
    Int,
    Bool,
}

#[derive(Debug, Clone)]
pub struct SmtContext<'a> {
    builtins: &'a HashMap<DefId, RuntimeBuiltin>,
    var_names: HashMap<Level, String>,
}

impl<'a> SmtContext<'a> {
    pub fn new(
        builtins: &'a HashMap<DefId, RuntimeBuiltin>,
        assumptions: &[VcAssumption],
    ) -> Self {
        let mut var_names = HashMap::new();
        for a in assumptions {
            var_names.insert(a.level, sanitize_name(&a.name));
        }
        Self {
            builtins,
            var_names,
        }
    }

    fn var_name(&self, level: Level) -> String {
        self.var_names
            .get(&level)
            .cloned()
            .unwrap_or_else(|| format!("v{}", level.0))
    }

    fn lookup_builtin(&self, id: DefId) -> Option<&RuntimeBuiltin> {
        self.builtins.get(&id)
    }

    fn is_if(&self, id: DefId) -> bool {
        matches!(self.lookup_builtin(id), Some(RuntimeBuiltin::If(_)))
    }

    fn cmp_op(&self, id: DefId) -> Option<CmpOp> {
        match self.lookup_builtin(id) {
            Some(RuntimeBuiltin::Cmp(op, _)) => Some(*op),
            _ => None,
        }
    }

    fn bin_op(&self, id: DefId) -> Option<BinOp> {
        match self.lookup_builtin(id) {
            Some(RuntimeBuiltin::BinOp(op, _)) => Some(*op),
            _ => None,
        }
    }
}

/// Builds an SMT-LIB script that checks whether `goal` follows from `assumptions`.
pub fn encode_problem(
    ctx: &SmtContext<'_>,
    assumptions: &[VcAssumption],
    goal: &Term,
) -> Result<String, String> {
    let goal = subst_assumptions(goal, assumptions);
    let mut script = String::from("(set-logic QF_LIA)\n");
    let mut declared = HashSet::new();

    for level in collect_var_levels(&goal) {
        let name = ctx.var_name(level);
        if declared.insert(name.clone()) {
            script.push_str(&format!("(declare-fun {name} () Int)\n"));
        }
    }

    for a in assumptions {
        let name = ctx.var_name(a.level);
        if let Some(lit) = encode_int_literal(&a.value) {
            script.push_str(&format!("(assert (= {name} {lit}))\n"));
        } else if let Ok(other) = encode_int(ctx, &a.value) {
            script.push_str(&format!("(assert (= {name} {other}))\n"));
        }
    }

    let goal_smt = encode_bool(ctx, &goal)?;
    script.push_str(&format!("(assert (not {goal_smt}))\n"));
    script.push_str("(check-sat)\n");
    Ok(script)
}

fn subst_assumptions(prop: &Term, assumptions: &[VcAssumption]) -> Term {
    let mut t = prop.clone();
    for a in assumptions {
        t = t.subst(a.level, &a.value);
    }
    t
}

fn collect_var_levels(term: &Term) -> HashSet<Level> {
    let mut out = HashSet::new();
    collect_var_levels_rec(term, &mut out);
    out
}

fn collect_var_levels_rec(term: &Term, out: &mut HashSet<Level>) {
    match term {
        Term::Var(level) => {
            out.insert(*level);
        }
        Term::App { fun, arg } => {
            collect_var_levels_rec(fun, out);
            collect_var_levels_rec(arg, out);
        }
        Term::Let { value, body, .. } => {
            collect_var_levels_rec(value, out);
            collect_var_levels_rec(body, out);
        }
        Term::Lam { body, .. } => collect_var_levels_rec(body, out),
        Term::LitInt { .. }
        | Term::Bool(_)
        | Term::Global(_)
        | Term::Unit
        | Term::I32(_)
        | Term::LitStr(_)
        | Term::LitFloat { .. } => {}
        other => {
            if let Some((_, args)) = super::unfold_call(term) {
                for arg in args {
                    collect_var_levels_rec(&arg, out);
                }
            }
            let _ = other;
        }
    }
}

fn sanitize_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() || out.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    out
}

fn encode_int_literal(term: &Term) -> Option<String> {
    match term {
        Term::LitInt { value, ty } if is_int_prim(*ty) => Some(format!("{value}")),
        Term::I32(v) => Some(format!("{v}")),
        _ => None,
    }
}

fn is_int_prim(id: DefId) -> bool {
    matches!(
        id,
        prim::I8
            | prim::U8
            | prim::I16
            | prim::U16
            | prim::I32
            | prim::I64
            | prim::U64
            | prim::I128
            | prim::U128
    )
}

fn encode_int(ctx: &SmtContext<'_>, term: &Term) -> Result<String, String> {
    if let Some(lit) = encode_int_literal(term) {
        return Ok(lit);
    }
    match term {
        Term::Var(level) => Ok(ctx.var_name(*level)),
        Term::App { fun, arg } => {
            let Term::App { fun: f2, arg: l } = fun.as_ref() else {
                return Err(format!("unsupported integer term: `{term:?}`"));
            };
            let Term::Global(id) = f2.as_ref() else {
                return Err(format!("unsupported integer term: `{term:?}`"));
            };
            let l_s = encode_int(ctx, l)?;
            let r_s = encode_int(ctx, arg)?;
            let op = match ctx.bin_op(*id) {
                Some(BinOp::Add) => "+",
                Some(BinOp::Sub) => "-",
                Some(BinOp::Mul) => "*",
                _ => return Err(format!("unsupported integer builtin: `{id:?}`")),
            };
            Ok(format!("({op} {l_s} {r_s})"))
        }
        _ => Err(format!("unsupported integer term: `{term:?}`")),
    }
}

fn encode_bool(ctx: &SmtContext<'_>, term: &Term) -> Result<String, String> {
    match decode_bool(ctx, term)? {
        BoolForm::True => Ok("true".into()),
        BoolForm::False => Ok("false".into()),
        BoolForm::Not(inner) => {
            let inner_s = encode_bool_form(ctx, &inner)?;
            Ok(format!("(not {inner_s})"))
        }
        BoolForm::And(parts) => {
            if parts.is_empty() {
                return Ok("true".into());
            }
            if parts.len() == 1 {
                return encode_bool_form(ctx, &parts[0]);
            }
            let encoded: Result<Vec<_>, _> =
                parts.iter().map(|p| encode_bool_form(ctx, p)).collect();
            Ok(format!("(and {})", encoded?.join(" ")))
        }
        BoolForm::Eq(l, r) => {
            let l_s = encode_int(ctx, &l)?;
            let r_s = encode_int(ctx, &r)?;
            Ok(format!("(= {l_s} {r_s})"))
        }
        BoolForm::Ne(l, r) => {
            let l_s = encode_int(ctx, &l)?;
            let r_s = encode_int(ctx, &r)?;
            Ok(format!("(distinct {l_s} {r_s})"))
        }
        BoolForm::Lt(l, r) => {
            let l_s = encode_int(ctx, &l)?;
            let r_s = encode_int(ctx, &r)?;
            Ok(format!("(< {l_s} {r_s})"))
        }
        BoolForm::Gt(l, r) => {
            let l_s = encode_int(ctx, &l)?;
            let r_s = encode_int(ctx, &r)?;
            Ok(format!("(> {l_s} {r_s})"))
        }
    }
}

fn encode_bool_form(ctx: &SmtContext<'_>, form: &BoolForm) -> Result<String, String> {
    match form {
        BoolForm::True => Ok("true".into()),
        BoolForm::False => Ok("false".into()),
        BoolForm::Not(inner) => {
            let inner_s = encode_bool_form(ctx, inner)?;
            Ok(format!("(not {inner_s})"))
        }
        BoolForm::And(parts) => {
            if parts.is_empty() {
                return Ok("true".into());
            }
            if parts.len() == 1 {
                return encode_bool_form(ctx, &parts[0]);
            }
            let encoded: Result<Vec<_>, _> =
                parts.iter().map(|p| encode_bool_form(ctx, p)).collect();
            Ok(format!("(and {})", encoded?.join(" ")))
        }
        BoolForm::Eq(l, r) => {
            let l_s = encode_int(ctx, l)?;
            let r_s = encode_int(ctx, r)?;
            Ok(format!("(= {l_s} {r_s})"))
        }
        BoolForm::Ne(l, r) => {
            let l_s = encode_int(ctx, l)?;
            let r_s = encode_int(ctx, r)?;
            Ok(format!("(distinct {l_s} {r_s})"))
        }
        BoolForm::Lt(l, r) => {
            let l_s = encode_int(ctx, l)?;
            let r_s = encode_int(ctx, r)?;
            Ok(format!("(< {l_s} {r_s})"))
        }
        BoolForm::Gt(l, r) => {
            let l_s = encode_int(ctx, l)?;
            let r_s = encode_int(ctx, r)?;
            Ok(format!("(> {l_s} {r_s})"))
        }
    }
}

#[derive(Debug, Clone)]
enum BoolForm {
    True,
    False,
    Not(Box<BoolForm>),
    And(Vec<BoolForm>),
    Eq(Term, Term),
    Ne(Term, Term),
    Lt(Term, Term),
    Gt(Term, Term),
}

fn decode_bool(ctx: &SmtContext<'_>, term: &Term) -> Result<BoolForm, String> {
    match term {
        Term::Bool(true) => Ok(BoolForm::True),
        Term::Bool(false) => Ok(BoolForm::False),
        _ => {
            if let Some(inner) = decode_bool_not(ctx, term) {
                if let Ok(BoolForm::Eq(l, r)) = decode_bool(ctx, inner) {
                    return Ok(BoolForm::Ne(l, r));
                }
                return Ok(BoolForm::Not(Box::new(decode_bool(ctx, inner)?)));
            }
            if let Some((a, b)) = decode_bool_and(ctx, term) {
                if let (Some(lt), Some(gt)) = (
                    decode_bool_not(ctx, a).and_then(|t| decode_cmp(ctx, t, CmpOp::Lt).ok().flatten()),
                    decode_bool_not(ctx, b).and_then(|t| decode_cmp(ctx, t, CmpOp::Gt).ok().flatten()),
                ) {
                    if lt.0 == gt.0 && lt.1 == gt.1 {
                        return Ok(BoolForm::Eq(lt.0, lt.1));
                    }
                }
                return Ok(BoolForm::And(vec![
                    decode_bool(ctx, a)?,
                    decode_bool(ctx, b)?,
                ]));
            }
            if let Some((l, r)) = decode_cmp(ctx, term, CmpOp::Lt)? {
                return Ok(BoolForm::Lt(l, r));
            }
            if let Some((l, r)) = decode_cmp(ctx, term, CmpOp::Gt)? {
                return Ok(BoolForm::Gt(l, r));
            }
            Err(format!("unsupported boolean VC term: `{term:?}`"))
        }
    }
}

fn decode_bool_not<'a>(ctx: &SmtContext<'_>, term: &'a Term) -> Option<&'a Term> {
    let (cond, then_b, else_b) = decode_if(ctx, term)?;
    if matches!(then_b, Term::Bool(false)) && matches!(else_b, Term::Bool(true)) {
        Some(cond)
    } else {
        None
    }
}

fn decode_bool_and<'a>(ctx: &SmtContext<'_>, term: &'a Term) -> Option<(&'a Term, &'a Term)> {
    let (a, b, else_b) = decode_if(ctx, term)?;
    if matches!(else_b, Term::Bool(false)) {
        Some((a, b))
    } else {
        None
    }
}

fn decode_if<'a>(
    ctx: &SmtContext<'_>,
    term: &'a Term,
) -> Option<(&'a Term, &'a Term, &'a Term)> {
    let Term::App { fun, arg: else_b } = term else {
        return None;
    };
    let Term::App { fun: fun2, arg: cond } = fun.as_ref() else {
        return None;
    };
    let Term::Global(id) = fun2.as_ref() else {
        return None;
    };
    if !ctx.is_if(*id) {
        return None;
    }
    let Term::App { fun: fun3, arg: else_leaf } = else_b.as_ref() else {
        return None;
    };
    let Term::App { fun: fun4, arg: then_t } = fun3.as_ref() else {
        return None;
    };
    let Term::Global(id2) = fun4.as_ref() else {
        return None;
    };
    if !ctx.is_if(*id2) {
        return None;
    }
    Some((cond, then_t, else_leaf))
}

fn decode_cmp<'a>(
    ctx: &SmtContext<'_>,
    term: &'a Term,
    expected: CmpOp,
) -> Result<Option<(Term, Term)>, String> {
    let Term::App { fun, arg: r } = term else {
        return Ok(None);
    };
    let Term::App { fun: f2, arg: l } = fun.as_ref() else {
        return Ok(None);
    };
    let Term::Global(id) = f2.as_ref() else {
        return Ok(None);
    };
    if ctx.cmp_op(*id) != Some(expected) {
        return Ok(None);
    }
    if !matches!(
        ctx.lookup_builtin(*id),
        Some(RuntimeBuiltin::Cmp(_, RuntimeTy::I64))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::I32))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::I8))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::I16))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::U8))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::U16))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::U64))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::I128))
            | Some(RuntimeBuiltin::Cmp(_, RuntimeTy::U128))
    ) {
        return Err(format!("comparison on unsupported type: `{id:?}`"));
    }
    Ok(Some((l.as_ref().clone(), r.as_ref().clone())))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn ctx_for_module<'a>(
        module: &'a crate::elab::ElaboratedModule,
        assumptions: &'a [VcAssumption],
    ) -> SmtContext<'a> {
        SmtContext::new(&module.symbols.builtins, assumptions)
    }

    #[test]
    fn encodes_safe_div_guard_after_substitution() {
        use crate::driver::fixtures::read_fixture;
        use crate::driver::pipeline::{elaborate_resolved_module, parse_module, resolve_parsed_module};
        use crate::verify::collect_vcs;

        let src = read_fixture("examples/tests/ok_safe_div.nia");
        let parsed = parse_module(&src).expect("parse");
        let resolved = resolve_parsed_module(parsed).expect("resolve");
        let elaborated = elaborate_resolved_module(&resolved).expect("elab");
        let vcs = collect_vcs(&elaborated);
        let guard = vcs
            .goals
            .iter()
            .find(|g| g.label.contains("refinement guard"))
            .expect("guard vc");
        let ctx = ctx_for_module(&elaborated, &guard.assumptions);
        let script = encode_problem(&ctx, &guard.assumptions, &guard.prop).expect("encode");
        assert!(script.contains("(set-logic QF_LIA)"));
        assert!(script.contains("(check-sat)"));
        assert!(script.contains("distinct"));
    }
}
