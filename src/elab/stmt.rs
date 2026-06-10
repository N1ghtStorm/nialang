use crate::core::effect::Effect;
use crate::core::globals::prim;
use crate::core::term::{Binder, Term};
use crate::elab::affine::{
    check_qubit_copy, is_qubit_new, is_qubit_type, register_qubit,
};
use crate::elab::env::ElabEnv;
use crate::elab::expr::{elab_expr, infer_expr};
use crate::elab::ty::elab_ty;
use crate::frontend::surface::{Block, Stmt, SurfaceTy};

pub fn elab_block(
    env: &mut ElabEnv,
    block: &Block,
    ret: Option<&SurfaceTy>,
) -> Result<Term, String> {
    let expected = ret.map(|ty| elab_ty(env, ty)).transpose()?;
    elab_stmts(env, &block.stmts, block.tail.as_ref(), expected.as_ref())
}

fn elab_stmts(
    env: &mut ElabEnv,
    stmts: &[Stmt],
    tail: Option<&crate::frontend::surface::Expr>,
    expected: Option<&Term>,
) -> Result<Term, String> {
    if stmts.is_empty() {
        return elab_tail(env, tail, expected);
    }

    match &stmts[0] {
        Stmt::Let { name, ty, init } => {
            if name != "_" && env.lookup_local(name).is_some() {
                return Err(format!("variable `{name}` is already bound in this scope"));
            }
            let (init_term, init_ty) = if let Some(surface_ty) = ty {
                let expected = elab_ty(env, surface_ty)?;
                let (term, _) = elab_expr(env, init, Some(&expected))?;
                (term, expected)
            } else {
                infer_expr(env, init)?
            };
            check_qubit_copy(env, init, &init_ty)?;
            let level = env.current_level();
            let track_binding = name != "_";
            if track_binding {
                env.push_local(name, init_ty.clone());
                if is_qubit_type(&init_ty) && is_qubit_new(&init_term) {
                    register_qubit(env, name);
                }
            }
            let rest = elab_stmts(env, &stmts[1..], tail, expected)?;
            if track_binding {
                env.pop_local();
            }
            Ok(Term::Let {
                binder: Binder::new(name, level, init_ty),
                value: Box::new(init_term),
                body: Box::new(rest),
            })
        }
        Stmt::Return(expr) => {
            let (term, _) = elab_expr(env, expr, expected)?;
            Ok(term)
        }
        Stmt::Admit(prop) => {
            env.require_effect(Effect::Ghost, "`admit`")?;
            let (prop_term, _) = elab_expr(env, prop, Some(&Term::Global(prim::BOOL)))?;
            elab_with_effect(
                env,
                Term::Admit {
                    prop: Box::new(prop_term),
                },
                Term::Global(prim::UNIT),
                |env| elab_stmts(env, &stmts[1..], tail, expected),
            )
        }
        Stmt::Expr(expr) => {
            let (effect, effect_ty) = infer_expr(env, expr)?;
            elab_with_effect(env, effect, effect_ty, |env| {
                elab_stmts(env, &stmts[1..], tail, expected)
            })
        }
        Stmt::If { cond, then_block } => {
            let (cond_term, _) = infer_expr(env, cond)?;
            let then_term = elab_block_as_expr(env, then_block, expected)?;
            let else_term = elab_stmts(env, &stmts[1..], tail, expected)?;
            Ok(if_then_else(env, cond_term, then_term, else_term, expected)?)
        }
        Stmt::Assign { target, value } => elab_assign(env, target, value, stmts, tail, expected),
        Stmt::While { cond, body } => {
            env.require_effect(Effect::IO, "`while` loop")?;
            let (cond_term, _) = infer_expr(env, cond)?;
            let body_term = env.enter_while_loop(|env| {
                elab_block_as_expr(env, body, Some(&Term::Global(prim::UNIT)))
            })?;
            elab_with_effect(
                env,
                Term::While {
                    cond: Box::new(cond_term),
                    body: Box::new(body_term),
                },
                Term::Global(prim::UNIT),
                |env| elab_stmts(env, &stmts[1..], tail, expected),
            )
        }
        Stmt::Loop { body } => {
            env.require_effect(Effect::IO, "`loop`")?;
            if !block_contains_break(body) {
                return Err("`loop` must contain a `break`".into());
            }
            let body_term = env.enter_loop_stmt(|env| {
                elab_block_as_expr(env, body, Some(&Term::Global(prim::UNIT)))
            })?;
            elab_with_effect(
                env,
                Term::Loop {
                    body: Box::new(body_term),
                },
                Term::Global(prim::UNIT),
                |env| elab_stmts(env, &stmts[1..], tail, expected),
            )
        }
        Stmt::Quant { body } => {
            env.require_effect(Effect::Quantum, "`quant { }` block")?;
            let body_term = env.with_quant_scope(|env| {
                elab_block_as_expr(env, body, Some(&Term::Global(prim::UNIT)))
            })?;
            elab_with_effect(env, body_term, Term::Global(prim::UNIT), |env| {
                elab_stmts(env, &stmts[1..], tail, expected)
            })
        }
        Stmt::Gpu { body } => {
            env.require_effect(Effect::Gpu, "`gpu { }` block")?;
            let body_term = env.with_gpu_scope(|env| {
                elab_block_as_expr(env, body, Some(&Term::Global(prim::UNIT)))
            })?;
            elab_with_effect(env, body_term, Term::Global(prim::UNIT), |env| {
                elab_stmts(env, &stmts[1..], tail, expected)
            })
        }
        Stmt::Break => {
            if env.while_depth > 0 {
                return Err("`break` inside `while` / `for` is not supported yet".into());
            }
            if env.loop_depth == 0 {
                return Err("`break` outside loop".into());
            }
            elab_with_effect(env, Term::Break, Term::Global(prim::UNIT), |env| {
                elab_stmts(env, &stmts[1..], tail, expected)
            })
        }
        Stmt::For {
            var,
            start,
            end,
            body,
        } => {
            if block_contains_return(body) {
                return Err("`return` is not allowed inside `for`".into());
            }
            let let_stmt = Stmt::Let {
                name: var.clone(),
                ty: Some(crate::frontend::surface::SurfaceTy::I32),
                init: start.clone(),
            };
            let cond = crate::frontend::surface::Expr::Lt(
                Box::new(crate::frontend::surface::Expr::Ident(var.clone())),
                Box::new(end.clone()),
            );
            let inc = Stmt::Assign {
                target: crate::frontend::surface::Expr::Ident(var.clone()),
                value: crate::frontend::surface::Expr::Add(
                    Box::new(crate::frontend::surface::Expr::Ident(var.clone())),
                    Box::new(crate::frontend::surface::Expr::Int(1)),
                ),
            };
            let mut while_body = body.clone();
            while_body.stmts.push(inc);
            let while_stmt = Stmt::While {
                cond,
                body: while_body,
            };
            let mut desugared = vec![let_stmt, while_stmt];
            desugared.extend_from_slice(&stmts[1..]);
            elab_stmts(env, &desugared, tail, expected)
        }
        other => Err(format!("statement not supported in elaborator yet: {other:?}")),
    }
}

