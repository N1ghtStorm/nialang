use crate::core::globals::{prim, GlobalEnv};
use crate::core::term::Term;
use crate::elab::ids::builtin_gid;
use crate::frontend::resolve::DefId;

#[derive(Debug, Clone, Copy)]
pub struct BuiltinOps {
    pub add: DefId,
    pub sub: DefId,
    pub mul: DefId,
    pub div: DefId,
    pub rem: DefId,
    pub bitand: DefId,
    pub bitor: DefId,
    pub bitxor: DefId,
    pub shl: DefId,
    pub shr: DefId,
    pub lt: DefId,
    pub gt: DefId,
    pub if_then_else: DefId,
    pub println: DefId,
    pub str_eq: DefId,
}

pub struct Prelude {
    pub next_builtin: u32,
    pub i8: BuiltinOps,
    pub u8: BuiltinOps,
    pub i16: BuiltinOps,
    pub u16: BuiltinOps,
    pub i32: BuiltinOps,
    pub i64: BuiltinOps,
    pub u64: BuiltinOps,
    pub i128: BuiltinOps,
    pub u128: BuiltinOps,
    pub f16: BuiltinOps,
    pub f32: BuiltinOps,
    pub f64: BuiltinOps,
    pub string: BuiltinOps,
    pub bool_: BuiltinOps,
    pub unit_: BuiltinOps,
}

