use std::collections::HashMap;

use crate::core::term::{Explicitness, Relevance};
use crate::elab::ids::{array_gid, builtin_gid, ptr_gid};

use crate::core::data::{DataEnv, EnumInfo, StructInfo, VariantFields};
use crate::core::globals::GlobalEnv;
use crate::core::inductive::seed;
use crate::core::effect::{join_effect, Effect, is_subeffect};
use crate::core::term::{Binder, Level, Term};
use crate::elab::ids::{fn_gid, type_gid};
use crate::elab::prelude::Prelude;
use crate::elab::symbols::{BinOp, CodegenSymbols, RuntimeBuiltin, RuntimeTy};
use crate::elab::ty::{elab_ty, elab_ty_for_param};
use crate::frontend::resolve::{
    ConstructorInfo, DefId, ResolvedModule, TypeDefKind,
};
use crate::frontend::surface::{EnumVariantFields, SurfaceTy};

#[derive(Debug, Clone, Copy)]
pub struct CoreInductive {
    pub nat: crate::frontend::resolve::DefId,
    pub nat_add: crate::frontend::resolve::DefId,
    pub vec: crate::frontend::resolve::DefId,
    pub vec_append: crate::frontend::resolve::DefId,
}

pub struct ElabEnv<'a> {
    pub resolved: &'a ResolvedModule,
    pub globals: GlobalEnv,
    pub data: DataEnv,
    pub prelude: Prelude,
    pub core_inductive: CoreInductive,
    pub enum_println: HashMap<crate::frontend::resolve::DefId, crate::frontend::resolve::DefId>,
    pub array_println: HashMap<crate::frontend::resolve::DefId, crate::frontend::resolve::DefId>,
    pub struct_println: HashMap<crate::frontend::resolve::DefId, crate::frontend::resolve::DefId>,
    pub ptr_println: HashMap<crate::frontend::resolve::DefId, crate::frontend::resolve::DefId>,
    pub matrix_println: HashMap<crate::frontend::resolve::DefId, crate::frontend::resolve::DefId>,
    next_builtin: u32,
    locals: HashMap<String, (Level, Term)>,
    local_stack: Vec<String>,
    type_params: HashMap<String, Level>,
    pub(crate) loop_depth: u32,
    pub(crate) while_depth: u32,
    pub(crate) quant_depth: u32,
    pub(crate) gpu_depth: u32,
    effect_floor: Effect,
    effect_used: Effect,
    qubit_affine: Vec<HashMap<String, super::affine::QubitState>>,
}

impl<'a> ElabEnv<'a> {
    pub fn new(resolved: &'a ResolvedModule) -> Result<Self, String> {
        let mut globals = GlobalEnv::with_primitives();
        let prelude = Prelude::register(&mut globals);
        let next_builtin = prelude.next_builtin;
        let mut data = DataEnv::default();
        let nat = seed::register_nat(&mut globals, &mut data)?;
        seed::register_nat_add_value(&mut globals, nat.family, nat.add);
        let vec = seed::register_vec(&mut globals, &mut data)?;
        seed::register_append_value(
            &mut globals,
            nat.family,
            vec.family,
            nat.add,
            vec.append,
        );
        let core_inductive = CoreInductive {
            nat: nat.family,
            nat_add: nat.add,
            vec: vec.family,
            vec_append: vec.append,
        };
        let mut env = Self {
            resolved,
            globals,
            data,
            prelude,
            core_inductive,
            enum_println: HashMap::new(),
            array_println: HashMap::new(),
            struct_println: HashMap::new(),
            ptr_println: HashMap::new(),
            matrix_println: HashMap::new(),
            next_builtin,
            locals: HashMap::new(),
            local_stack: Vec::new(),
            type_params: HashMap::new(),
            loop_depth: 0,
            while_depth: 0,
            quant_depth: 0,
            gpu_depth: 0,
            effect_floor: Effect::Tot,
            effect_used: Effect::Tot,
            qubit_affine: Vec::new(),
        };
        env.register_types()?;
        env.register_fn_sigs()?;
        Ok(env)
    }