fn elab_assign(
    env: &mut ElabEnv,
    target: &crate::frontend::surface::Expr,
    value: &crate::frontend::surface::Expr,
    stmts: &[Stmt],
    tail: Option<&crate::frontend::surface::Expr>,
    expected: Option<&Term>,
) -> Result<Term, String> {
    env.require_effect(Effect::IO, "assignment")?;
    let assign_term = match target {
        crate::frontend::surface::Expr::Index(arr, idx) => {
            let (arr_term, arr_ty) = infer_expr(env, arr)?;
            let Term::Global(array_gid) = arr_ty else {
                return Err("array assign expects array".into());
            };
            let (arr_len, elem_ty) = {
                let info = env
                    .data
                    .array_info(array_gid)
                    .ok_or_else(|| "unknown array type".to_string())?;
                (info.len, info.elem.clone())
            };
            let (idx_term, _) = elab_expr(env, idx, Some(&Term::Global(prim::I32)))?;
            let (val_term, _) = elab_expr(env, value, Some(&elem_ty))?;
            Term::ArraySet {
                elem_ty: array_gid,
                len: arr_len,
                arr: Box::new(arr_term),
                index: Box::new(idx_term),
                value: Box::new(val_term),
            }
        }
        crate::frontend::surface::Expr::Deref(ptr) => {
            let (ptr_term, ptr_ty) = infer_expr(env, ptr)?;
            let Term::Global(ptr_gid) = ptr_ty else {
                return Err("pointer assign expects pointer".into());
            };
            let inner = env
                .data
                .ptr_info(ptr_gid)
                .ok_or_else(|| "unknown pointer type".to_string())?
                .inner
                .clone();
            let (val_term, _) = elab_expr(env, value, Some(&inner))?;
            Term::Assign {
                target: Box::new(Term::Deref {
                    inner_ty: ptr_gid,
                    ptr: Box::new(ptr_term),
                }),
                value: Box::new(val_term),
            }
        }
        crate::frontend::surface::Expr::Ident(name) => {
            let (level, ty) = env
                .lookup_local(name)
                .ok_or_else(|| format!("unknown variable `{name}`"))?;
            let (val_term, _) = elab_expr(env, value, Some(&ty))?;
            Term::Assign {
                target: Box::new(Term::Var(level)),
                value: Box::new(val_term),
            }
        }
        other => {
            let (target_term, target_ty) = infer_expr(env, other)?;
            let (val_term, _) = elab_expr(env, value, Some(&target_ty))?;
            Term::Assign {
                target: Box::new(target_term),
                value: Box::new(val_term),
            }
        }
    };
    elab_with_effect(env, assign_term, Term::Global(prim::UNIT), |env| {
        elab_stmts(env, &stmts[1..], tail, expected)
    })
}

