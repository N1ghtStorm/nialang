//! Verification-condition collection and SMT discharge (phases 10–11).

mod smt;
mod z3;

use crate::core::checker::Checker;
use crate::core::env::TypingCtx;
use crate::core::globals::GlobalEnv;
use crate::core::meta::MetaEnv;
use crate::core::term::{Level, Term};
use crate::elab::ElaboratedModule;

pub use smt::encode_problem;
pub use z3::{find_z3, SmtResult, Z3Config};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VcStatus {
    Pending,
    Admitted,
    Discharged,
}

#[derive(Debug, Clone)]
pub struct VcAssumption {
    pub name: String,
    pub level: Level,
    pub value: Term,
}

#[derive(Debug, Clone)]
pub struct VcGoal {
    pub label: String,
    pub prop: Term,
    pub status: VcStatus,
    pub assumptions: Vec<VcAssumption>,
}

#[derive(Debug, Clone, Default)]
pub struct VcModule {
    pub goals: Vec<VcGoal>,
    pub warnings: Vec<String>,
}

struct Collector<'a> {
    globals: &'a GlobalEnv,
    fn_name: &'a str,
    goals: Vec<VcGoal>,
    admitted: Vec<Term>,
    warnings: Vec<String>,
    locals: Vec<VcAssumption>,
}

/// Collects verification conditions from an elaborated module.
pub fn collect_vcs(module: &ElaboratedModule) -> VcModule {
    let mut goals = Vec::new();
    let mut warnings = Vec::new();
    for f in &module.fns {
        let mut collector = Collector {
            globals: &module.globals,
            fn_name: &f.name,
            goals: Vec::new(),
            admitted: Vec::new(),
            warnings: Vec::new(),
            locals: Vec::new(),
        };
        if let Some(req) = &f.requires {
            collector.push_vc(format!("requires `{}`", f.name), req.clone(), VcStatus::Pending);
        }
        collector.visit_term(&f.body);
        if let Some(ens) = &f.ensures {
            collector.push_vc(format!("ensures `{}`", f.name), ens.clone(), VcStatus::Pending);
        }
        collector.discharge_admitted();
        goals.extend(collector.goals);
        warnings.extend(collector.warnings);
    }
    VcModule { goals, warnings }
}

/// Tries to discharge pending goals with Z3.
pub fn discharge_vcs(module: &ElaboratedModule, vcs: &mut VcModule) -> Result<(), String> {
    let Some(z3_path) = find_z3() else {
        return Err(
            "Z3 not found in PATH (set Z3_PATH or install z3) — cannot discharge verification conditions"
                .into(),
        );
    };
    let config = Z3Config {
        z3_path,
        ..Z3Config::default()
    };
    for goal in &mut vcs.goals {
        if goal.status != VcStatus::Pending {
            continue;
        }
        let ctx = smt::SmtContext::new(&module.symbols.builtins, &goal.assumptions);
        let script = encode_problem(&ctx, &goal.assumptions, &goal.prop)?;
        match z3::solve(&script, &config)? {
            SmtResult::Unsat => goal.status = VcStatus::Discharged,
            SmtResult::Sat | SmtResult::Unknown => {}
        }
    }
    Ok(())
}

/// Collects VCs, discharges with SMT, and checks for remaining obligations.
pub fn verify_module(module: &ElaboratedModule) -> Result<(), String> {
    let mut vcs = collect_vcs(module);
    if let Err(e) = discharge_vcs(module, &mut vcs) {
        return Err(e);
    }
    for warning in &vcs.warnings {
        eprintln!("warning: {warning}");
    }
    let pending: Vec<_> = vcs
        .goals
        .iter()
        .filter(|g| g.status == VcStatus::Pending)
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let mut msg = format!("{} undischarged verification condition(s)", pending.len());
    for goal in pending.iter().take(3) {
        msg.push_str(&format!("\n  - {}: {:?}", goal.label, goal.prop));
        if !goal.assumptions.is_empty() {
            msg.push_str("\n    assumptions:");
            for a in &goal.assumptions {
                msg.push_str(&format!("\n      {}@{} = {:?}", a.name, a.level.0, a.value));
            }
        }
    }
    Err(msg)
}

/// Pretty-prints collected VCs for `--dump-vc`.
pub fn format_vcs(vcs: &VcModule) -> String {
    let mut out = String::from(";; nialang verification conditions\n");
    if vcs.goals.is_empty() {
        out.push_str(";; (no goals)\n");
        return out;
    }
    for (i, goal) in vcs.goals.iter().enumerate() {
        let status = match goal.status {
            VcStatus::Pending => "pending",
            VcStatus::Admitted => "admitted",
            VcStatus::Discharged => "discharged",
        };
        out.push_str(&format!(
            "\nVC[{i}] {status}: {}\n  prop: {:#?}\n",
            goal.label, goal.prop
        ));
        if !goal.assumptions.is_empty() {
            out.push_str("  assumptions:\n");
            for a in &goal.assumptions {
                out.push_str(&format!(
                    "    {}@{} = {:#?}\n",
                    a.name, a.level.0, a.value
                ));
            }
        }
    }
    if !vcs.warnings.is_empty() {
        out.push_str("\n;; warnings\n");
        for w in &vcs.warnings {
            out.push_str(&format!(";; {w}\n"));
        }
    }
    out
}