    pub fn type_gid(&self, kind: TypeDefKind) -> crate::frontend::resolve::DefId {
        type_gid(kind)
    }

    pub fn fn_gid(&self, resolved: crate::frontend::resolve::DefId) -> crate::frontend::resolve::DefId {
        fn_gid(resolved)
    }

    pub fn current_level(&self) -> Level {
        Level(self.local_stack.len() as u32)
    }

    pub fn push_local(&mut self, name: &str, ty: Term) -> Level {
        let level = self.current_level();
        self.locals.insert(name.to_string(), (level, ty));
        self.local_stack.push(name.to_string());
        level
    }

    pub fn lookup_local(&self, name: &str) -> Option<(Level, Term)> {
        self.locals
            .get(name)
            .map(|(level, ty)| (*level, ty.clone()))
    }

    pub fn lookup_type_param(&self, name: &str) -> Option<Level> {
        self.type_params.get(name).copied()
    }

    pub fn enter_loop_stmt<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.loop_depth += 1;
        let out = f(self);
        self.loop_depth -= 1;
        out
    }

    pub fn enter_while_loop<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.while_depth += 1;
        let out = f(self);
        self.while_depth -= 1;
        out
    }

    pub fn begin_fn(&mut self, gid: crate::frontend::resolve::DefId, is_quantum: bool) {
        self.effect_floor = self.globals.effect_of(gid).unwrap_or(Effect::Tot);
        self.effect_used = self.effect_floor;
        if is_quantum {
            self.enter_quant_affine_scope();
        }
    }

    pub fn end_fn(&mut self, is_quantum: bool) -> Effect {
        if is_quantum {
            self.leave_quant_affine_scope();
        }
        self.effect_used
    }

    pub fn in_qubit_affine_scope(&self) -> bool {
        !self.qubit_affine.is_empty()
    }

    pub(crate) fn qubit_affine_mut(&mut self) -> Option<&mut HashMap<String, super::affine::QubitState>> {
        self.qubit_affine.last_mut()
    }

    pub fn qubit_state(&self, name: &str) -> Option<super::affine::QubitState> {
        self.qubit_affine.last()?.get(name).copied()
    }

    fn enter_quant_affine_scope(&mut self) {
        self.quant_depth += 1;
        self.qubit_affine.push(HashMap::new());
    }

    fn leave_quant_affine_scope(&mut self) {
        self.quant_depth = self.quant_depth.saturating_sub(1);
        self.qubit_affine.pop();
    }

    pub fn require_effect(&mut self, effect: Effect, what: &str) -> Result<(), String> {
        if !is_subeffect(effect, self.effect_floor) {
            return Err(format!(
                "{what} requires `{}` effect, but this function is `{}`",
                effect.as_str(),
                self.effect_floor.as_str()
            ));
        }
        self.effect_used = join_effect(self.effect_used, effect);
        Ok(())
    }

    pub fn check_call_effect(&self, callee: Effect, label: &str) -> Result<(), String> {
        if !is_subeffect(callee, self.effect_floor) {
            return Err(format!(
                "effect mismatch calling `{label}`: requires `{}`, but this function is `{}`",
                callee.as_str(),
                self.effect_floor.as_str()
            ));
        }
        Ok(())
    }

    pub fn require_quant_scope(&self, what: &str) -> Result<(), String> {
        if self.quant_depth == 0 {
            return Err(format!("{what} requires a `quant {{ }}` scope"));
        }
        Ok(())
    }

    pub fn require_gpu_scope(&self, what: &str) -> Result<(), String> {
        if self.gpu_depth == 0 {
            return Err(format!("{what} requires a `gpu {{ }}` scope"));
        }
        Ok(())
    }

    pub fn with_quant_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.enter_quant_affine_scope();
        let out = f(self);
        self.leave_quant_affine_scope();
        out
    }

    pub fn with_gpu_scope<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.gpu_depth += 1;
        let out = f(self);
        self.gpu_depth -= 1;
        out
    }

    pub fn with_type_params<R>(
        &mut self,
        params: &[(String, Level)],
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        for (name, level) in params {
            self.type_params.insert(name.clone(), *level);
        }
        let out = f(self);
        for (name, _) in params {
            self.type_params.remove(name);
        }
        out
    }

    pub fn pop_local(&mut self) {
        if let Some(name) = self.local_stack.pop() {
            self.locals.remove(&name);
        }
    }

    pub fn with_local<R>(&mut self, name: &str, ty: Term, f: impl FnOnce(&mut Self) -> R) -> R {
        let _level = self.push_local(name, ty);
        let out = f(self);
        self.pop_local();
        out
    }

    pub fn with_locals<R>(
        &mut self,
        names: &[String],
        types: &[Term],
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        if names.is_empty() {
            return f(self);
        }
        let ty = types[0].clone();
        let name = names[0].clone();
        self.with_local(&name, ty, |env| {
            env.with_locals(&names[1..], &types[1..], f)
        })
    }

    /// Binds locals starting at `Level(0)` regardless of the outer scope.
    pub fn with_locals_isolated<R>(
        &mut self,
        names: &[String],
        types: &[Term],
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_stack = std::mem::take(&mut self.local_stack);
        let out = self.with_locals(names, types, f);
        self.locals = saved_locals;
        self.local_stack = saved_stack;
        out
    }

    pub fn ctor_for_variant(&self, enum_name: &str, variant: &str) -> Option<&ConstructorInfo> {
        self.resolved.constructors.iter().find(|c| {
            c.enum_name == enum_name && c.variant_name == variant
        })
    }

    fn register_types(&mut self) -> Result<(), String> {
        for s in &self.resolved.structs {
            if s.is_builtin {
                continue;
            }
            let gid = type_gid(TypeDefKind::Struct(s.id));
            self.globals.insert_type(gid, Term::ty());
            let fields = s
                .def
                .fields
                .iter()
                .map(|(_, ty)| elab_ty(self, ty))
                .collect::<Result<Vec<_>, _>>()?;
            self.data.structs.insert(
                gid,
                StructInfo { fields },
            );
            self.register_struct_println(gid);
        }

        for e in &self.resolved.enums {
            let gid = type_gid(TypeDefKind::Enum(e.id));
            self.globals.insert_type(gid, Term::ty());
            let mut variants = Vec::new();
            for variant in &e.def.variants {
                let fields = match &variant.fields {
                    EnumVariantFields::Unit => VariantFields::Unit,
                    EnumVariantFields::Tuple(types) => {
                        let ts = types
                            .iter()
                            .map(|ty| elab_ty(self, ty))
                            .collect::<Result<Vec<_>, _>>()?;
                        VariantFields::Tuple(ts)
                    }
                    EnumVariantFields::Struct(fields) => {
                        let ts = fields
                            .iter()
                            .map(|(_, ty)| elab_ty(self, ty))
                            .collect::<Result<Vec<_>, _>>()?;
                        VariantFields::Struct(ts)
                    }
                };
                variants.push(fields);
            }
            self.data.enums.insert(gid, EnumInfo { variants });
            self.register_enum_println(gid);
        }

        Ok(())
    }

    fn alloc_builtin(&mut self) -> crate::frontend::resolve::DefId {
        let id = builtin_gid(self.next_builtin);
        self.next_builtin += 1;
        id
    }

    fn register_enum_println(&mut self, enum_gid: crate::frontend::resolve::DefId) {
        let println = self.alloc_builtin();
        let enum_ty = Term::Global(enum_gid);
        let unit = Term::Global(crate::core::globals::prim::UNIT);
        self.globals.insert_type(
            println,
            Term::Pi {
                binder: Binder::new("_", Level(0), enum_ty),
                body: Box::new(unit),
            },
        );
        self.enum_println.insert(enum_gid, println);
    }

    fn register_struct_println(&mut self, struct_gid: crate::frontend::resolve::DefId) {
        let println = self.alloc_builtin();
        let struct_ty = Term::Global(struct_gid);
        let unit = Term::Global(crate::core::globals::prim::UNIT);
        self.globals.insert_type(
            println,
            Term::Pi {
                binder: Binder::new("_", Level(0), struct_ty),
                body: Box::new(unit),
            },
        );
        self.struct_println.insert(struct_gid, println);
    }

    fn register_ptr_println(&mut self, ptr_gid: crate::frontend::resolve::DefId) {
        let println = self.alloc_builtin();
        let ptr_ty = Term::Global(ptr_gid);
        let unit = Term::Global(crate::core::globals::prim::UNIT);
        self.globals.insert_type(
            println,
            Term::Pi {
                binder: Binder::new("_", Level(0), ptr_ty),
                body: Box::new(unit),
            },
        );
        self.ptr_println.insert(ptr_gid, println);
    }

    fn register_fn_sigs(&mut self) -> Result<(), String> {
        for f in &self.resolved.fns {
            let gid = fn_gid(f.id);
            let sig = fn_type_for_def(self, &f.def.params, f.def.ret.as_ref(), f.def.is_quantum)?;
            let effect = crate::elab::effect::effect_of_fn_def(&f.def);
            self.globals.insert_type(gid, sig);
            self.globals.insert_effect(gid, effect);
            self.globals.update_fn_return_effect(gid, effect);
        }
        Ok(())
    }

    pub fn finish_fn(&mut self, resolved_id: crate::frontend::resolve::DefId, term: Term) {
        self.globals.insert_value(fn_gid(resolved_id), term);
    }

    pub fn build_symbols(&self) -> Result<CodegenSymbols, String> {
        let mut out = CodegenSymbols::default();
        for f in &self.resolved.fns {
            out.fns.insert(fn_gid(f.id), f.name.clone());
        }
        for s in &self.resolved.structs {
            if s.is_builtin {
                continue;
            }
            let gid = type_gid(TypeDefKind::Struct(s.id));
            out.structs.insert(gid, s.name.clone());
            if let Some(println) = self.struct_println.get(&gid) {
                out.builtins.insert(
                    *println,
                    RuntimeBuiltin::Println(RuntimeTy::Struct(s.name.clone())),
                );
            }
        }
        for e in &self.resolved.enums {
            let gid = type_gid(TypeDefKind::Enum(e.id));
            out.enums.insert(gid, e.name.clone());
            if let Some(println) = self.enum_println.get(&gid) {
                out.builtins.insert(
                    *println,
                    RuntimeBuiltin::Println(RuntimeTy::Enum(e.name.clone())),
                );
            }
        }
        self.register_prim_builtins(&mut out)?;
        for (gid, rt) in &out.arrays {
            if let Some(println) = self.array_println.get(gid) {
                out.builtins.insert(*println, RuntimeBuiltin::Println(rt.clone()));
            }
        }
        for (gid, rt) in &out.ptrs {
            if let Some(println) = self.ptr_println.get(gid) {
                out.builtins.insert(*println, RuntimeBuiltin::Println(rt.clone()));
            }
        }
        for (gid, rt) in &out.matrices {
            if let Some(println) = self.matrix_println.get(gid) {
                out.builtins.insert(*println, RuntimeBuiltin::Println(rt.clone()));
            }
        }
        Ok(out)
    }

    fn register_prim_builtins(&self, out: &mut CodegenSymbols) -> Result<(), String> {
        for (ops, ty) in [
            (self.prelude.i8, RuntimeTy::I8),
            (self.prelude.u8, RuntimeTy::U8),
            (self.prelude.i16, RuntimeTy::I16),
            (self.prelude.u16, RuntimeTy::U16),
            (self.prelude.i32, RuntimeTy::I32),
            (self.prelude.i64, RuntimeTy::I64),
            (self.prelude.u64, RuntimeTy::U64),
            (self.prelude.i128, RuntimeTy::I128),
            (self.prelude.u128, RuntimeTy::U128),
            (self.prelude.f16, RuntimeTy::F16),
            (self.prelude.f32, RuntimeTy::F32),
            (self.prelude.f64, RuntimeTy::F64),
            (self.prelude.string, RuntimeTy::String),
            (self.prelude.bool_, RuntimeTy::Bool),
            (self.prelude.unit_, RuntimeTy::Unit),
        ] {
            let pairs = [
                (ops.add, BinOp::Add),
                (ops.sub, BinOp::Sub),
                (ops.mul, BinOp::Mul),
                (ops.div, BinOp::Div),
                (ops.rem, BinOp::Rem),
                (ops.bitand, BinOp::BitAnd),
                (ops.bitor, BinOp::BitOr),
                (ops.bitxor, BinOp::BitXor),
                (ops.shl, BinOp::Shl),
                (ops.shr, BinOp::Shr),
            ];
            for (id, op) in pairs {
                out.builtins
                    .insert(id, RuntimeBuiltin::BinOp(op, ty.clone()));
            }
            out.builtins.insert(
                ops.lt,
                RuntimeBuiltin::Cmp(crate::elab::symbols::CmpOp::Lt, ty.clone()),
            );
            out.builtins.insert(
                ops.gt,
                RuntimeBuiltin::Cmp(crate::elab::symbols::CmpOp::Gt, ty.clone()),
            );
            out.builtins
                .insert(ops.if_then_else, RuntimeBuiltin::If(ty.clone()));
            out.builtins
                .insert(ops.println, RuntimeBuiltin::Println(ty.clone()));
            if matches!(ty, RuntimeTy::String) {
                out.builtins
                    .insert(ops.str_eq, RuntimeBuiltin::StrEq);
            }
        }
        for _ in 0..=self.data.arrays.len() {
            for (gid, info) in &self.data.arrays {
                if out.arrays.contains_key(gid) {
                    continue;
                }
                if let Ok(rt) = runtime_ty_from_core(&info.elem, out) {
                    out.arrays.insert(
                        *gid,
                        RuntimeTy::Array {
                            elem: Box::new(rt),
                            len: info.len,
                        },
                    );
                }
            }
            if out.arrays.len() == self.data.arrays.len() {
                break;
            }
        }
        if out.arrays.len() != self.data.arrays.len() {
            return Err("failed to resolve nested array runtime types".into());
        }
        for (gid, info) in &self.data.ptrs {
            let rt = runtime_ty_from_core(&info.inner, out)?;
            out.ptrs.insert(*gid, RuntimeTy::Ptr(Box::new(rt)));
        }
        for (gid, info) in &self.data.matrices {
            let rt = runtime_ty_from_core(&info.elem, out)?;
            out.matrices
                .insert(*gid, RuntimeTy::Matrix { elem: Box::new(rt) });
        }
        Ok(())
    }
}

