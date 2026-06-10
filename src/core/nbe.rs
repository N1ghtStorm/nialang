use crate::core::env::{Closure, EvalEnv, Neutral, Value};
use crate::core::globals::GlobalEnv;
use crate::core::term::{Level, Term, UniverseLevel};

pub fn eval(env: &EvalEnv, globals: &GlobalEnv, term: &Term) -> Value {
    match term {
        Term::Var(level) => env
            .lookup(*level)
            .cloned()
            .unwrap_or_else(|| Value::Neut(Neutral::Var(*level))),
        Term::Global(id) => globals
            .value_of(*id)
            .map(|v| eval(env, globals, v))
            .unwrap_or_else(|| Value::Neut(Neutral::Global(*id))),
        Term::Universe(u) => Value::Universe(u.0),
        Term::I32(n) => Value::I32(*n),
        Term::Bool(b) => Value::Bool(*b),
        Term::LitInt { value, ty } => Value::LitInt {
            value: *value,
            ty: *ty,
        },
        Term::LitFloat { value, ty } => Value::LitFloat {
            value: *value,
            ty: *ty,
        },
        Term::LitStr(_) => Value::LitStr,
        Term::Unit => Value::Unit,
        Term::DataCtor {
            type_def,
            variant,
            args,
        } => Value::Data {
            type_def: *type_def,
            variant: *variant,
            args: args
                .iter()
                .map(|a| eval(env, globals, a))
                .collect(),
        },
        Term::DataProj {
            value,
            type_def: _,
            field,
        } => {
            let v = eval(env, globals, value);
            match v {
                Value::Data { args, .. } => args
                    .get(*field as usize)
                    .cloned()
                    .unwrap_or_else(|| Value::Neut(Neutral::Var(Level(0)))),
                other => Value::Neut(Neutral::App {
                    fun: Box::new(value_to_neutral(other)),
                    arg: Box::new(Value::Neut(Neutral::Var(Level(0)))),
                }),
            }
        }
        Term::DataMatch {
            scrutinee,
            enum_def: _,
            arms,
        } => {
            let v = eval(env, globals, scrutinee);
            match v {
                Value::Data { variant, args, .. } => {
                    let arm = arms
                        .iter()
                        .find(|a| a.variant_index == variant)
                        .or_else(|| arms.first());
                    if let Some(arm) = arm {
                        let mut ext = EvalEnv::default();
                        for arg in args {
                            ext.push(arg);
                        }
                        eval(&ext, globals, &arm.body)
                    } else {
                        Value::Neut(Neutral::Var(Level(0)))
                    }
                }
                other => Value::Neut(Neutral::App {
                    fun: Box::new(value_to_neutral(other)),
                    arg: Box::new(Value::Neut(Neutral::Var(Level(0)))),
                }),
            }
        }
        Term::Error => Value::Neut(Neutral::Var(Level(0))),
        Term::Pi { binder, body } => Value::Pi {
            binder: binder.clone(),
            body: Closure {
                env: env.clone(),
                body: *body.clone(),
            },
        },
        Term::Lam { binder, body } => Value::Lam {
            binder: binder.clone(),
            body: Closure {
                env: env.clone(),
                body: *body.clone(),
            },
        },
        Term::App { fun, arg } => {
            let fun_v = eval(env, globals, fun);
            let arg_v = eval(env, globals, arg);
            apply(fun_v, arg_v, globals)
        }
        Term::Let { value, body, .. } => {
            let value_v = eval(env, globals, value);
            let mut ext = env.clone();
            ext.push(value_v);
            eval(&ext, globals, body)
        }
        Term::Refinement { binder, pred } => Value::Pi {
            binder: binder.clone(),
            body: Closure {
                env: env.clone(),
                body: *pred.clone(),
            },
        },
        Term::Admit { .. } => Value::Unit,
        Term::Computation { result, .. } => eval(env, globals, result),
        Term::Meta(_)
        | Term::ArrayLit { .. }
        | Term::ArrayGet { .. }
        | Term::ArraySet { .. }
        | Term::AddrOf { .. }
        | Term::Deref { .. }
        | Term::Len { .. }
        | Term::While { .. }
        | Term::Loop { .. }
        | Term::For { .. }
        | Term::Break
        | Term::Assign { .. }
        | Term::HeapAlloc { .. }
        | Term::HeapDealloc { .. }
        | Term::HeapRealloc { .. }
        | Term::MatrixNew { .. }
        | Term::MatrixToArray { .. }
        | Term::MatrixDrop { .. } => Value::Neut(Neutral::Var(Level(0))),
        Term::Quant { args, .. } => {
            for arg in args {
                let _ = eval(env, globals, arg);
            }
            Value::Neut(Neutral::Var(Level(0)))
        }
    }
}

