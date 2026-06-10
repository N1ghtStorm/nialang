use crate::core::env::{EvalEnv, TypingCtx};
use crate::core::globals::GlobalEnv;
use crate::core::meta::{MetaEnv, MetaId};
use crate::core::term::{Binder, Level, Term};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnifyError {
    Message(String),
}

impl std::fmt::Display for UnifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnifyError::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for UnifyError {}

pub type UnifyResult<T> = Result<T, UnifyError>;

pub fn unify(
    ctx: &TypingCtx,
    globals: &GlobalEnv,
    metas: &mut MetaEnv,
    t1: &Term,
    t2: &Term,
) -> UnifyResult<()> {
    let t1 = metas.normalize(t1);
    let t2 = metas.normalize(t2);
    unify_nf(ctx, globals, metas, &t1, &t2)
}

fn unify_nf(
    ctx: &TypingCtx,
    globals: &GlobalEnv,
    metas: &mut MetaEnv,
    t1: &Term,
    t2: &Term,
) -> UnifyResult<()> {
    let env = typing_env(ctx);
    let v1 = whnf_term(&env, globals, metas, t1);
    let v2 = whnf_term(&env, globals, metas, t2);
    unify_values(ctx, globals, metas, &v1, &v2)
}

fn whnf_term(env: &EvalEnv, globals: &GlobalEnv, metas: &MetaEnv, term: &Term) -> Term {
    metas.normalize(term)
}

fn unify_values(
    ctx: &TypingCtx,
    globals: &GlobalEnv,
    metas: &mut MetaEnv,
    t1: &Term,
    t2: &Term,
) -> UnifyResult<()> {
    let t1 = metas.normalize(t1);
    let t2 = metas.normalize(t2);

    if t1 == t2 {
        return Ok(());
    }

    match (&t1, &t2) {
        (Term::Meta(id1), Term::Meta(id2)) if id1 == id2 => Ok(()),
        (Term::Meta(id), other) | (other, Term::Meta(id)) => {
            metas
                .solve(*id, other.clone())
                .map_err(UnifyError::Message)
        }
        (Term::Global(a), Term::Global(b)) if a == b => Ok(()),
        (Term::Universe(a), Term::Universe(b)) if a == b => Ok(()),
        (Term::I32(a), Term::I32(b)) if a == b => Ok(()),
        (Term::Bool(a), Term::Bool(b)) if a == b => Ok(()),
        (Term::Unit, Term::Unit) => Ok(()),
        (
            Term::LitInt {
                value: a,
                ty: ta,
            },
            Term::LitInt {
                value: b,
                ty: tb,
            },
        ) if a == b && ta == tb => Ok(()),
        (
            Term::LitFloat {
                value: a,
                ty: ta,
            },
            Term::LitFloat {
                value: b,
                ty: tb,
            },
        ) if a == b && ta == tb => Ok(()),
        (Term::LitStr(a), Term::LitStr(b)) if a == b => Ok(()),
        (Term::Pi { binder: b1, body: c1 }, Term::Pi { binder: b2, body: c2 }) => {
            unify_nf(ctx, globals, metas, b1.ty(), b2.ty())?;
            let body1 = c1.subst(b1.level, &Term::Var(b2.level));
            unify_nf(ctx, globals, metas, &body1, c2)
        }
        (Term::App { fun: f1, arg: a1 }, Term::App { fun: f2, arg: a2 }) => {
            unify_nf(ctx, globals, metas, f1, f2)?;
            unify_nf(ctx, globals, metas, a1, a2)
        }
        _ => Err(UnifyError::Message(format!(
            "cannot unify `{t1:?}` with `{t2:?}`"
        ))),
    }
}

fn typing_env(ctx: &TypingCtx) -> EvalEnv {
    let mut env = EvalEnv::default();
    for i in 0..ctx.len() {
        let level = Level(i as u32);
        env.push(crate::core::env::Value::Neut(crate::core::env::Neutral::Var(
            level,
        )));
    }
    env
}

/// Instantiate implicit Pi binders by creating fresh metas.
pub fn intro_implicit(
    metas: &mut MetaEnv,
    ty: &Term,
) -> (Term, Vec<MetaId>) {
    let mut cur = metas.normalize(ty);
    let mut created = Vec::new();
    loop {
        match &cur {
            Term::Pi { binder, body }
                if binder.explicitness == crate::core::term::Explicitness::Implicit =>
            {
                let meta = metas.fresh();
                created.push(meta);
                cur = body.subst(binder.level, &Term::Meta(meta));
            }
            _ => break,
        }
    }
    (cur, created)
}

/// Skip implicit Pi binders when matching explicit call arguments.
pub fn skip_implicit_pis(metas: &MetaEnv, ty: &Term) -> Term {
    let mut cur = metas.normalize(ty);
    loop {
        match &cur {
            Term::Pi { binder, body }
                if binder.explicitness == crate::core::term::Explicitness::Implicit =>
            {
                cur = *body.clone();
            }
            _ => break,
        }
    }
    cur
}

pub fn implicit_pi_binder(ty: &Term) -> Option<Binder> {
    match ty {
        Term::Pi { binder, .. }
            if binder.explicitness == crate::core::term::Explicitness::Implicit =>
        {
            Some(binder.clone())
        }
        _ => None,
    }
}
