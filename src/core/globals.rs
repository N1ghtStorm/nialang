use std::collections::HashMap;

use crate::core::effect::Effect;
use crate::core::term::{Term, UniverseLevel};
use crate::frontend::resolve::DefId;

/// Built-in primitive type globals for Core.
pub mod prim {
    use super::DefId;

    pub const I8: DefId = DefId(0);
    pub const U8: DefId = DefId(1);
    pub const I16: DefId = DefId(2);
    pub const U16: DefId = DefId(3);
    pub const I32: DefId = DefId(4);
    pub const I64: DefId = DefId(5);
    pub const U64: DefId = DefId(6);
    pub const I128: DefId = DefId(7);
    pub const U128: DefId = DefId(8);
    pub const BOOL: DefId = DefId(9);
    pub const UNIT: DefId = DefId(10);
    pub const F16: DefId = DefId(11);
    pub const F32: DefId = DefId(12);
    pub const F64: DefId = DefId(13);
    pub const STRING: DefId = DefId(14);
    pub const QUBIT: DefId = DefId(15);
    pub const RESULT: DefId = DefId(16);
}

/// Global axioms used by the Core checker and NbE.
#[derive(Debug, Clone, Default)]
pub struct GlobalEnv {
    types: HashMap<DefId, Term>,
    values: HashMap<DefId, Term>,
    effects: HashMap<DefId, Effect>,
}

impl GlobalEnv {
    pub fn with_primitives() -> Self {
        let mut env = Self::default();
        for id in [
            prim::I8,
            prim::U8,
            prim::I16,
            prim::U16,
            prim::I32,
            prim::I64,
            prim::U64,
            prim::I128,
            prim::U128,
            prim::BOOL,
            prim::UNIT,
            prim::F16,
            prim::F32,
            prim::F64,
            prim::STRING,
            prim::QUBIT,
            prim::RESULT,
        ] {
            env.types.insert(id, Term::ty());
        }
        env
    }

    pub fn type_of(&self, id: DefId) -> Option<&Term> {
        self.types.get(&id)
    }

    pub fn value_of(&self, id: DefId) -> Option<&Term> {
        self.values.get(&id)
    }

    pub fn insert_type(&mut self, id: DefId, ty: Term) {
        self.types.insert(id, ty);
    }

    pub fn insert_value(&mut self, id: DefId, value: Term) {
        self.values.insert(id, value);
    }

    pub fn insert_effect(&mut self, id: DefId, effect: Effect) {
        self.effects.insert(id, effect);
    }

    pub fn effect_of(&self, id: DefId) -> Option<Effect> {
        self.effects.get(&id).copied()
    }

    pub fn update_fn_return_effect(&mut self, id: DefId, effect: Effect) {
        let Some(sig) = self.types.get(&id).cloned() else {
            return;
        };
        let updated = wrap_return_effect(sig, effect);
        self.types.insert(id, updated);
        self.effects.insert(id, effect);
    }

    pub fn universe_of_global(&self, id: DefId) -> Option<UniverseLevel> {
        match self.type_of(id)? {
            Term::Universe(u) => Some(*u),
            _ => None,
        }
    }
}

fn wrap_return_effect(sig: Term, effect: Effect) -> Term {
    let (prefix, ret) = peel_pi_prefix(sig);
    let result = match ret {
        Term::Computation { result, .. } => *result,
        other => other,
    };
    let wrapped = Term::computation(effect, result);
    rebuild_pi(prefix, wrapped)
}

fn peel_pi_prefix(mut sig: Term) -> (Vec<crate::core::term::Binder>, Term) {
    let mut binders = Vec::new();
    while let Term::Pi { binder, body } = sig {
        binders.push(binder);
        sig = *body;
    }
    (binders, sig)
}

fn rebuild_pi(binders: Vec<crate::core::term::Binder>, ret: Term) -> Term {
    let mut ty = ret;
    for binder in binders.into_iter().rev() {
        ty = Term::Pi {
            binder,
            body: Box::new(ty),
        };
    }
    ty
}