fn runtime_ty_from_core(term: &Term, out: &CodegenSymbols) -> Result<RuntimeTy, String> {
    match term {
        Term::Global(id) => {
            if let Some(rt) = RuntimeTy::from_prim(*id) {
                return Ok(rt);
            }
            if let Some(name) = out.structs.get(id) {
                return Ok(RuntimeTy::Struct(name.clone()));
            }
            if let Some(name) = out.enums.get(id) {
                return Ok(RuntimeTy::Enum(name.clone()));
            }
            if let Some(rt) = out.arrays.get(id) {
                return Ok(rt.clone());
            }
            if let Some(rt) = out.ptrs.get(id) {
                return Ok(rt.clone());
            }
            if let Some(rt) = out.matrices.get(id) {
                return Ok(rt.clone());
            }
            Err(format!("unknown type global `{id:?}`"))
        }
        _ => Err(format!("expected type global, got `{term:?}`")),
    }
}

fn is_implicit_type_param(ty: &SurfaceTy) -> bool {
    matches!(ty, SurfaceTy::Struct(name) if name == "Type")
}

pub fn type_param_bindings(
    params: &[(String, SurfaceTy, bool)],
) -> Vec<(String, Level)> {
    params
        .iter()
        .enumerate()
        .filter_map(|(i, (name, ty, implicit))| {
            if *implicit && is_implicit_type_param(ty) {
                Some((name.clone(), Level(i as u32)))
            } else {
                None
            }
        })
        .collect()
}

