//! Affine qubit usage tracking in `quant { }` and `quant fn` bodies (phase 13.1).

use crate::core::globals::prim;
use crate::core::quant::QuantKind;
use crate::core::term::Term;
use crate::elab::env::ElabEnv;
use crate::frontend::surface::Expr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum QubitState {
    Available,
    Measured,
}

pub fn is_qubit_type(ty: &Term) -> bool {
    matches!(ty, Term::Global(id) if *id == prim::QUBIT)
}

pub fn is_qubit_new(term: &Term) -> bool {
    matches!(term, Term::Quant { kind: QuantKind::QubitNew, .. })
}

/// Reject `let dst = src` when `src` is an existing qubit binding.
pub fn check_qubit_copy(env: &ElabEnv, init: &Expr, init_ty: &Term) -> Result<(), String> {
    if !env.in_qubit_affine_scope() || !is_qubit_type(init_ty) {
        return Ok(());
    }
    let Expr::Ident(src) = init else {
        return Ok(());
    };
    if is_qubit_new_binding(init) {
        return Ok(());
    }
    if env.qubit_state(src).is_some() {
        return Err(format!("cannot copy qubit `{src}`"));
    }
    Ok(())
}

fn is_qubit_new_binding(init: &Expr) -> bool {
    matches!(init, Expr::Call { name, args } if name == crate::nia_std::QUBIT && args.is_empty())
}

/// Register a freshly allocated qubit local.
pub fn register_qubit(env: &mut ElabEnv, name: &str) {
    if name == "_" {
        return;
    }
    if let Some(scope) = env.qubit_affine_mut() {
        scope.insert(name.to_string(), QubitState::Available);
    }
}

/// Require a qubit argument to still be available (not yet measured).
pub fn check_qubit_available(env: &ElabEnv, expr: &Expr) -> Result<(), String> {
    if !env.in_qubit_affine_scope() {
        return Ok(());
    }
    let Expr::Ident(name) = expr else {
        return Ok(());
    };
    match env.qubit_state(name) {
        None => Ok(()),
        Some(QubitState::Available) => Ok(()),
        Some(QubitState::Measured) => Err(format!("qubit `{name}` was already measured")),
    }
}

/// Mark a qubit consumed by `q_measure`.
pub fn mark_qubit_measured(env: &mut ElabEnv, expr: &Expr) -> Result<(), String> {
    check_qubit_available(env, expr)?;
    let Expr::Ident(name) = expr else {
        return Ok(());
    };
    if let Some(scope) = env.qubit_affine_mut() {
        if let Some(state) = scope.get_mut(name) {
            *state = QubitState::Measured;
        }
    }
    Ok(())
}
