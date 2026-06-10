use crate::core::data::{DataEnv, VariantFields};
use crate::core::env::{EvalEnv, TypingCtx};
use crate::core::globals::{prim, GlobalEnv};
use crate::core::quant::QuantKind;
use crate::core::meta::MetaEnv;
use crate::core::nbe::is_def_eq;
use crate::core::term::{Binder, Explicitness, Level, MatchArm, Term, UniverseLevel};
use crate::core::unify::{skip_implicit_pis, unify};
use crate::frontend::resolve::DefId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TypeError {
    Message(String),
}

impl std::fmt::Display for TypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeError::Message(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for TypeError {}

pub type CheckResult<T> = Result<T, TypeError>;

pub struct Checker<'a> {
    pub globals: &'a GlobalEnv,
    pub data: Option<&'a DataEnv>,
}

impl<'a> Checker<'a> {
    pub fn new(globals: &'a GlobalEnv) -> Self {
        Self { globals, data: None }
    }

    pub fn with_data(globals: &'a GlobalEnv, data: &'a DataEnv) -> Self {
        Self {
            globals,
            data: Some(data),
        }
    }

    pub fn check(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        term: &Term,
        expected: &Term,
    ) -> CheckResult<()> {
        let inferred = self.infer(ctx, metas, term)?;
        let inferred = metas.normalize(&inferred);
        let expected = metas.normalize(expected);
        let expected = peel_computation_result(&expected);
        if self.def_eq(ctx, metas, &inferred, &expected)
            || self.subtype(ctx, metas, &inferred, &expected)
        {
            return Ok(());
        }
        unify(ctx, self.globals, metas, &inferred, &expected).map_err(|e| {
            TypeError::Message(self.format_unify_error(metas, &inferred, &expected, &e))
        })?;
        Ok(())
    }

    /// Refinement subtyping: refined values are accepted where base types are expected,
    /// and base values are accepted where refined types are expected (VC deferred to verify).
    fn subtype(
        &self,
        ctx: &TypingCtx,
        metas: &MetaEnv,
        inferred: &Term,
        expected: &Term,
    ) -> bool {
        let (inferred_base, _) = peel_refinement(inferred);
        let (expected_base, expected_pred) = peel_refinement(expected);
        if expected_pred.is_some() && self.def_eq(ctx, metas, inferred_base, expected_base) {
            return true;
        }
        if peel_refinement(inferred).1.is_some() && self.def_eq(ctx, metas, inferred_base, expected_base)
        {
            return true;
        }
        false
    }

