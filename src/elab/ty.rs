use crate::core::globals::prim;
use crate::core::term::{Binder, Level, Term};
use crate::elab::env::ElabEnv;
use crate::elab::expr::elab_expr;
use crate::frontend::surface::SurfaceTy;
use crate::nia_std::{QUBIT, RESULT};

pub fn elab_ty(env: &mut ElabEnv, ty: &SurfaceTy) -> Result<Term, String> {
    elab_ty_for_param(env, ty, None, None)
}

pub fn elab_ty_for_param(
    env: &mut ElabEnv,
    ty: &SurfaceTy,
    bind_name: Option<&str>,
    bind_level: Option<Level>,
) -> Result<Term, String> {
    match ty {
        SurfaceTy::Refined { base, pred } => {
            let name = bind_name
                .ok_or_else(|| "refinement type requires a parameter name in this context".to_string())?;
            let level = bind_level.unwrap_or(Level(0));
            let base_ty = elab_ty_for_param(env, base, None, None)?;
            let (pred_term, _) = env.with_locals_isolated(
                &[name.to_string()],
                &[base_ty.clone()],
                |env| elab_expr(env, pred, Some(&Term::Global(prim::BOOL))),
            )?;
            let pred_term = pred_term.subst(Level(0), &Term::Var(level));
            Ok(Term::Refinement {
                binder: Binder::new(name, level, base_ty),
                pred: Box::new(pred_term),
            })
        }
        SurfaceTy::I8 => Ok(Term::Global(crate::core::globals::prim::I8)),
        SurfaceTy::U8 => Ok(Term::Global(crate::core::globals::prim::U8)),
        SurfaceTy::I16 => Ok(Term::Global(crate::core::globals::prim::I16)),
        SurfaceTy::U16 => Ok(Term::Global(crate::core::globals::prim::U16)),
        SurfaceTy::I32 => Ok(Term::Global(crate::core::globals::prim::I32)),
        SurfaceTy::I64 => Ok(Term::Global(crate::core::globals::prim::I64)),
        SurfaceTy::U64 => Ok(Term::Global(crate::core::globals::prim::U64)),
        SurfaceTy::I128 => Ok(Term::Global(crate::core::globals::prim::I128)),
        SurfaceTy::U128 => Ok(Term::Global(crate::core::globals::prim::U128)),
        SurfaceTy::Isize => Ok(Term::Global(crate::core::globals::prim::I64)),
        SurfaceTy::Usize => Ok(Term::Global(crate::core::globals::prim::U64)),
        SurfaceTy::Bool => Ok(Term::Global(crate::core::globals::prim::BOOL)),
        SurfaceTy::F16 => Ok(Term::Global(crate::core::globals::prim::F16)),
        SurfaceTy::F32 => Ok(Term::Global(crate::core::globals::prim::F32)),
        SurfaceTy::F64 => Ok(Term::Global(crate::core::globals::prim::F64)),
        SurfaceTy::String => Ok(Term::Global(crate::core::globals::prim::STRING)),
        SurfaceTy::Qubit => {
            env.require_quant_scope("type `qubit`")?;
            Ok(Term::Global(prim::QUBIT))
        }
        SurfaceTy::Result => {
            env.require_quant_scope("type `result`")?;
            Ok(Term::Global(prim::RESULT))
        }
        SurfaceTy::Unit => Ok(Term::Global(crate::core::globals::prim::UNIT)),
        SurfaceTy::Struct(name) if name == "Type" => Ok(Term::ty()),
        SurfaceTy::Struct(name) if name == QUBIT => {
            env.require_quant_scope("type `qubit`")?;
            Ok(Term::Global(prim::QUBIT))
        }
        SurfaceTy::Struct(name) if name == RESULT => {
            env.require_quant_scope("type `result`")?;
            Ok(Term::Global(prim::RESULT))
        }
        SurfaceTy::Struct(name) => {
            if let Some(level) = env.lookup_type_param(name) {
                return Ok(Term::Var(level));
            }
            let kind = env
                .resolved
                .lookup_type(name)
                .ok_or_else(|| format!("unknown type `{name}`"))?;
            Ok(Term::Global(env.type_gid(kind)))
        }
        SurfaceTy::Enum(name) | SurfaceTy::Vector(name, _) => {
            let kind = env
                .resolved
                .lookup_type(name)
                .ok_or_else(|| format!("unknown type `{name}`"))?;
            Ok(Term::Global(env.type_gid(kind)))
        }
        SurfaceTy::Array(elem, len) | SurfaceTy::AnonVector(elem, len) => {
            let elem_ty = elab_ty_for_param(env, elem, None, None)?;
            let gid = env.register_array_type(&elem_ty, *len as u32);
            Ok(Term::Global(gid))
        }
        SurfaceTy::Ptr(inner) => {
            let inner_ty = elab_ty_for_param(env, inner, None, None)?;
            let gid = env.register_ptr_type(&inner_ty);
            Ok(Term::Global(gid))
        }
        SurfaceTy::Matrix(elem, _) => {
            let elem_ty = elab_ty_for_param(env, elem, None, None)?;
            let gid = env.register_matrix_type(&elem_ty);
            Ok(Term::Global(gid))
        }
        other => Err(format!("type not supported in elaborator yet: {other:?}")),
    }
}
