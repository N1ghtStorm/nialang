//! Dependently-typed core calculus (phase 3 skeleton).

pub mod checker;
pub mod data;
pub mod effect;
pub mod quant;
pub mod inductive;
pub mod env;
pub mod globals;
pub mod meta;
pub mod nbe;
pub mod termination;
pub mod term;
pub mod unify;

pub use checker::{CheckResult, Checker, TypeError};
pub use effect::{join_effect, Effect, is_subeffect};
pub use quant::QuantKind;
pub use env::{Closure, EvalEnv, Neutral, TypingCtx, Value};
pub use data::{DataEnv, EnumInfo, InductiveInfo, StructInfo, VariantFields};
pub use inductive::{apply_family, check_family, check_strict_positivity, register_family};
pub use globals::{GlobalEnv, prim};
pub use nbe::{eval, is_def_eq, quote, whnf};
pub use meta::{MetaEnv, MetaId};
pub use term::{Binder, Explicitness, Level, Relevance, Term, UniverseLevel};
pub use termination::{
    check_structural_termination, has_recursive_calls, FnTerminationSpec, Partiality,
};
pub use unify::{intro_implicit, skip_implicit_pis, unify, UnifyError};

#[cfg(test)]
mod tests;