    pub fn infer(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        term: &Term,
    ) -> CheckResult<Term> {
        match term {
            Term::Error => Err(TypeError::Message("encountered error term".into())),
            Term::Meta(id) => metas.lookup(*id).cloned().ok_or_else(|| {
                TypeError::Message(
                    metas
                        .implicit_name(*id)
                        .map(|name| format!("failed to infer implicit argument `{name}`"))
                        .unwrap_or_else(|| format!("unsolved metavariable {id:?}")),
                )
            }),
            Term::Var(level) => ctx
                .lookup(*level)
                .map(|b| metas.normalize(b.ty()))
                .ok_or_else(|| TypeError::Message(format!("unbound variable {level:?}"))),
            Term::Global(id) => self
                .globals
                .type_of(*id)
                .cloned()
                .map(|t| metas.normalize(&t))
                .ok_or_else(|| TypeError::Message(format!("unknown global {id:?}"))),
            Term::Universe(u) => Ok(Term::Universe(UniverseLevel(u.0 + 1))),
            Term::I32(_) => Ok(Term::Global(prim::I32)),
            Term::Bool(_) => Ok(Term::Global(prim::BOOL)),
            Term::LitInt { ty, .. } => Ok(Term::Global(*ty)),
            Term::LitFloat { ty, .. } => Ok(Term::Global(*ty)),
            Term::LitStr(_) => Ok(Term::Global(prim::STRING)),
            Term::Unit => Ok(Term::Global(prim::UNIT)),
            Term::Pi { binder, body } => {
                self.check_is_type(ctx, metas, binder.ty())?;
                let body_ty = ctx.bind(&binder.name_hint, (*binder.ty).clone(), |ctx| {
                    self.infer(ctx, metas, body)
                })?;
                self.check_is_type(ctx, metas, &body_ty)?;
                Ok(Term::ty())
            }
            Term::Lam { .. } => Err(TypeError::Message(
                "lambda requires expected Pi type; use check mode".into(),
            )),
            Term::App { fun, arg } => match fun.as_ref() {
                Term::Lam { binder, body } => {
                    self.check(ctx, metas, arg, binder.ty())?;
                    let result = body.subst(binder.level, arg);
                    self.infer(ctx, metas, &result)
                }
                _ => {
                    let fun_ty = self.infer(ctx, metas, fun)?;
                    let fun_ty = self.instantiate_implicits(metas, &fun_ty);
                    let (binder, body) = self.expect_pi(ctx, metas, &fun_ty)?;
                    self.check(ctx, metas, arg, binder.ty()).map_err(|e| match e {
                        TypeError::Message(msg) => TypeError::Message(format!(
                            "argument `{}`: {msg}",
                            binder.name_hint
                        )),
                    })?;
                    let result = body.subst(binder.level, arg);
                    Ok(metas.normalize(&peel_computation_result(&result)))
                }
            },
            Term::Let { binder, value, body } => {
                let val_ty = if binder.name_hint == "_" {
                    self.infer(ctx, metas, value)?
                } else {
                    self.check_term(ctx, metas, value, binder.ty())?;
                    binder.ty().clone()
                };
                let result = ctx.bind(&binder.name_hint, val_ty, |ctx| {
                    self.infer(ctx, metas, body)
                })?;
                Ok(metas.normalize(&result))
            }
            Term::DataCtor {
                type_def,
                variant,
                args,
            } => self.infer_data_ctor(ctx, metas, *type_def, *variant, args),
            Term::DataProj {
                value,
                type_def,
                field,
            } => self.infer_data_proj(ctx, metas, value, *type_def, *field),
            Term::DataMatch {
                scrutinee,
                enum_def,
                arms,
            } => self.infer_data_match(ctx, metas, scrutinee, *enum_def, arms),
            Term::ArrayLit { elem_ty, elems } => {
                let data = self.data_required()?;
                let info = data
                    .array_info(*elem_ty)
                    .ok_or_else(|| TypeError::Message("unknown array type".into()))?;
                if elems.len() as u32 != info.len {
                    return Err(TypeError::Message("array literal length mismatch".into()));
                }
                for elem in elems {
                    self.check(ctx, metas, elem, &info.elem)?;
                }
                Ok(Term::Global(*elem_ty))
            }
            Term::ArrayGet {
                elem_ty,
                len: _,
                arr,
                index,
            } => {
                let data = self.data_required()?;
                let info = data
                    .array_info(*elem_ty)
                    .ok_or_else(|| TypeError::Message("unknown array type".into()))?;
                self.check(ctx, metas, arr, &Term::Global(*elem_ty))?;
                self.check(ctx, metas, index, &Term::Global(prim::I32))?;
                Ok(info.elem.clone())
            }
            Term::ArraySet {
                elem_ty,
                len: _,
                arr,
                index,
                value,
            } => {
                let data = self.data_required()?;
                let info = data
                    .array_info(*elem_ty)
                    .ok_or_else(|| TypeError::Message("unknown array type".into()))?;
                self.check(ctx, metas, arr, &Term::Global(*elem_ty))?;
                self.check(ctx, metas, index, &Term::Global(prim::I32))?;
                self.check(ctx, metas, value, &info.elem)?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::AddrOf { inner_ty, value } => {
                let data = self.data_required()?;
                let info = data
                    .ptr_info(*inner_ty)
                    .ok_or_else(|| TypeError::Message("unknown pointer type".into()))?;
                self.check(ctx, metas, value, &info.inner)?;
                Ok(Term::Global(*inner_ty))
            }
            Term::Deref { inner_ty, ptr } => {
                let data = self.data_required()?;
                let info = data
                    .ptr_info(*inner_ty)
                    .ok_or_else(|| TypeError::Message("unknown pointer type".into()))?;
                self.check(ctx, metas, ptr, &Term::Global(*inner_ty))?;
                Ok(info.inner.clone())
            }
            Term::Len {
                elem_ty,
                len: _,
                arr,
            } => {
                self.check(ctx, metas, arr, &Term::Global(*elem_ty))?;
                Ok(Term::Global(prim::I32))
            }
            Term::While { cond, body } => {
                self.check(ctx, metas, cond, &Term::Global(prim::BOOL))?;
                self.check(ctx, metas, body, &Term::Global(prim::UNIT))?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::Loop { body } => {
                self.check(ctx, metas, body, &Term::Global(prim::UNIT))?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::For { start, end, body, .. } => {
                self.check(ctx, metas, start, &Term::Global(prim::I32))?;
                self.check(ctx, metas, end, &Term::Global(prim::I32))?;
                self.check(ctx, metas, body, &Term::Global(prim::UNIT))?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::Break => Ok(Term::Global(prim::UNIT)),
            Term::Assign { target, value } => {
                let target_ty = self.infer(ctx, metas, target)?;
                self.check(ctx, metas, value, &target_ty)?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::HeapAlloc { ptr_ty, value } => {
                let data = self.data_required()?;
                let info = data
                    .ptr_info(*ptr_ty)
                    .ok_or_else(|| TypeError::Message("unknown pointer type".into()))?;
                self.check(ctx, metas, value, &info.inner)?;
                Ok(Term::Global(*ptr_ty))
            }
            Term::HeapDealloc { ptr_ty, ptr } => {
                self.check(ctx, metas, ptr, &Term::Global(*ptr_ty))?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::HeapRealloc {
                ptr_ty,
                ptr,
                value,
            } => {
                let data = self.data_required()?;
                let info = data
                    .ptr_info(*ptr_ty)
                    .ok_or_else(|| TypeError::Message("unknown pointer type".into()))?;
                self.check(ctx, metas, ptr, &Term::Global(*ptr_ty))?;
                self.check(ctx, metas, value, &info.inner)?;
                Ok(Term::Global(*ptr_ty))
            }
            Term::MatrixNew {
                matrix_ty,
                outer_array_ty,
                src,
                ..
            } => {
                let data = self.data_required()?;
                data.matrix_info(*matrix_ty)
                    .ok_or_else(|| TypeError::Message("unknown matrix type".into()))?;
                self.check(ctx, metas, src, &Term::Global(*outer_array_ty))?;
                Ok(Term::Global(*matrix_ty))
            }
            Term::MatrixToArray {
                matrix_ty,
                outer_array_ty,
                matrix,
                ..
            } => {
                let data = self.data_required()?;
                data.matrix_info(*matrix_ty)
                    .ok_or_else(|| TypeError::Message("unknown matrix type".into()))?;
                self.check(ctx, metas, matrix, &Term::Global(*matrix_ty))?;
                Ok(Term::Global(*outer_array_ty))
            }
            Term::MatrixDrop { matrix_ty, matrix } => {
                let data = self.data_required()?;
                data.matrix_info(*matrix_ty)
                    .ok_or_else(|| TypeError::Message("unknown matrix type".into()))?;
                self.check(ctx, metas, matrix, &Term::Global(*matrix_ty))?;
                Ok(Term::Global(prim::UNIT))
            }
            Term::Refinement { binder, pred } => {
                self.check_is_type(ctx, metas, binder.ty())?;
                self.check(ctx, metas, pred, &Term::Global(prim::BOOL))?;
                Ok(Term::ty())
            }
            Term::Admit { .. } => Ok(Term::Global(prim::UNIT)),
            Term::Computation { result, .. } => {
                self.check_is_type(ctx, metas, result)?;
                Ok(Term::ty())
            }
            Term::Quant { kind, args } => self.infer_quant(ctx, metas, *kind, args),
        }
    }

    fn infer_quant(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        kind: QuantKind,
        args: &[Term],
    ) -> CheckResult<Term> {
        let ret = match kind {
            QuantKind::QubitNew => Term::Global(prim::QUBIT),
            QuantKind::Measure => {
                self.check(ctx, metas, &args[0], &Term::Global(prim::QUBIT))?;
                Term::Global(prim::RESULT)
            }
            QuantKind::Read => {
                self.check(ctx, metas, &args[0], &Term::Global(prim::RESULT))?;
                Term::Global(prim::BOOL)
            }
            QuantKind::Record => {
                self.infer(ctx, metas, &args[0])?;
                Term::Global(prim::UNIT)
            }
            QuantKind::GateI
            | QuantKind::GateH
            | QuantKind::GateX
            | QuantKind::GateY
            | QuantKind::GateZ
            | QuantKind::GateS
            | QuantKind::GateSdg
            | QuantKind::GateT
            | QuantKind::GateTdg => {
                self.check(ctx, metas, &args[0], &Term::Global(prim::QUBIT))?;
                Term::Global(prim::UNIT)
            }
            QuantKind::GateCnot
            | QuantKind::GateCz
            | QuantKind::GateSwap
            | QuantKind::GateCh
            | QuantKind::GateCy
            | QuantKind::GateCs
            | QuantKind::GateCsdg
            | QuantKind::GateCt
            | QuantKind::GateCtdg => {
                self.check(ctx, metas, &args[0], &Term::Global(prim::QUBIT))?;
                self.check(ctx, metas, &args[1], &Term::Global(prim::QUBIT))?;
                Term::Global(prim::UNIT)
            }
            QuantKind::GateCcnot | QuantKind::GateCcz | QuantKind::GateCswap => {
                for arg in args {
                    self.check(ctx, metas, arg, &Term::Global(prim::QUBIT))?;
                }
                Term::Global(prim::UNIT)
            }
            QuantKind::GateRx
            | QuantKind::GateRy
            | QuantKind::GateRz
            | QuantKind::GateR1 => {
                self.check(ctx, metas, &args[0], &Term::Global(prim::F64))?;
                self.check(ctx, metas, &args[1], &Term::Global(prim::QUBIT))?;
                Term::Global(prim::UNIT)
            }
            QuantKind::GateCrx
            | QuantKind::GateCry
            | QuantKind::GateCrz
            | QuantKind::GateCr1 => {
                self.check(ctx, metas, &args[0], &Term::Global(prim::F64))?;
                self.check(ctx, metas, &args[1], &Term::Global(prim::QUBIT))?;
                self.check(ctx, metas, &args[2], &Term::Global(prim::QUBIT))?;
                Term::Global(prim::UNIT)
            }
        };
        Ok(ret)
    }

    pub fn check_lambda(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        binder: &Binder,
        body: &Term,
        expected: &Term,
    ) -> CheckResult<()> {
        let expected = self.instantiate_implicits(metas, expected);
        let (pi_binder, pi_body) = self.expect_pi(ctx, metas, &expected)?;
        if !self.def_eq(ctx, metas, pi_binder.ty(), binder.ty()) {
            unify(ctx, self.globals, metas, pi_binder.ty(), binder.ty())
                .map_err(|e| TypeError::Message(e.to_string()))?;
        }
        let body_expected =
            peel_computation_result(&pi_body.subst(pi_binder.level, &Term::Var(binder.level)));
        ctx.bind(&binder.name_hint, (*binder.ty).clone(), |ctx| {
            self.check_term(ctx, metas, body, &body_expected)
        })
    }

    pub fn check_term(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        term: &Term,
        expected: &Term,
    ) -> CheckResult<()> {
        match term {
            Term::Lam { binder, body } => {
                self.check_lambda(ctx, metas, binder, body, expected)
            }
            _ => self.check(ctx, metas, term, expected),
        }
    }

    pub fn def_eq(&self, ctx: &TypingCtx, metas: &MetaEnv, t1: &Term, t2: &Term) -> bool {
        let t1 = metas.normalize(t1);
        let t2 = metas.normalize(t2);
        let env = typing_env(ctx);
        is_def_eq(&env, self.globals, &t1, &t2)
    }

    fn instantiate_implicits(&self, metas: &mut MetaEnv, ty: &Term) -> Term {
        let mut cur = metas.normalize(ty);
        loop {
            match &cur {
                Term::Pi { binder, body }
                    if binder.explicitness == Explicitness::Implicit =>
                {
                    let meta = metas.fresh_implicit(&binder.name_hint);
                    cur = body.subst(binder.level, &Term::Meta(meta));
                }
                _ => break,
            }
        }
        cur
    }

    fn check_is_type(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        term: &Term,
    ) -> CheckResult<()> {
        if self.def_eq(ctx, metas, term, &Term::ty()) {
            return Ok(());
        }
        let inferred = self.infer(ctx, metas, term)?;
        if self.def_eq(ctx, metas, &inferred, &Term::ty()) {
            Ok(())
        } else {
            Err(TypeError::Message(format!(
                "expected a type, got `{inferred:?}`"
            )))
        }
    }

    fn expect_pi(
        &self,
        _ctx: &TypingCtx,
        metas: &MetaEnv,
        term: &Term,
    ) -> CheckResult<(Binder, Term)> {
        let term = skip_implicit_pis(metas, term);
        let term = metas.normalize(&term);
        match term {
            Term::Pi { binder, body } => Ok((binder, metas.normalize(&body))),
            other => Err(TypeError::Message(format!("expected Pi type, got `{other:?}`"))),
        }
    }

    fn data_required(&self) -> CheckResult<&DataEnv> {
        self.data
            .ok_or_else(|| TypeError::Message("missing nominal metadata".into()))
    }

    fn infer_data_ctor(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        type_def: DefId,
        variant: u32,
        args: &[Term],
    ) -> CheckResult<Term> {
        let data = self.data_required()?;

        if let Some(info) = data.inductive(type_def) {
            return self.infer_inductive_ctor(ctx, metas, type_def, info, variant, args);
        }

        if let Some(fields) = data.struct_fields(type_def) {
            if variant != 0 {
                return Err(TypeError::Message("struct ctor variant must be 0".into()));
            }
            if args.len() != fields.len() {
                return Err(TypeError::Message("struct ctor arity mismatch".into()));
            }
            for (arg, expected) in args.iter().zip(fields.iter()) {
                self.check(ctx, metas, arg, expected)?;
            }
            return Ok(Term::Global(type_def));
        }

        let variant_fields = data
            .variant_fields(type_def, variant)
            .ok_or_else(|| TypeError::Message(format!("unknown variant {variant}")))?;
        let expected = match variant_fields {
            VariantFields::Unit => {
                if !args.is_empty() {
                    return Err(TypeError::Message("unit variant expects no args".into()));
                }
                return Ok(Term::Global(type_def));
            }
            VariantFields::Tuple(fields) | VariantFields::Struct(fields) => fields,
        };
        if args.len() != expected.len() {
            return Err(TypeError::Message("enum ctor arity mismatch".into()));
        }
        for (arg, expected_ty) in args.iter().zip(expected.iter()) {
            self.check(ctx, metas, arg, expected_ty)?;
        }
        Ok(Term::Global(type_def))
    }

    fn infer_data_proj(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        value: &Term,
        type_def: DefId,
        field: u32,
    ) -> CheckResult<Term> {
        let data = self.data_required()?;
        let val_ty = self.infer(ctx, metas, value)?;
        if !self.def_eq(ctx, metas, &val_ty, &Term::Global(type_def)) {
            return Err(TypeError::Message("projection on wrong type".into()));
        }
        let fields = data
            .struct_fields(type_def)
            .ok_or_else(|| TypeError::Message("projection requires struct type".into()))?;
        fields
            .get(field as usize)
            .cloned()
            .ok_or_else(|| TypeError::Message(format!("unknown field index {field}")))
    }

    fn infer_inductive_ctor(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        type_def: DefId,
        info: &crate::core::data::InductiveInfo,
        variant: u32,
        args: &[Term],
    ) -> CheckResult<Term> {
        let ctor = info
            .constructors
            .get(variant as usize)
            .ok_or_else(|| TypeError::Message(format!("unknown inductive ctor {variant}")))?;
        let mut cur = self.instantiate_implicits(metas, &ctor.ty);
        for arg in args {
            let (binder, body) = self.expect_pi(ctx, metas, &cur)?;
            self.check(ctx, metas, arg, binder.ty())?;
            cur = body.subst(binder.level, arg);
        }
        let (extra, _) = crate::core::inductive::ctor_arg_types(&cur);
        if !extra.is_empty() {
            return Err(TypeError::Message(format!(
                "inductive ctor `{}` expects more arguments",
                ctor.name
            )));
        }
        let _ = type_def;
        Ok(metas.normalize(&cur))
    }

    fn infer_data_match(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        scrutinee: &Term,
        enum_def: DefId,
        arms: &[MatchArm],
    ) -> CheckResult<Term> {
        let data = self.data_required()?;
        let scrut_ty = self.infer(ctx, metas, scrutinee)?;
        if let Some(info) = data.inductive(enum_def) {
            let param_count = info.params.len();
            let index_count = info.indices.len();
            let (params, indices) = if param_count + index_count == 0 {
                if !self.def_eq(ctx, metas, &scrut_ty, &Term::Global(enum_def)) {
                    return Err(TypeError::Message(
                        "match scrutinee type mismatch for inductive".into(),
                    ));
                }
                (vec![], vec![])
            } else {
                crate::core::inductive::family_instance_parts(
                    &scrut_ty,
                    enum_def,
                    param_count,
                    index_count,
                )
                .ok_or_else(|| {
                    TypeError::Message("match scrutinee is not a family instance".into())
                })?
            };
            return self.infer_dependent_inductive_match(
                ctx, metas, info, enum_def, &params, &indices, arms,
            );
        }
        if !self.def_eq(ctx, metas, &scrut_ty, &Term::Global(enum_def)) {
            return Err(TypeError::Message("match scrutinee type mismatch".into()));
        }
        let variants = data
            .enum_variants(enum_def)
            .ok_or_else(|| TypeError::Message("match on non-enum type".into()))?;

        let mut result_ty: Option<Term> = None;
        for arm in arms {
            let variant_fields = variants
                .get(arm.variant_index as usize)
                .ok_or_else(|| TypeError::Message("unknown match variant".into()))?;
            let mut arm_ctx = TypingCtx::default();
            let arm_ty = arm_ctx.bind_variant_fields(variant_fields, |arm_ctx| {
                self.infer(arm_ctx, metas, &arm.body)
            })?;
            match &result_ty {
                None => result_ty = Some(arm_ty),
                Some(prev) => {
                    if !self.def_eq(ctx, metas, prev, &arm_ty) {
                        return Err(TypeError::Message("match arm type mismatch".into()));
                    }
                }
            }
        }
        result_ty.ok_or_else(|| TypeError::Message("empty match".into()))
    }

    fn infer_dependent_inductive_match(
        &self,
        ctx: &mut TypingCtx,
        metas: &mut MetaEnv,
        info: &crate::core::data::InductiveInfo,
        family: DefId,
        params: &[Term],
        scrut_indices: &[Term],
        arms: &[MatchArm],
    ) -> CheckResult<Term> {
        let nat_family = info
            .indices
            .first()
            .and_then(|b| match b.ty() {
                Term::Global(id) => Some(*id),
                _ => None,
            });

        let mut result_ty: Option<Term> = None;
        for arm in arms {
            let ctor = info
                .constructors
                .get(arm.variant_index as usize)
                .ok_or_else(|| TypeError::Message("unknown match variant".into()))?;

            let mut field_tys = crate::core::inductive::ctor_arg_types(&ctor.ty).0;
            for ty in &mut field_tys {
                *ty = crate::core::inductive::subst_family_params(info, params, ty);
            }

            let arm_ty = if arm.variant_index == 0 && field_tys.is_empty() {
                if !scrut_indices.is_empty() {
                    let nat = nat_family
                        .ok_or_else(|| TypeError::Message("missing index type".into()))?;
                    unify(
                        ctx,
                        self.globals,
                        metas,
                        &scrut_indices[0],
                        &crate::core::inductive::seed::zero_ctor(nat),
                    )
                    .map_err(|e| TypeError::Message(e.to_string()))?;
                }
                self.infer(&mut TypingCtx::default(), metas, &arm.body)?
            } else if arm.variant_index == 1 && field_tys.len() >= 3 {
                let nat = nat_family
                    .ok_or_else(|| TypeError::Message("missing index type".into()))?;
                let rest = field_tys[1..].to_vec();
                let mut arm_ctx = TypingCtx::default();
                arm_ctx.bind("n", field_tys[0].clone(), |ctx| {
                    if !scrut_indices.is_empty() {
                        let expected = crate::core::inductive::seed::succ_ctor(
                            nat,
                            Term::Var(Level(0)),
                        );
                        unify(ctx, self.globals, metas, &scrut_indices[0], &expected)
                            .map_err(|e| TypeError::Message(e.to_string()))?;
                    }
                    ctx.bind_variant_fields(
                        &crate::core::data::VariantFields::Tuple(rest),
                        |ctx| self.infer(ctx, metas, &arm.body),
                    )
                })?
            } else if field_tys.is_empty() {
                self.infer(&mut TypingCtx::default(), metas, &arm.body)?
            } else {
                let mut arm_ctx = TypingCtx::default();
                arm_ctx.bind_variant_fields(
                    &crate::core::data::VariantFields::Tuple(field_tys),
                    |arm_ctx| self.infer(arm_ctx, metas, &arm.body),
                )?
            };
            let _ = family;
            match &result_ty {
                None => result_ty = Some(arm_ty),
                Some(prev) => {
                    if !self.def_eq(ctx, metas, prev, &arm_ty) {
                        return Err(TypeError::Message("match arm type mismatch".into()));
                    }
                }
            }
        }
        result_ty.ok_or_else(|| TypeError::Message("empty match".into()))
    }

    fn format_unify_error(
        &self,
        metas: &MetaEnv,
        inferred: &Term,
        expected: &Term,
        err: &crate::core::unify::UnifyError,
    ) -> String {
        let inferred = metas.normalize(inferred);
        let expected = metas.normalize(expected);
        if let Some(msg) =
            metas.implicit_unify_hint(&inferred, &expected, self.globals)
        {
            return msg;
        }
        match err {
            crate::core::unify::UnifyError::Message(msg) if !msg.is_empty() => msg.clone(),
            _ => format!(
                "cannot unify `{}` with `{}`",
                crate::core::meta::format_type(&inferred, self.globals),
                crate::core::meta::format_type(&expected, self.globals)
            ),
        }
    }
}

fn peel_computation_result(ty: &Term) -> Term {
    match ty {
        Term::Computation { result, .. } => peel_computation_result(result),
        other => other.clone(),
    }
}

fn peel_refinement<'a>(ty: &'a Term) -> (&'a Term, Option<&'a Term>) {
    match ty {
        Term::Refinement { binder, pred } => (binder.ty(), Some(pred.as_ref())),
        _ => (ty, None),
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