pub fn whnf(env: &EvalEnv, globals: &GlobalEnv, term: &Term) -> Value {
    eval(env, globals, term)
}

fn apply(fun: Value, arg: Value, globals: &GlobalEnv) -> Value {
    match fun {
        Value::Lam { binder: _, body } => {
            let mut ext = body.env;
            ext.push(arg);
            eval(&ext, globals, &body.body)
        }
        Value::Neut(neut) => Value::Neut(Neutral::App {
            fun: Box::new(neut),
            arg: Box::new(arg),
        }),
        other => Value::Neut(Neutral::App {
            fun: Box::new(value_to_neutral(other)),
            arg: Box::new(arg),
        }),
    }
}

fn value_to_neutral(value: Value) -> Neutral {
    match value {
        Value::Neut(n) => n,
        Value::I32(_)
        | Value::Bool(_)
        | Value::LitInt { .. }
        | Value::LitFloat { .. }
        | Value::LitStr
        | Value::Unit
        | Value::Data { .. }
        | Value::Universe(_)
        | Value::Pi { .. }
        | Value::Lam { .. } => Neutral::Var(Level(0)),
    }
}

pub fn quote(level: Level, value: &Value) -> Term {
    match value {
        Value::Universe(u) => Term::Universe(UniverseLevel(*u)),
        Value::I32(n) => Term::I32(*n),
        Value::Bool(b) => Term::Bool(*b),
        Value::LitInt { value, ty } => Term::LitInt {
            value: *value,
            ty: *ty,
        },
        Value::LitFloat { value, ty } => Term::LitFloat {
            value: *value,
            ty: *ty,
        },
        Value::LitStr => Term::LitStr(String::new()),
        Value::Unit => Term::Unit,
        Value::Data {
            type_def,
            variant,
            args,
        } => Term::DataCtor {
            type_def: *type_def,
            variant: *variant,
            args: args.iter().map(|a| quote(level, a)).collect(),
        },
        Value::Pi { binder, body } => {
            let mut ext = body.env.clone();
            let fresh = Value::Neut(Neutral::Var(level));
            ext.push(fresh);
            let body_nf = eval(&ext, &GlobalEnv::with_primitives(), &body.body);
            Term::Pi {
                binder: binder.clone(),
                body: Box::new(quote(Level(level.0 + 1), &body_nf)),
            }
        }
        Value::Lam { binder, body } => {
            let mut ext = body.env.clone();
            let fresh = Value::Neut(Neutral::Var(level));
            ext.push(fresh);
            let body_nf = eval(&ext, &GlobalEnv::with_primitives(), &body.body);
            Term::Lam {
                binder: binder.clone(),
                body: Box::new(quote(Level(level.0 + 1), &body_nf)),
            }
        }
        Value::Neut(Neutral::Var(v)) => {
            if v.0 >= level.0 {
                Term::Var(Level(v.0 - level.0))
            } else {
                Term::Var(*v)
            }
        }
        Value::Neut(Neutral::Global(id)) => Term::Global(*id),
        Value::Neut(Neutral::App { fun, arg }) => Term::App {
            fun: Box::new(quote(level, &Value::Neut(*fun.clone()))),
            arg: Box::new(quote(level, arg)),
        },
    }
}