fn elab_block_as_expr(
    env: &mut ElabEnv,
    block: &Block,
    expected: Option<&Term>,
) -> Result<Term, String> {
    elab_stmts(env, &block.stmts, block.tail.as_ref(), expected)
}

fn elab_tail(
    env: &mut ElabEnv,
    tail: Option<&crate::frontend::surface::Expr>,
    expected: Option<&Term>,
) -> Result<Term, String> {
    match tail {
        Some(expr) => {
            let (term, _) = elab_expr(env, expr, expected)?;
            Ok(term)
        }
        None => Ok(Term::Unit),
    }
}

/// Wraps a side-effecting term before the rest of a block, keeping elaboration
/// levels aligned with the `Let` binder the checker will introduce.
fn elab_with_effect(
    env: &mut ElabEnv,
    effect: Term,
    effect_ty: Term,
    f: impl FnOnce(&mut ElabEnv) -> Result<Term, String>,
) -> Result<Term, String> {
    let level = env.current_level();
    env.push_local("_", effect_ty.clone());
    let body = f(env)?;
    env.pop_local();
    Ok(Term::Let {
        binder: Binder::new("_", level, effect_ty),
        value: Box::new(effect),
        body: Box::new(body),
    })
}

fn if_then_else(
    env: &ElabEnv,
    cond: Term,
    then_term: Term,
    else_term: Term,
    expected: Option<&Term>,
) -> Result<Term, String> {
    let result_ty = expected
        .cloned()
        .or_else(|| infer_common_type(env, &then_term, &else_term))
        .ok_or_else(|| "cannot infer if expression type".to_string())?;
    let prim_id = match &result_ty {
        Term::Global(id) => *id,
        _ => return Err("if expression must have primitive or data type".into()),
    };
    let ops = env
        .prelude
        .ops_for_prim(prim_id)
        .ok_or_else(|| format!("no if-then-else for `{result_ty:?}`"))?;
    Ok(Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(ops.if_then_else)),
                arg: Box::new(cond),
            }),
            arg: Box::new(then_term),
        }),
        arg: Box::new(else_term),
    })
}

