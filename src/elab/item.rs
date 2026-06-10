use crate::core::term::{Binder, Level, Term};
use crate::elab::affine::{is_qubit_type, register_qubit};
use crate::elab::env::{fn_type_for_def, type_param_bindings, ElabEnv};
use crate::elab::stmt::elab_block;
use crate::frontend::resolve::ResolvedFn;

pub fn elab_fn(env: &mut ElabEnv, f: &ResolvedFn) -> Result<Term, String> {
    env.begin_fn(env.fn_gid(f.id), f.def.is_quantum);
    let sig = fn_type_for_def(env, &f.def.params, f.def.ret.as_ref(), f.def.is_quantum)?;
    let explicit_params: Vec<_> = f
        .def
        .params
        .iter()
        .filter(|(_, _, implicit)| !*implicit)
        .cloned()
        .collect();
    let type_params = type_param_bindings(&f.def.params);
    let term = env.with_type_params(&type_params, |env| {
        let body = env.with_fn_params(&explicit_params, |env| {
            elab_block(env, &f.def.body, f.def.ret.as_ref())
        })?;
        let mut term = body;
        for (name, ty, _) in explicit_params.iter().rev() {
            let idx = f
                .def
                .params
                .iter()
                .position(|(n, _, _)| n == name)
                .expect("explicit param in signature");
            let level = crate::core::term::Level(idx as u32);
            let domain = crate::elab::ty::elab_ty_for_param(env, ty, Some(name), Some(level))?;
            term = Term::Lam {
                binder: Binder::new(name, level, domain),
                body: Box::new(term),
            };
        }
        Ok::<Term, String>(term)
    })?;
    let effect = env.end_fn(f.def.is_quantum);
    env.globals
        .update_fn_return_effect(env.fn_gid(f.id), effect);
    env.finish_fn(f.id, term.clone());
    let _ = sig;
    Ok(term)
}

impl<'a> ElabEnv<'a> {
    pub fn with_fn_params<R>(
        &mut self,
        params: &[(String, crate::frontend::surface::SurfaceTy, bool)],
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if params.is_empty() {
            return f(self);
        }
        let (name, ty, _) = &params[0];
        let idx = self.current_level().0;
        let core_ty = crate::elab::ty::elab_ty_for_param(self, ty, Some(name), Some(Level(idx)))
            .expect("fn param type");
        self.with_local(name, core_ty.clone(), |env| {
            if is_qubit_type(&core_ty) {
                register_qubit(env, name);
            }
            env.with_fn_params(&params[1..], f)
        })
    }
}
