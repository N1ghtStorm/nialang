//! Surface effect estimation for function declarations (phase 12).

use crate::core::effect::{join_effect, Effect};
use crate::frontend::surface::{Block, Expr, FnDef, Stmt};
use crate::nia_std::is_quantum_builtin_fn;

pub fn effect_of_fn_def(def: &FnDef) -> Effect {
    join_effect(flag_effect(def), direct_effect_of_block(&def.body))
}

pub fn flag_effect(def: &FnDef) -> Effect {
    if def.partial {
        Effect::Div
    } else if def.is_quantum {
        Effect::Quantum
    } else {
        Effect::Tot
    }
}

fn direct_effect_of_block(block: &Block) -> Effect {
    let mut effect = Effect::Tot;
    for stmt in &block.stmts {
        effect = join_effect(effect, direct_effect_of_stmt(stmt));
    }
    if let Some(tail) = &block.tail {
        effect = join_effect(effect, direct_effect_of_expr(tail));
    }
    effect
}

fn direct_effect_of_stmt(stmt: &Stmt) -> Effect {
    match stmt {
        Stmt::Let { init, .. } => direct_effect_of_expr(init),
        Stmt::Return(expr) | Stmt::Admit(expr) => direct_effect_of_expr(expr),
        Stmt::Expr(expr) => direct_effect_of_expr(expr),
        Stmt::Assign { target, value } => {
            join_effect(
                Effect::IO,
                join_effect(direct_effect_of_expr(target), direct_effect_of_expr(value)),
            )
        }
        Stmt::If { cond, then_block } => {
            join_effect(direct_effect_of_expr(cond), direct_effect_of_block(then_block))
        }
        Stmt::While { cond, body } => join_effect(
            Effect::IO,
            join_effect(direct_effect_of_expr(cond), direct_effect_of_block(body)),
        ),
        Stmt::Loop { body } => join_effect(Effect::IO, direct_effect_of_block(body)),
        Stmt::For { start, end, body, .. } => join_effect(
            Effect::IO,
            join_effect(
                direct_effect_of_expr(start),
                join_effect(direct_effect_of_expr(end), direct_effect_of_block(body)),
            ),
        ),
        Stmt::Break => Effect::IO,
        Stmt::Quant { body } => join_effect(Effect::Quantum, direct_effect_of_block(body)),
        Stmt::Gpu { body } => join_effect(Effect::Gpu, direct_effect_of_block(body)),
    }
}

fn direct_effect_of_expr(expr: &Expr) -> Effect {
    match expr {
        Expr::Call { name, args } => {
            let mut effect = if is_quantum_builtin_fn(name) {
                Effect::Quantum
            } else if *name == "println" {
                Effect::IO
            } else {
                Effect::Tot
            };
            for arg in args {
                effect = join_effect(effect, direct_effect_of_expr(arg));
            }
            effect
        }
        Expr::Quant { body } => join_effect(Effect::Quantum, direct_effect_of_block(body)),
        Expr::Gpu { body } => join_effect(Effect::Gpu, direct_effect_of_block(body)),
        Expr::Neg(inner)
        | Expr::Not(inner)
        | Expr::BitNot(inner)
        | Expr::AddrOf(inner)
        | Expr::Deref(inner) => direct_effect_of_expr(inner),
        Expr::Add(l, r)
        | Expr::Sub(l, r)
        | Expr::Mul(l, r)
        | Expr::Div(l, r)
        | Expr::Rem(l, r)
        | Expr::BitAnd(l, r)
        | Expr::BitOr(l, r)
        | Expr::BitXor(l, r)
        | Expr::Shl(l, r)
        | Expr::Shr(l, r)
        | Expr::Lt(l, r)
        | Expr::Gt(l, r)
        | Expr::Le(l, r)
        | Expr::Ge(l, r)
        | Expr::Eq(l, r)
        | Expr::Ne(l, r)
        | Expr::VecDot(l, r) => join_effect(direct_effect_of_expr(l), direct_effect_of_expr(r)),
        Expr::Index(arr, idx) => join_effect(direct_effect_of_expr(arr), direct_effect_of_expr(idx)),
        Expr::ArrayLit(elems) | Expr::AnonVectorLit(elems) => elems
            .iter()
            .fold(Effect::Tot, |e, x| join_effect(e, direct_effect_of_expr(x))),
        Expr::StructLit { fields, .. } => fields
            .iter()
            .fold(Effect::Tot, |e, (_, x)| join_effect(e, direct_effect_of_expr(x))),
        Expr::Field(receiver, _) => direct_effect_of_expr(receiver),
        Expr::EnumVariant { .. } => Effect::Tot,
        Expr::EnumTuple { args, .. } => args
            .iter()
            .fold(Effect::Tot, |e, x| join_effect(e, direct_effect_of_expr(x))),
        Expr::EnumStruct { fields, .. } => fields
            .iter()
            .fold(Effect::Tot, |e, (_, x)| join_effect(e, direct_effect_of_expr(x))),
        Expr::Match { scrutinee, arms } => arms.iter().fold(direct_effect_of_expr(scrutinee), |e, (_, body)| {
            join_effect(e, direct_effect_of_expr(body))
        }),
        Expr::MethodCall { receiver, args, .. } => args.iter().fold(direct_effect_of_expr(receiver), |e, x| {
            join_effect(e, direct_effect_of_expr(x))
        }),
        Expr::Int(_)
        | Expr::Bool(_)
        | Expr::Float(_)
        | Expr::String(_)
        | Expr::Ident(_)
        | Expr::GenericCall { .. }
        | Expr::VectorLit { .. } => Effect::Tot,
    }
}