fn block_contains_return(block: &Block) -> bool {
    block
        .stmts
        .iter()
        .any(|st| matches!(st, Stmt::Return(_)))
}

fn block_contains_break(block: &Block) -> bool {
    block.stmts.iter().any(|st| stmt_contains_break(st))
}

fn stmt_contains_break(st: &Stmt) -> bool {
    match st {
        Stmt::Break => true,
        Stmt::If { then_block, .. } => block_contains_break(then_block),
        Stmt::While { body, .. }
        | Stmt::Loop { body, .. }
        | Stmt::Gpu { body, .. }
        | Stmt::Quant { body, .. } => block_contains_break(body),
        Stmt::For { body, .. } => block_contains_break(body),
        Stmt::Let { init, .. } | Stmt::Expr(init) | Stmt::Return(init) => {
            expr_contains_break(init)
        }
        Stmt::Assign { target, value } => {
            expr_contains_break(target) || expr_contains_break(value)
        }
        Stmt::Admit(_) => false,
    }
}

fn expr_contains_break(expr: &crate::frontend::surface::Expr) -> bool {
    use crate::frontend::surface::Expr;
    match expr {
        Expr::Gpu { body } | Expr::Quant { body } => block_contains_break(body),
        Expr::Neg(inner)
        | Expr::Not(inner)
        | Expr::BitNot(inner)
        | Expr::AddrOf(inner)
        | Expr::Deref(inner)
        | Expr::Field(inner, _) => expr_contains_break(inner),
        Expr::Add(l, r)
        | Expr::Sub(l, r)
        | Expr::Mul(l, r)
        | Expr::VecDot(l, r)
        | Expr::Div(l, r)
        | Expr::Rem(l, r)
        | Expr::BitAnd(l, r)
        | Expr::BitOr(l, r)
        | Expr::BitXor(l, r)
        | Expr::Shl(l, r)
        | Expr::Shr(l, r)
        | Expr::Eq(l, r)
        | Expr::Ne(l, r)
        | Expr::Lt(l, r)
        | Expr::Le(l, r)
        | Expr::Gt(l, r)
        | Expr::Ge(l, r)
        | Expr::Index(l, r) => expr_contains_break(l) || expr_contains_break(r),
        Expr::Call { args, .. } | Expr::GenericCall { args, .. } => {
            args.iter().any(expr_contains_break)
        }
        Expr::MethodCall { receiver, args, .. } => {
            expr_contains_break(receiver) || args.iter().any(expr_contains_break)
        }
        Expr::StructLit { fields, .. }
        | Expr::VectorLit { fields, .. }
        | Expr::EnumStruct { fields, .. } => fields.iter().any(|(_, e)| expr_contains_break(e)),
        Expr::AnonVectorLit(elems)
        | Expr::ArrayLit(elems)
        | Expr::EnumTuple { args: elems, .. } => elems.iter().any(expr_contains_break),
        Expr::Match { scrutinee, arms } => {
            expr_contains_break(scrutinee) || arms.iter().any(|(_, e)| expr_contains_break(e))
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Ident(_)
        | Expr::EnumVariant { .. } => false,
    }
}

fn infer_common_type(env: &ElabEnv, a: &Term, b: &Term) -> Option<Term> {
    let checker = crate::core::Checker::with_data(&env.globals, &env.data);
    let mut ctx = crate::core::TypingCtx::default();
    let mut metas = crate::core::MetaEnv::default();
    let ta = checker.infer(&mut ctx.clone(), &mut metas, a).ok()?;
    let tb = checker.infer(&mut ctx, &mut metas, b).ok()?;
    if checker.def_eq(&crate::core::TypingCtx::default(), &metas, &ta, &tb) {
        Some(ta)
    } else {
        None
    }
}
