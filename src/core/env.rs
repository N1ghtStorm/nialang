use crate::core::term::{Binder, Level, Term};

/// Typing context using de Bruijn levels.
#[derive(Debug, Clone, Default)]
pub struct TypingCtx {
    binders: Vec<Binder>,
}

impl TypingCtx {
    pub fn level(&self) -> Level {
        Level(self.binders.len() as u32)
    }

    pub fn len(&self) -> usize {
        self.binders.len()
    }

    pub fn lookup(&self, level: Level) -> Option<&Binder> {
        self.binders.get(level.0 as usize)
    }

    pub fn push(&mut self, binder: Binder) {
        self.binders.push(binder);
    }

    pub fn pop(&mut self) {
        self.binders.pop();
    }

    pub fn bind<R>(&mut self, name_hint: &str, ty: Term, f: impl FnOnce(&mut Self) -> R) -> R
    where
        R: Sized,
    {
        let level = self.level();
        let binder = Binder::new(name_hint, level, ty);
        self.push(binder);
        let out = f(self);
        self.pop();
        out
    }

    /// Binds one local per payload field for a match arm.
    pub fn bind_variant_fields<R>(
        &mut self,
        fields: &crate::core::data::VariantFields,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R
    where
        R: Sized,
    {
        match fields {
            crate::core::data::VariantFields::Unit => f(self),
            crate::core::data::VariantFields::Tuple(types)
            | crate::core::data::VariantFields::Struct(types) => {
                self.bind_many("_", types, f)
            }
        }
    }

    fn bind_many<R>(&mut self, prefix: &str, types: &[Term], f: impl FnOnce(&mut Self) -> R) -> R
    where
        R: Sized,
    {
        if types.is_empty() {
            return f(self);
        }
        let ty = types[0].clone();
        let name = format!("{prefix}{}", self.len());
        self.bind(&name, ty, |ctx| ctx.bind_many(prefix, &types[1..], f))
    }
}

/// Runtime values for NbE evaluation (parallel to typing levels).
#[derive(Debug, Clone)]
pub enum Value {
    Universe(u32),
    Pi {
        binder: Binder,
        body: Closure,
    },
    Lam {
        binder: Binder,
        body: Closure,
    },
    I32(i32),
    Bool(bool),
    LitInt {
        value: i128,
        ty: crate::frontend::resolve::DefId,
    },
    LitFloat {
        value: f64,
        ty: crate::frontend::resolve::DefId,
    },
    LitStr,
    Unit,
    Data {
        type_def: crate::frontend::resolve::DefId,
        variant: u32,
        args: Vec<Value>,
    },
    Neut(Neutral),
}

#[derive(Debug, Clone)]
pub enum Neutral {
    Var(Level),
    Global(crate::frontend::resolve::DefId),
    App {
        fun: Box<Neutral>,
        arg: Box<Value>,
    },
}

#[derive(Debug, Clone)]
pub struct Closure {
    pub env: EvalEnv,
    pub body: Term,
}

#[derive(Debug, Clone, Default)]
pub struct EvalEnv {
    values: Vec<Value>,
}

impl EvalEnv {
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn lookup(&self, level: Level) -> Option<&Value> {
        self.values.get(level.0 as usize)
    }

    pub fn push(&mut self, value: Value) {
        self.values.push(value);
    }

    pub fn pop(&mut self) {
        self.values.pop();
    }
}