pub fn is_def_eq(
    env: &EvalEnv,
    globals: &GlobalEnv,
    t1: &Term,
    t2: &Term,
) -> bool {
    match (t1, t2) {
        (
            Term::Refinement {
                binder: b1,
                pred: p1,
            },
            Term::Refinement {
                binder: b2,
                pred: p2,
            },
        ) => {
            is_def_eq(env, globals, b1.ty(), b2.ty())
                && is_def_eq(env, globals, p1, p2)
        }
        _ => eq_values(
            &whnf(env, globals, t1),
            &whnf(env, globals, t2),
            env.len(),
        ),
    }
}

fn eq_values(v1: &Value, v2: &Value, depth: usize) -> bool {
    match (v1, v2) {
        (Value::Universe(a), Value::Universe(b)) => a == b,
        (Value::I32(a), Value::I32(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (
            Value::LitInt {
                value: a,
                ty: ta,
            },
            Value::LitInt {
                value: b,
                ty: tb,
            },
        ) => a == b && ta == tb,
        (
            Value::LitFloat {
                value: a,
                ty: ta,
            },
            Value::LitFloat {
                value: b,
                ty: tb,
            },
        ) => a == b && ta == tb,
        (Value::LitStr, Value::LitStr) => true,
        (Value::Unit, Value::Unit) => true,
        (
            Value::Data {
                type_def: t1,
                variant: v1,
                args: a1,
            },
            Value::Data {
                type_def: t2,
                variant: v2,
                args: a2,
            },
        ) => {
            t1 == t2
                && v1 == v2
                && a1.len() == a2.len()
                && a1
                    .iter()
                    .zip(a2.iter())
                    .all(|(x, y)| eq_values(x, y, depth))
        }
        (
            Value::Pi {
                binder: b1,
                body: c1,
            },
            Value::Pi {
                binder: b2,
                body: c2,
            },
        ) => {
            if !eq_terms(b1.ty(), b2.ty(), depth) {
                return false;
            }
            let fresh = Value::Neut(Neutral::Var(Level(depth as u32)));
            let mut e1 = c1.env.clone();
            let mut e2 = c2.env.clone();
            e1.push(fresh.clone());
            e2.push(fresh);
            eq_values(
                &eval(&e1, &GlobalEnv::default(), &c1.body),
                &eval(&e2, &GlobalEnv::default(), &c2.body),
                depth + 1,
            )
        }
        (
            Value::Lam {
                binder: b1,
                body: c1,
            },
            Value::Lam {
                binder: b2,
                body: c2,
            },
        ) => {
            if !eq_terms(b1.ty(), b2.ty(), depth) {
                return false;
            }
            let fresh = Value::Neut(Neutral::Var(Level(depth as u32)));
            let mut e1 = c1.env.clone();
            let mut e2 = c2.env.clone();
            e1.push(fresh.clone());
            e2.push(fresh);
            eq_values(
                &eval(&e1, &GlobalEnv::default(), &c1.body),
                &eval(&e2, &GlobalEnv::default(), &c2.body),
                depth + 1,
            )
        }
        (Value::Neut(n1), Value::Neut(n2)) => eq_neutrals(n1, n2, depth),
        _ => false,
    }
}

fn eq_terms(t1: &Term, t2: &Term, depth: usize) -> bool {
    let env = EvalEnv::default();
    let globals = GlobalEnv::default();
    eq_values(
        &whnf(&env, &globals, t1),
        &whnf(&env, &globals, t2),
        depth,
    )
}

fn eq_neutrals(n1: &Neutral, n2: &Neutral, depth: usize) -> bool {
    match (n1, n2) {
        (Neutral::Var(a), Neutral::Var(b)) => a == b,
        (Neutral::Global(a), Neutral::Global(b)) => a == b,
        (
            Neutral::App { fun: f1, arg: a1 },
            Neutral::App { fun: f2, arg: a2 },
        ) => eq_neutrals(f1, f2, depth) && eq_values(a1, a2, depth),
        _ => false,
    }
}

pub fn type_of_i32() -> Term {
    Term::Global(crate::core::globals::prim::I32)
}

pub fn type_of_bool() -> Term {
    Term::Global(crate::core::globals::prim::BOOL)
}