pub fn fn_type_for_def(
    env: &mut ElabEnv,
    params: &[(String, SurfaceTy, bool)],
    ret: Option<&SurfaceTy>,
    is_quantum: bool,
) -> Result<Term, String> {
    if is_quantum {
        env.with_quant_scope(|env| fn_type(env, params, ret))
    } else {
        fn_type(env, params, ret)
    }
}

pub fn fn_type(
    env: &mut ElabEnv,
    params: &[(String, SurfaceTy, bool)],
    ret: Option<&SurfaceTy>,
) -> Result<Term, String> {
    let type_params = type_param_bindings(params);
    env.with_type_params(&type_params, |env| {
        let ret_ty = match ret {
            Some(ty) => elab_ty(env, ty)?,
            None => Term::Global(crate::core::globals::prim::UNIT),
        };
        let mut ty = Term::computation(Effect::Tot, ret_ty);
        for (i, (name, param_ty, implicit)) in params.iter().enumerate().rev() {
            let level = Level(i as u32);
            let domain = elab_ty_for_param(env, param_ty, Some(name), Some(level))?;
            let mut binder = Binder::new(name, level, domain);
            if *implicit {
                binder.explicitness = Explicitness::Implicit;
                binder.relevance = Relevance::Erased;
            }
            ty = Term::Pi {
                binder,
                body: Box::new(ty),
            };
        }
        Ok(ty)
    })
}