impl<'a> Collector<'a> {
    fn push_vc(&mut self, label: String, prop: Term, status: VcStatus) {
        self.goals.push(VcGoal {
            label,
            prop,
            status,
            assumptions: self.locals.clone(),
        });
    }

    fn discharge_admitted(&mut self) {
        let checker = Checker::new(self.globals);
        let ctx = TypingCtx::default();
        let metas = MetaEnv::default();
        for goal in &mut self.goals {
            if goal.status != VcStatus::Pending {
                continue;
            }
            if self
                .admitted
                .iter()
                .any(|admitted| checker.def_eq(&ctx, &metas, admitted, &goal.prop))
            {
                goal.status = VcStatus::Admitted;
            }
        }
    }

    fn visit_term(&mut self, term: &Term) {
        if let Some((head, args)) = unfold_call(term) {
            self.check_call_vcs(&head, &args);
        }
        match term {
            Term::Admit { prop } => {
                self.warnings.push(format!(
                    "admitted proof obligation in `{}` without verification: `{prop:?}`",
                    self.fn_name
                ));
                self.admitted.push((**prop).clone());
                self.push_vc(
                    format!("admit in `{}`", self.fn_name),
                    (**prop).clone(),
                    VcStatus::Admitted,
                );
            }
            Term::App { fun, arg } => {
                self.visit_term(fun);
                self.visit_term(arg);
            }
            Term::Let { binder, value, body } => {
                self.visit_term(value);
                if is_assumption_value(value) {
                    self.locals.push(VcAssumption {
                        name: binder.name_hint.clone(),
                        level: binder.level,
                        value: value.as_ref().clone(),
                    });
                    self.visit_term(body);
                    self.locals.pop();
                } else {
                    self.visit_term(body);
                }
            }
            Term::Lam { body, .. } => self.visit_term(body),
            Term::While { cond, body } => {
                self.visit_term(cond);
                self.visit_term(body);
            }
            Term::Loop { body } => self.visit_term(body),
            Term::For { start, end, body, .. } => {
                self.visit_term(start);
                self.visit_term(end);
                self.visit_term(body);
            }
            Term::Assign { target, value } => {
                self.visit_term(target);
                self.visit_term(value);
            }
            Term::DataCtor { args, .. } => {
                for arg in args {
                    self.visit_term(arg);
                }
            }
            Term::DataProj { value, .. } => self.visit_term(value),
            Term::DataMatch { scrutinee, arms, .. } => {
                self.visit_term(scrutinee);
                for arm in arms {
                    self.visit_term(&arm.body);
                }
            }
            Term::ArrayLit { elems, .. } => {
                for elem in elems {
                    self.visit_term(elem);
                }
            }
            Term::ArrayGet { arr, index, .. } => {
                self.visit_term(arr);
                self.visit_term(index);
            }
            Term::ArraySet { arr, index, value, .. } => {
                self.visit_term(arr);
                self.visit_term(index);
                self.visit_term(value);
            }
            Term::AddrOf { value, .. } => self.visit_term(value),
            Term::HeapAlloc { value, .. } | Term::HeapRealloc { value, .. } => {
                self.visit_term(value);
            }
            Term::Deref { ptr, .. } | Term::HeapDealloc { ptr, .. } => self.visit_term(ptr),
            Term::Len { arr, .. } => self.visit_term(arr),
            Term::MatrixNew { src, .. } => self.visit_term(src),
            Term::MatrixToArray { matrix, .. } | Term::MatrixDrop { matrix, .. } => {
                self.visit_term(matrix);
            }
            _ => {}
        }
    }

    fn check_call_vcs(&mut self, head: &Term, args: &[Term]) {
        let Term::Global(id) = head else {
            return;
        };
        let Some(mut fun_ty) = self.globals.type_of(*id).cloned() else {
            return;
        };
        for arg in args {
            fun_ty = skip_implicit_pis(fun_ty);
            let Term::Pi { binder, body } = fun_ty else {
                break;
            };
            if let Term::Refinement {
                binder: rb,
                pred,
            } = binder.ty()
            {
                let guard = pred.subst(rb.level, arg);
                self.push_vc(
                    format!("call `{}` refinement guard", self.fn_name),
                    guard,
                    VcStatus::Pending,
                );
            }
            fun_ty = body.subst(binder.level, arg);
        }
    }
}

fn is_assumption_value(value: &Term) -> bool {
    matches!(value, Term::LitInt { .. } | Term::Bool(_) | Term::I32(_))
}

fn skip_implicit_pis(mut ty: Term) -> Term {
    loop {
        match ty {
            Term::Pi {
                binder,
                body,
            } if binder.explicitness == crate::core::term::Explicitness::Implicit => {
                ty = body.subst(binder.level, &Term::Unit);
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests;

pub(crate) fn unfold_call(term: &Term) -> Option<(Term, Vec<Term>)> {
    let mut args = Vec::new();
    let mut cur = term;
    loop {
        match cur {
            Term::App { fun, arg } => {
                args.push(arg.as_ref().clone());
                cur = fun;
            }
            other => {
                if args.is_empty() {
                    return None;
                }
                args.reverse();
                return Some((other.clone(), args));
            }
        }
    }
}