impl Prelude {
    pub fn register(globals: &mut GlobalEnv) -> Self {
        let mut next = 0u32;
        let mut alloc = || {
            let id = builtin_gid(next);
            next += 1;
            id
        };

        let mut mk_numeric = |ty: DefId| {
            let ty_term = Term::Global(ty);
            let unit = Term::Global(prim::UNIT);
            let bin_ty = pi(ty_term.clone(), pi(ty_term.clone(), ty_term.clone()));
            let add = alloc();
            globals.insert_type(add, bin_ty.clone());
            let sub = alloc();
            globals.insert_type(sub, bin_ty.clone());
            let mul = alloc();
            globals.insert_type(mul, bin_ty.clone());
            let div = alloc();
            globals.insert_type(div, bin_ty.clone());
            let rem = alloc();
            globals.insert_type(rem, bin_ty.clone());
            let bitand = alloc();
            globals.insert_type(bitand, bin_ty.clone());
            let bitor = alloc();
            globals.insert_type(bitor, bin_ty.clone());
            let bitxor = alloc();
            globals.insert_type(bitxor, bin_ty.clone());
            let shl = alloc();
            globals.insert_type(shl, bin_ty.clone());
            let shr = alloc();
            globals.insert_type(shr, bin_ty);
            let cmp_ty = pi(
                ty_term.clone(),
                pi(ty_term.clone(), Term::Global(prim::BOOL)),
            );
            let lt = alloc();
            globals.insert_type(lt, cmp_ty.clone());
            let gt = alloc();
            globals.insert_type(gt, cmp_ty);
            let if_then_else = alloc();
            let println = alloc();
            globals.insert_type(
                if_then_else,
                pi(
                    Term::Global(prim::BOOL),
                    pi(
                        Term::Global(ty),
                        pi(Term::Global(ty), Term::Global(ty)),
                    ),
                ),
            );
            globals.insert_type(println, pi(Term::Global(ty), unit));
            BuiltinOps {
                add,
                sub,
                mul,
                div,
                rem,
                bitand,
                bitor,
                bitxor,
                shl,
                shr,
                lt,
                gt,
                if_then_else,
                println,
                str_eq: alloc(),
            }
        };

        let i8 = mk_numeric(prim::I8);
        let u8 = mk_numeric(prim::U8);
        let i16 = mk_numeric(prim::I16);
        let u16 = mk_numeric(prim::U16);
        let i32 = mk_numeric(prim::I32);
        let i64 = mk_numeric(prim::I64);
        let u64 = mk_numeric(prim::U64);
        let i128 = mk_numeric(prim::I128);
        let u128 = mk_numeric(prim::U128);
        let f16 = mk_numeric(prim::F16);
        let f32 = mk_numeric(prim::F32);
        let f64 = mk_numeric(prim::F64);
        let string = {
            let str_eq = alloc();
            let println = alloc();
            let if_then_else = alloc();
            let str_ty = Term::Global(prim::STRING);
            let bool_ty = Term::Global(prim::BOOL);
            let unit = Term::Global(prim::UNIT);
            globals.insert_type(
                str_eq,
                pi(
                    str_ty.clone(),
                    pi(str_ty.clone(), bool_ty.clone()),
                ),
            );
            globals.insert_type(println, pi(str_ty.clone(), unit.clone()));
            globals.insert_type(
                if_then_else,
                pi(
                    bool_ty,
                    pi(str_ty.clone(), pi(str_ty.clone(), str_ty)),
                ),
            );
            BuiltinOps {
                add: alloc(),
                sub: alloc(),
                mul: alloc(),
                div: alloc(),
                rem: alloc(),
                bitand: alloc(),
                bitor: alloc(),
                bitxor: alloc(),
                shl: alloc(),
                shr: alloc(),
                lt: alloc(),
                gt: alloc(),
                if_then_else,
                println,
                str_eq,
            }
        };
        let bool_ = {
            let if_then_else = alloc();
            let println = alloc();
            let bool_ty = Term::Global(prim::BOOL);
            let unit = Term::Global(prim::UNIT);
            globals.insert_type(
                if_then_else,
                pi(
                    bool_ty.clone(),
                    pi(bool_ty.clone(), pi(bool_ty.clone(), bool_ty.clone())),
                ),
            );
            globals.insert_type(println, pi(bool_ty, unit));
            BuiltinOps {
                add: alloc(),
                sub: alloc(),
                mul: alloc(),
                div: alloc(),
                rem: alloc(),
                bitand: alloc(),
                bitor: alloc(),
                bitxor: alloc(),
                shl: alloc(),
                shr: alloc(),
                lt: alloc(),
                gt: alloc(),
                if_then_else,
                println,
                str_eq: alloc(),
            }
        };

        let unit_ = {
            let if_then_else = alloc();
            let println = alloc();
            let unit_ty = Term::Global(prim::UNIT);
            globals.insert_type(
                if_then_else,
                pi(
                    Term::Global(prim::BOOL),
                    pi(unit_ty.clone(), pi(unit_ty.clone(), unit_ty.clone())),
                ),
            );
            globals.insert_type(println, pi(unit_ty, Term::Global(prim::UNIT)));
            BuiltinOps {
                add: alloc(),
                sub: alloc(),
                mul: alloc(),
                div: alloc(),
                rem: alloc(),
                bitand: alloc(),
                bitor: alloc(),
                bitxor: alloc(),
                shl: alloc(),
                shr: alloc(),
                lt: alloc(),
                gt: alloc(),
                if_then_else,
                println,
                str_eq: alloc(),
            }
        };

        Self {
            next_builtin: next,
            i8,
            u8,
            i16,
            u16,
            i32,
            i64,
            u64,
            i128,
            u128,
            f16,
            f32,
            f64,
            string,
            bool_,
            unit_,
        }
    }

    pub fn ops_for_prim(&self, ty: DefId) -> Option<&BuiltinOps> {
        match ty {
            prim::I8 => Some(&self.i8),
            prim::U8 => Some(&self.u8),
            prim::I16 => Some(&self.i16),
            prim::U16 => Some(&self.u16),
            prim::I32 => Some(&self.i32),
            prim::I64 => Some(&self.i64),
            prim::U64 => Some(&self.u64),
            prim::I128 => Some(&self.i128),
            prim::U128 => Some(&self.u128),
            prim::F16 => Some(&self.f16),
            prim::F32 => Some(&self.f32),
            prim::F64 => Some(&self.f64),
            prim::STRING => Some(&self.string),
            prim::BOOL => Some(&self.bool_),
            prim::UNIT => Some(&self.unit_),
            _ => None,
        }
    }
}

fn pi(domain: Term, codomain: Term) -> Term {
    Term::Pi {
        binder: crate::core::term::Binder::new("_", crate::core::term::Level(0), domain),
        body: Box::new(codomain),
    }
}