impl<'a> ElabEnv<'a> {
    pub fn register_array_type(&mut self, elem: &Term, len: u32) -> DefId {
        let elem_gid = match elem {
            Term::Global(id) => *id,
            _ => panic!("array element must be a global type"),
        };
        let gid = array_gid(elem_gid, len);
        if self.globals.type_of(gid).is_none() {
            self.globals.insert_type(gid, Term::ty());
            self.data.arrays.insert(
                gid,
                crate::core::data::ArrayInfo {
                    elem: elem.clone(),
                    len,
                },
            );
            let println = self.alloc_builtin();
            self.globals.insert_type(
                println,
                Term::Pi {
                    binder: Binder::new("_", Level(0), Term::Global(gid)),
                    body: Box::new(Term::Global(crate::core::globals::prim::UNIT)),
                },
            );
            self.array_println.insert(gid, println);
        }
        gid
    }

    pub fn register_matrix_type(&mut self, elem: &Term) -> DefId {
        let elem_gid = match elem {
            Term::Global(id) => *id,
            _ => panic!("matrix element must be a global type"),
        };
        let gid = crate::elab::ids::matrix_gid(elem_gid);
        if self.globals.type_of(gid).is_none() {
            self.globals.insert_type(gid, Term::ty());
            self.data.matrices.insert(
                gid,
                crate::core::data::MatrixInfo {
                    elem: elem.clone(),
                },
            );
            let println = self.alloc_builtin();
            self.globals.insert_type(
                println,
                Term::Pi {
                    binder: Binder::new("_", Level(0), Term::Global(gid)),
                    body: Box::new(Term::Global(crate::core::globals::prim::UNIT)),
                },
            );
            self.matrix_println.insert(gid, println);
        }
        gid
    }

    pub fn register_ptr_type(&mut self, inner: &Term) -> DefId {
        let inner_gid = match inner {
            Term::Global(id) => *id,
            _ => panic!("pointer inner must be a global type"),
        };
        let gid = ptr_gid(inner_gid);
        if self.globals.type_of(gid).is_none() {
            self.globals.insert_type(gid, Term::ty());
            self.data.ptrs.insert(
                gid,
                crate::core::data::PtrInfo {
                    inner: inner.clone(),
                },
            );
            self.register_ptr_println(gid);
        }
        gid
    }
}

impl<'a> ElabEnv<'a> {
    pub fn struct_name_for_ty(&self, ty: &Term) -> Option<String> {
        let Term::Global(id) = ty else {
            return None;
        };
        if let Some(name) = self
            .resolved
            .structs
            .iter()
            .find(|s| self.type_gid(TypeDefKind::Struct(s.id)) == *id)
            .map(|s| s.name.clone())
        {
            return Some(name);
        }
        let info = self.data.ptrs.get(id)?;
        self.struct_name_for_ty(&info.inner)
    }
}
