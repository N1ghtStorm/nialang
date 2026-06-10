use crate::core::effect::Effect;
use crate::core::globals::prim;
use crate::core::inductive::{
    apply_family, ctor_arg_types, family_instance_parts, seed, subst_family_params,
};
use crate::core::term::{Binder, Explicitness, Level, Term};
use crate::elab::env::ElabEnv;
use crate::elab::symbols::BinOp;
use crate::elab::ty::elab_ty;
use crate::frontend::resolve::{DefId, TypeDefKind};
use crate::frontend::surface::{Expr, MatchPattern};
use crate::nia_std::{
    is_quantum_builtin_fn, ALLOC, DEALLOC, LEN, MATRIX_DROP, REALLOC, TO_ARRAY, TO_MATRIX, TO_VEC,
};

pub fn infer_expr(env: &mut ElabEnv, expr: &Expr) -> Result<(Term, Term), String> {
    elab_expr(env, expr, None)
}

pub fn elab_expr(
    env: &mut ElabEnv,
    expr: &Expr,
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    match expr {
        Expr::Int(n) => {
            let ty = expected
                .cloned()
                .or_else(|| Some(Term::Global(prim::I32)));
            let ty = ty.ok_or_else(|| "integer literal needs a type".to_string())?;
            let term = int_literal(*n, &ty)?;
            Ok((term, ty))
        }
        Expr::Bool(b) => Ok((Term::Bool(*b), Term::Global(prim::BOOL))),
        Expr::Float(f) => {
            let ty = expected
                .cloned()
                .or_else(|| Some(Term::Global(prim::F64)));
            let ty = ty.ok_or_else(|| "float literal needs a type".to_string())?;
            let term = float_literal(*f, &ty)?;
            Ok((term, ty))
        }
        Expr::String(s) => Ok((Term::LitStr(s.clone()), Term::Global(prim::STRING))),
        Expr::Ident(name) => {
            if let Some(result) = elab_core_inductive_ident(env, name, expected)? {
                return Ok(result);
            }
            if name == crate::nia_std::PI {
                let ty = Term::Global(prim::F64);
                return Ok((
                    Term::LitFloat {
                        value: std::f64::consts::PI,
                        ty: prim::F64,
                    },
                    ty,
                ));
            }
            let (level, ty) = env
                .lookup_local(name)
                .ok_or_else(|| format!("unknown variable `{name}`"))?;
            Ok((Term::Var(level), ty))
        }
        Expr::Neg(inner) => elab_unary_neg(env, inner),
        Expr::Not(inner) => elab_unary_not(env, inner),
        Expr::BitNot(inner) => elab_unary_bitnot(env, inner),
        Expr::Add(l, r) => elab_binop(env, l, r, BinOp::Add, expected),
        Expr::Sub(l, r) => elab_binop(env, l, r, BinOp::Sub, expected),
        Expr::Mul(l, r) => elab_binop(env, l, r, BinOp::Mul, expected),
        Expr::Div(l, r) => elab_binop(env, l, r, BinOp::Div, expected),
        Expr::Rem(l, r) => elab_binop(env, l, r, BinOp::Rem, expected),
        Expr::BitAnd(l, r) => elab_binop(env, l, r, BinOp::BitAnd, expected),
        Expr::BitOr(l, r) => elab_binop(env, l, r, BinOp::BitOr, expected),
        Expr::BitXor(l, r) => elab_binop(env, l, r, BinOp::BitXor, expected),
        Expr::Shl(l, r) => elab_binop(env, l, r, BinOp::Shl, expected),
        Expr::Shr(l, r) => elab_binop(env, l, r, BinOp::Shr, expected),
        Expr::Lt(l, r) => elab_cmp_order(env, l, r, CmpOrder::Lt),
        Expr::Gt(l, r) => elab_cmp_order(env, l, r, CmpOrder::Gt),
        Expr::Le(l, r) => elab_cmp_order(env, l, r, CmpOrder::Le),
        Expr::Ge(l, r) => elab_cmp_order(env, l, r, CmpOrder::Ge),
        Expr::Eq(l, r) => elab_cmp_eq(env, l, r),
        Expr::Ne(l, r) => elab_cmp_ne(env, l, r),
        Expr::AddrOf(inner) => elab_addr_of(env, inner),
        Expr::Deref(inner) => elab_deref(env, inner),
        Expr::Index(arr, idx) => elab_index(env, arr, idx),
        Expr::ArrayLit(elems) => elab_array_lit(env, elems, expected),
        Expr::AnonVectorLit(elems) => elab_anon_vector_lit(env, elems, expected),
        Expr::VecDot(l, r) => elab_vec_dot(env, l, r),
        Expr::Call { name, args } => elab_call(env, name, args, expected),
        Expr::StructLit { name, fields } => elab_struct_lit(env, name, fields),
        Expr::Field(receiver, field) => elab_field(env, receiver, field),
        Expr::EnumVariant { enum_name, variant } => {
            elab_enum_ctor(env, enum_name, variant, &[])
        }
        Expr::EnumTuple {
            enum_name,
            variant,
            args,
        } => {
            let elab_args = args
                .iter()
                .map(|a| infer_expr(env, a).map(|(t, _)| t))
                .collect::<Result<Vec<_>, _>>()?;
            elab_enum_ctor(env, enum_name, variant, &elab_args)
        }
        Expr::EnumStruct {
            enum_name,
            variant,
            fields,
        } => {
            let ctor = env
                .ctor_for_variant(enum_name, variant)
                .ok_or_else(|| format!("unknown variant `{enum_name}::{variant}`"))?;
            let enum_def = env
                .resolved
                .enum_by_id(ctor.enum_id)
                .ok_or_else(|| format!("missing enum `{enum_name}`"))?;
            let variant_def = enum_def
                .def
                .variants
                .iter()
                .find(|v| v.name == *variant)
                .ok_or_else(|| format!("missing variant `{enum_name}::{variant}`"))?;
            let crate::frontend::surface::EnumVariantFields::Struct(field_defs) =
                &variant_def.fields
            else {
                return Err(format!("`{enum_name}::{variant}` is not a struct variant"));
            };
            let mut args = Vec::new();
            for (field_name, field_ty) in field_defs {
                let init = fields
                    .iter()
                    .find(|(n, _)| n == field_name)
                    .map(|(_, e)| e)
                    .ok_or_else(|| {
                        format!("missing field `{field_name}` in `{enum_name}::{variant}`")
                    })?;
                let expected = elab_ty(env, field_ty)?;
                let (term, _) = elab_expr(env, init, Some(&expected))?;
                args.push(term);
            }
            elab_enum_ctor(env, enum_name, variant, &args)
        }
        Expr::Match { scrutinee, arms } => elab_match(env, scrutinee, arms, expected),
        Expr::MethodCall {
            receiver,
            name,
            args,
        } => elab_method_call(env, receiver, name, args, expected),
        Expr::Quant { body } => {
            env.require_effect(Effect::Quantum, "`quant { }` expression")?;
            let term = env.with_quant_scope(|env| {
                crate::elab::stmt::elab_block(env, body, None)
            })?;
            Ok((term, Term::Global(prim::UNIT)))
        }
        Expr::Gpu { body } => {
            env.require_effect(Effect::Gpu, "`gpu { }` expression")?;
            let term = env.with_gpu_scope(|env| {
                crate::elab::stmt::elab_block(env, body, None)
            })?;
            Ok((term, Term::Global(prim::UNIT)))
        }
        other => Err(format!("expression not supported in elaborator yet: {other:?}")),
    }
}

fn int_literal(n: i128, ty: &Term) -> Result<Term, String> {
    match ty {
        Term::Global(id) if *id == prim::I32 => Ok(Term::I32(n as i32)),
        Term::Global(id) if is_float_prim(*id) => Ok(Term::LitFloat {
            value: n as f64,
            ty: *id,
        }),
        Term::Global(id) => Ok(Term::LitInt { value: n, ty: *id }),
        Term::Var(_) | Term::Meta(_) => Ok(Term::I32(n as i32)),
        _ => Err(format!("expected a primitive type for integer literal, got `{ty:?}`")),
    }
}

fn float_literal(f: f64, ty: &Term) -> Result<Term, String> {
    let Term::Global(id) = ty else {
        return Err(format!("expected a float type for float literal, got `{ty:?}`"));
    };
    if !is_float_prim(*id) {
        return Err(format!("expected a float type for float literal, got `{ty:?}`"));
    }
    Ok(Term::LitFloat { value: f, ty: *id })
}

fn is_float_prim(id: DefId) -> bool {
    matches!(id, prim::F16 | prim::F32 | prim::F64)
}

fn prim_from_ty(ty: &Term) -> Result<DefId, String> {
    match ty {
        Term::Global(id) => Ok(*id),
        _ => Err(format!("expected primitive type, got `{ty:?}`")),
    }
}

fn is_numeric_prim(id: DefId) -> bool {
    matches!(
        id,
        prim::I8
            | prim::U8
            | prim::I16
            | prim::U16
            | prim::I32
            | prim::I64
            | prim::U64
            | prim::I128
            | prim::U128
            | prim::F16
            | prim::F32
            | prim::F64
    )
}

fn is_numeric_ty(ty: &Term) -> bool {
    matches!(ty, Term::Global(id) if is_numeric_prim(*id))
}

fn peel_nested_array(
    env: &ElabEnv,
    ty: &Term,
) -> Result<(u32, u32, Term, DefId, DefId), String> {
    let Term::Global(outer_gid) = ty else {
        return Err(format!("expected nested array, got `{ty:?}`"));
    };
    let outer = env
        .data
        .array_info(*outer_gid)
        .ok_or_else(|| "expected nested array".to_string())?;
    let Term::Global(row_gid) = &outer.elem else {
        return Err("expected array of arrays".into());
    };
    let row = env
        .data
        .array_info(*row_gid)
        .ok_or_else(|| "expected array rows".to_string())?;
    if outer.len == 0 || row.len == 0 {
        return Err("matrix source must be non-empty".into());
    }
    Ok((outer.len, row.len, row.elem.clone(), *outer_gid, *row_gid))
}

fn nested_array_from_expected(expected: &Term, env: &ElabEnv) -> Option<(u32, u32, DefId, DefId)> {
    peel_nested_array(env, expected)
        .ok()
        .map(|(rows, cols, _, outer_gid, row_gid)| (rows, cols, outer_gid, row_gid))
}

#[derive(Clone, Copy)]
enum CmpOrder {
    Lt,
    Gt,
    Le,
    Ge,
}

fn elab_cmp_operands(
    env: &mut ElabEnv,
    l: &Expr,
    r: &Expr,
) -> Result<(Term, Term, Term), String> {
    let (_, lty) = infer_expr(env, l)?;
    let (lt, _) = elab_expr(env, l, Some(&lty))?;
    let (rt, _) = elab_expr(env, r, Some(&lty))?;
    Ok((lt, rt, lty))
}

fn elab_cmp_order(
    env: &mut ElabEnv,
    l: &Expr,
    r: &Expr,
    order: CmpOrder,
) -> Result<(Term, Term), String> {
    let (lt, rt, ty) = elab_cmp_operands(env, l, r)?;
    let prim_id = prim_from_ty(&ty)?;
    if prim_id == prim::STRING {
        return Err("ordering comparison on string is not supported".into());
    }
    let ops = env
        .prelude
        .ops_for_prim(prim_id)
        .ok_or_else(|| format!("no builtins for type `{ty:?}`"))?;
    let term = match order {
        CmpOrder::Lt => app2(ops.lt, lt, rt),
        CmpOrder::Gt => app2(ops.gt, lt, rt),
        CmpOrder::Le => elab_bool_not(env, app2(ops.gt, lt, rt))?,
        CmpOrder::Ge => elab_bool_not(env, app2(ops.lt, lt, rt))?,
    };
    Ok((term, Term::Global(prim::BOOL)))
}

fn elab_cmp_eq(env: &mut ElabEnv, l: &Expr, r: &Expr) -> Result<(Term, Term), String> {
    let (lt, rt, ty) = elab_cmp_operands(env, l, r)?;
    let prim_id = prim_from_ty(&ty)?;
    if prim_id == prim::STRING {
        let ops = &env.prelude.string;
        return Ok((app2(ops.str_eq, lt, rt), Term::Global(prim::BOOL)));
    }
    let ops = env
        .prelude
        .ops_for_prim(prim_id)
        .ok_or_else(|| format!("no builtins for type `{ty:?}`"))?;
    let eq = elab_bool_and(env, elab_bool_not(env, app2(ops.lt, lt.clone(), rt.clone()))?, elab_bool_not(env, app2(ops.gt, lt, rt))?)?;
    Ok((eq, Term::Global(prim::BOOL)))
}

fn elab_cmp_ne(env: &mut ElabEnv, l: &Expr, r: &Expr) -> Result<(Term, Term), String> {
    let (eq, _) = elab_cmp_eq(env, l, r)?;
    Ok((elab_bool_not(env, eq)?, Term::Global(prim::BOOL)))
}

fn elab_bool_not(env: &ElabEnv, cond: Term) -> Result<Term, String> {
    Ok(Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::Global(env.prelude.bool_.if_then_else)),
            arg: Box::new(cond),
        }),
        arg: Box::new(Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(env.prelude.bool_.if_then_else)),
                arg: Box::new(Term::Bool(false)),
            }),
            arg: Box::new(Term::Bool(true)),
        }),
    })
}

fn elab_bool_and(env: &ElabEnv, a: Term, b: Term) -> Result<Term, String> {
    Ok(Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::Global(env.prelude.bool_.if_then_else)),
            arg: Box::new(a),
        }),
        arg: Box::new(Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(env.prelude.bool_.if_then_else)),
                arg: Box::new(b),
            }),
            arg: Box::new(Term::Bool(false)),
        }),
    })
}

fn elab_binop(
    env: &mut ElabEnv,
    l: &Expr,
    r: &Expr,
    op: BinOp,
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    if matches!(op, BinOp::Div | BinOp::Rem) && matches!(r, Expr::Int(0)) {
        return Err("division by zero".into());
    }
    let (lt, lty) = elab_expr(env, l, expected)?;
    let (rt, _) = elab_expr(env, r, Some(&lty))?;
    let prim_id = prim_from_ty(&lty)?;
    let ops = env
        .prelude
        .ops_for_prim(prim_id)
        .ok_or_else(|| format!("no builtins for type `{lty:?}`"))?;
    let builtin = match op {
        BinOp::Add => ops.add,
        BinOp::Sub => ops.sub,
        BinOp::Mul => ops.mul,
        BinOp::Div => ops.div,
        BinOp::Rem => ops.rem,
        BinOp::BitAnd => ops.bitand,
        BinOp::BitOr => ops.bitor,
        BinOp::BitXor => ops.bitxor,
        BinOp::Shl => ops.shl,
        BinOp::Shr => ops.shr,
    };
    Ok((app2(builtin, lt, rt), lty))
}

fn coerce_receiver_for_self(
    env: &mut ElabEnv,
    receiver: &Expr,
    self_ty: &Term,
) -> Result<Term, String> {
    let (recv, recv_ty) = infer_expr(env, receiver)?;
    if recv_ty == *self_ty {
        return Ok(recv);
    }
    if let Term::Global(ptr_gid) = self_ty {
        if let Some(info) = env.data.ptrs.get(ptr_gid) {
            if recv_ty == info.inner {
                let inner = info.inner.clone();
                let (value, _) = elab_expr(env, receiver, Some(&inner))?;
                return Ok(Term::AddrOf {
                    inner_ty: *ptr_gid,
                    value: Box::new(value),
                });
            }
        }
    }
    if let Term::Global(recv_ptr) = &recv_ty {
        if env.data.ptrs.get(recv_ptr).is_some_and(|info| self_ty == &info.inner) {
            let (ptr, _) = elab_expr(env, receiver, Some(&recv_ty))?;
            return Ok(Term::Deref {
                inner_ty: *recv_ptr,
                ptr: Box::new(ptr),
            });
        }
    }
    elab_expr(env, receiver, Some(self_ty)).map(|(t, _)| t)
}

fn elab_method_call(
    env: &mut ElabEnv,
    receiver: &Expr,
    method: &str,
    args: &[Expr],
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    if method == TO_MATRIX {
        if !args.is_empty() {
            return Err(format!(
                "method `{TO_MATRIX}`: expected 0 args, got {}",
                args.len()
            ));
        }
        let (recv_term, recv_ty) = match receiver {
            Expr::ArrayLit(elems) => {
                let exp = array_lit_expected(env, elems, None)?;
                elab_array_lit(env, elems, exp.as_ref())?
            }
            _ => {
                let (_, recv_ty) = infer_expr(env, receiver)?;
                let (recv_term, _) = elab_expr(env, receiver, Some(&recv_ty))?;
                (recv_term, recv_ty)
            }
        };
        let (rows, cols, cell_elem, outer_gid, row_gid) = peel_nested_array(env, &recv_ty)?;
        if !is_numeric_ty(&cell_elem) {
            return Err(format!(
                "method `{TO_MATRIX}` cells must be numeric, got `{cell_elem:?}`"
            ));
        }
        let matrix_ty = env.register_matrix_type(&cell_elem);
        return Ok((
            Term::MatrixNew {
                matrix_ty,
                rows,
                cols,
                row_array_ty: row_gid,
                outer_array_ty: outer_gid,
                src: Box::new(recv_term),
            },
            Term::Global(matrix_ty),
        ));
    }

    if method == TO_ARRAY {
        if let Some(exp) = expected {
            if let Some((rows, cols, outer_gid, row_gid)) = nested_array_from_expected(exp, env) {
                let (_, recv_ty) = infer_expr(env, receiver)?;
                if let Term::Global(matrix_gid) = recv_ty {
                    if env.data.matrix_info(matrix_gid).is_some() {
                        let (recv_term, _) = elab_expr(env, receiver, Some(&recv_ty))?;
                        return Ok((
                            Term::MatrixToArray {
                                matrix_ty: matrix_gid,
                                rows,
                                cols,
                                row_array_ty: row_gid,
                                outer_array_ty: outer_gid,
                                matrix: Box::new(recv_term),
                            },
                            exp.clone(),
                        ));
                    }
                }
            }
        }
    }

    if method == TO_VEC || method == TO_ARRAY {
        if !args.is_empty() {
            return Err(format!("method `{method}`: expected 0 args, got {}", args.len()));
        }
        let (recv_term, recv_ty) = match receiver {
            Expr::ArrayLit(elems) => {
                let exp = array_lit_expected(env, elems, expected)?;
                elab_array_lit(env, elems, exp.as_ref())?
            }
            Expr::AnonVectorLit(elems) => elab_anon_vector_lit(env, elems, expected)?,
            _ => {
                let (_, recv_ty) = infer_expr(env, receiver)?;
                let Term::Global(array_gid) = recv_ty else {
                    return Err(format!("unknown method `{method}` for type `{recv_ty:?}`"));
                };
                if env.data.array_info(array_gid).is_none() {
                    return Err(format!("unknown method `{method}` for type `{recv_ty:?}`"));
                }
                let (recv_term, _) = elab_expr(env, receiver, Some(&recv_ty))?;
                (recv_term, recv_ty)
            }
        };
        return Ok((recv_term, recv_ty));
    }

    let (_, recv_ty) = infer_expr(env, receiver)?;
    let struct_name = env
        .struct_name_for_ty(&recv_ty)
        .ok_or_else(|| format!("method call on non-struct type `{recv_ty:?}`"))?;
    let symbol = format!("{struct_name}__{method}");
    let fn_id = env
        .resolved
        .lookup_fn(&symbol)
        .ok_or_else(|| format!("unknown method `{method}` for `{struct_name}`"))?;
    let gid = env.fn_gid(fn_id);
    let fn_ty = env
        .globals
        .type_of(gid)
        .cloned()
        .ok_or_else(|| format!("missing type for method `{symbol}`"))?;
    let mut term = Term::Global(gid);
    let mut cur_ty = fn_ty;
    cur_ty = skip_implicit_pis(cur_ty);
    let (self_ty, codomain) = peel_pi(cur_ty)?;
    let recv_elab = coerce_receiver_for_self(env, receiver, &self_ty)?;
    term = Term::App {
        fun: Box::new(term),
        arg: Box::new(recv_elab),
    };
    cur_ty = codomain;
    for arg in args {
        cur_ty = skip_implicit_pis(cur_ty);
        let (domain, rest) = peel_pi(cur_ty)?;
        let (arg_term, _) = elab_expr(env, arg, Some(&domain))?;
        term = Term::App {
            fun: Box::new(term),
            arg: Box::new(arg_term),
        };
        cur_ty = rest;
    }
    let ret = expected.cloned().unwrap_or(cur_ty);
    Ok((term, ret))
}

fn elab_unary_neg(env: &mut ElabEnv, inner: &Expr) -> Result<(Term, Term), String> {
    elab_binop(env, inner, &Expr::Int(0), BinOp::Sub, None)
}

fn elab_unary_not(env: &mut ElabEnv, inner: &Expr) -> Result<(Term, Term), String> {
    let (term, ty) = infer_expr(env, inner)?;
    if ty != Term::Global(prim::BOOL) {
        return Err("`!` expects bool".into());
    }
    Ok((
        Term::App {
            fun: Box::new(Term::App {
                fun: Box::new(Term::Global(env.prelude.bool_.if_then_else)),
                arg: Box::new(term),
            }),
            arg: Box::new(Term::App {
                fun: Box::new(Term::App {
                    fun: Box::new(Term::Global(env.prelude.bool_.if_then_else)),
                    arg: Box::new(Term::Bool(false)),
                }),
                arg: Box::new(Term::Bool(true)),
            }),
        },
        Term::Global(prim::BOOL),
    ))
}

fn elab_unary_bitnot(env: &mut ElabEnv, inner: &Expr) -> Result<(Term, Term), String> {
    let (term, ty) = infer_expr(env, inner)?;
    let prim_id = match &ty {
        Term::Global(id) => *id,
        _ => return Err("bitwise not on non-primitive type".into()),
    };
    let ops = env
        .prelude
        .ops_for_prim(prim_id)
        .ok_or_else(|| format!("no builtins for type `{ty:?}`"))?;
    let mask = int_literal(-1, &ty)?;
    Ok((app2(ops.bitxor, mask, term), ty))
}

fn elab_addr_of(env: &mut ElabEnv, inner: &Expr) -> Result<(Term, Term), String> {
    let (value, val_ty) = infer_expr(env, inner)?;
    let ptr_gid = env.register_ptr_type(&val_ty);
    Ok((
        Term::AddrOf {
            inner_ty: ptr_gid,
            value: Box::new(value),
        },
        Term::Global(ptr_gid),
    ))
}

fn elab_deref(env: &mut ElabEnv, inner: &Expr) -> Result<(Term, Term), String> {
    let (ptr, ptr_ty) = infer_expr(env, inner)?;
    let Term::Global(ptr_gid) = ptr_ty else {
        return Err("deref expects pointer".into());
    };
    let info = env
        .data
        .ptr_info(ptr_gid)
        .ok_or_else(|| "unknown pointer type".to_string())?;
    Ok((
        Term::Deref {
            inner_ty: ptr_gid,
            ptr: Box::new(ptr),
        },
        info.inner.clone(),
    ))
}

fn elab_index(env: &mut ElabEnv, arr: &Expr, idx: &Expr) -> Result<(Term, Term), String> {
    let (arr_term, arr_ty) = infer_expr(env, arr)?;
    let (idx_term, _) = elab_expr(env, idx, Some(&Term::Global(prim::I32)))?;
    let Term::Global(array_gid) = arr_ty else {
        return Err("index expects array".into());
    };
    let info = env
        .data
        .array_info(array_gid)
        .ok_or_else(|| "unknown array type".to_string())?;
    Ok((
        Term::ArrayGet {
            elem_ty: array_gid,
            len: info.len,
            arr: Box::new(arr_term),
            index: Box::new(idx_term),
        },
        info.elem.clone(),
    ))
}

fn array_lit_expected(
    env: &mut ElabEnv,
    elems: &[Expr],
    expected: Option<&Term>,
) -> Result<Option<Term>, String> {
    if let Some(exp) = expected {
        return Ok(Some(exp.clone()));
    }
    if elems.is_empty() {
        return Ok(None);
    }
    if let Expr::ArrayLit(inner_elems) = &elems[0] {
        let inner_exp = array_lit_expected(env, inner_elems, None)?;
        let inner_ty = match inner_exp {
            Some(t) => t,
            None => {
                let (_, first_inner_ty) = infer_expr(env, &inner_elems[0])?;
                for e in inner_elems.iter().skip(1) {
                    elab_expr(env, e, Some(&first_inner_ty))?;
                }
                Term::Global(env.register_array_type(&first_inner_ty, inner_elems.len() as u32))
            }
        };
        for row in elems.iter().skip(1) {
            if let Expr::ArrayLit(_) = row {
                elab_expr(env, row, Some(&inner_ty))?;
            } else {
                return Err("nested array rows must be array literals".into());
            }
        }
        let outer_gid = env.register_array_type(&inner_ty, elems.len() as u32);
        return Ok(Some(Term::Global(outer_gid)));
    }
    let (_, first_ty) = infer_expr(env, &elems[0])?;
    for elem in elems.iter().skip(1) {
        elab_expr(env, elem, Some(&first_ty))?;
    }
    let gid = env.register_array_type(&first_ty, elems.len() as u32);
    Ok(Some(Term::Global(gid)))
}

fn elab_anon_vector_lit(
    env: &mut ElabEnv,
    elems: &[Expr],
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    if elems.is_empty() {
        return Err("anonymous vector literal must not be empty".into());
    }
    let array_ty = match expected {
        Some(Term::Global(gid)) if env.data.array_info(*gid).is_some() => Term::Global(*gid),
        _ => array_lit_expected(env, elems, expected)?
            .ok_or_else(|| "anonymous vector literal needs an expected array type".to_string())?,
    };
    elab_array_lit(env, elems, Some(&array_ty))
}

fn elab_vec_dot(env: &mut ElabEnv, l: &Expr, r: &Expr) -> Result<(Term, Term), String> {
    let (_, lty) = infer_expr(env, l)?;
    let (_, rty) = infer_expr(env, r)?;
    if lty != rty {
        return Err(format!(
            "`@` operands must have the same type, got `{lty:?}` and `{rty:?}`"
        ));
    }
    let Term::Global(array_gid) = lty else {
        return Err("`@` requires fixed-size array operands".into());
    };
    let (elem_ty, len) = {
        let info = env
            .data
            .array_info(array_gid)
            .ok_or_else(|| "unknown array type".to_string())?;
        (info.elem.clone(), info.len)
    };
    let (l_term, _) = elab_expr(env, l, Some(&lty))?;
    let (r_term, _) = elab_expr(env, r, Some(&rty))?;
    let prim_id = prim_from_ty(&elem_ty)?;
    let ops = env
        .prelude
        .ops_for_prim(prim_id)
        .ok_or_else(|| format!("no builtins for `{elem_ty:?}`"))?;

    let acc_level = env.current_level();
    let i_level = Level(acc_level.0 + 1);
    let zero = int_literal(0, &elem_ty)?;
    let i32_ty = Term::Global(prim::I32);
    let unit = Term::Global(prim::UNIT);
    let end = Term::I32(len as i32);

    let idx = Term::Var(i_level);
    let li = Term::ArrayGet {
        elem_ty: array_gid,
        len,
        arr: Box::new(l_term),
        index: Box::new(idx.clone()),
    };
    let ri = Term::ArrayGet {
        elem_ty: array_gid,
        len,
        arr: Box::new(r_term),
        index: Box::new(idx),
    };
    let prod = app2(ops.mul, li, ri);
    let sum = app2(ops.add, Term::Var(acc_level), prod);
    let inc = app2(
        env.prelude.i32.add,
        Term::Var(i_level),
        Term::I32(1),
    );
    let while_body = Term::Let {
        binder: Binder::new("_", Level(i_level.0 + 1), unit.clone()),
        value: Box::new(Term::Assign {
            target: Box::new(Term::Var(acc_level)),
            value: Box::new(sum),
        }),
        body: Box::new(Term::Assign {
            target: Box::new(Term::Var(i_level)),
            value: Box::new(inc),
        }),
    };
    let cond = Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::Global(env.prelude.i32.lt)),
            arg: Box::new(Term::Var(i_level)),
        }),
        arg: Box::new(end),
    };

    let term = Term::Let {
        binder: Binder::new("_dot_acc", acc_level, elem_ty.clone()),
        value: Box::new(zero),
        body: Box::new(Term::Let {
            binder: Binder::new("i", i_level, i32_ty),
            value: Box::new(Term::I32(0)),
            body: Box::new(Term::Let {
                binder: Binder::new("_", Level(i_level.0 + 1), unit),
                value: Box::new(Term::While {
                    cond: Box::new(cond),
                    body: Box::new(while_body),
                }),
                body: Box::new(Term::Var(acc_level)),
            }),
        }),
    };
    Ok((term, elem_ty))
}

fn elab_array_lit(
    env: &mut ElabEnv,
    elems: &[Expr],
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    let array_ty = match expected {
        Some(t) if matches!(t, Term::Global(_)) => t.clone(),
        _ => array_lit_expected(env, elems, expected)?
            .ok_or_else(|| "array literal needs an expected array type".to_string())?,
    };
    let Term::Global(array_gid) = array_ty else {
        return Err("array literal expected type must be array global".into());
    };
    let info = env
        .data
        .array_info(array_gid)
        .ok_or_else(|| "unknown array type".to_string())?;
    if elems.len() as u32 != info.len {
        return Err("array literal length mismatch".into());
    }
    let elem_ty = info.elem.clone();
    let mut elab_elems = Vec::new();
    for elem in elems {
        let (t, _) = elab_expr(env, elem, Some(&elem_ty))?;
        elab_elems.push(t);
    }
    Ok((
        Term::ArrayLit {
            elem_ty: array_gid,
            elems: elab_elems,
        },
        Term::Global(array_gid),
    ))
}

fn elab_core_inductive_ident(
    env: &ElabEnv,
    name: &str,
    expected: Option<&Term>,
) -> Result<Option<(Term, Term)>, String> {
    let nat = env.core_inductive.nat;
    let vec = env.core_inductive.vec;
    let nat_ty = Term::Global(nat);
    if name == "Zero" {
        return Ok(Some((seed::zero_ctor(nat), nat_ty)));
    }
    if name == "Nil" {
        let exp = expected
            .ok_or_else(|| "`Nil` needs expected Vec type from context".to_string())?;
        let (elem, idx) = peel_vec_instance(env, exp)?;
        let ty = apply_family(vec, &[elem], &[idx]);
        return Ok(Some((seed::nil_ctor(vec), ty)));
    }
    Ok(None)
}

fn elab_core_inductive_call(
    env: &mut ElabEnv,
    name: &str,
    args: &[Expr],
    expected: Option<&Term>,
) -> Result<Option<(Term, Term)>, String> {
    let nat = env.core_inductive.nat;
    let vec = env.core_inductive.vec;
    let nat_ty = Term::Global(nat);
    match name {
        "Zero" if args.is_empty() => {
            return Ok(Some((seed::zero_ctor(nat), nat_ty)));
        }
        "Succ" if args.len() == 1 => {
            let (arg, _) = elab_expr(env, &args[0], Some(&nat_ty))?;
            return Ok(Some((seed::succ_ctor(nat, arg), nat_ty)));
        }
        "Nil" if args.is_empty() => {
            let nil = seed::nil_ctor(vec);
            let exp = expected
                .ok_or_else(|| "`Nil` needs expected Vec type from context".to_string())?;
            let (elem, idx) = peel_vec_instance(env, exp)?;
            let ty = apply_family(vec, &[elem], &[idx]);
            return Ok(Some((nil, ty)));
        }
        "Cons" if args.len() == 2 => {
            return Ok(Some(elab_vec_cons(env, &args[0], &args[1])?));
        }
        "add" if args.len() == 2 => {
            return Ok(Some(elab_core_fn_call(
                env,
                env.core_inductive.nat_add,
                "add",
                args,
            )?));
        }
        "append" if args.len() == 2 => {
            return Ok(Some(elab_core_fn_call(
                env,
                env.core_inductive.vec_append,
                "append",
                args,
            )?));
        }
        _ => {}
    }
    Ok(None)
}

fn elab_core_fn_call(
    env: &mut ElabEnv,
    gid: crate::frontend::resolve::DefId,
    name: &str,
    args: &[Expr],
) -> Result<(Term, Term), String> {
    let fn_ty = env
        .globals
        .type_of(gid)
        .cloned()
        .ok_or_else(|| format!("missing type for core `{name}`"))?;
    let mut term = Term::Global(gid);
    let mut cur_ty = fn_ty;
    for arg in args {
        cur_ty = skip_implicit_pis(cur_ty);
        let (domain, codomain) = peel_pi(cur_ty)?;
        let (arg_term, _) = elab_expr(env, arg, Some(&domain))?;
        term = Term::App {
            fun: Box::new(term),
            arg: Box::new(arg_term),
        };
        cur_ty = codomain;
    }
    Ok((term, cur_ty))
}

fn elab_vec_cons(
    env: &mut ElabEnv,
    head: &Expr,
    tail: &Expr,
) -> Result<(Term, Term), String> {
    let vec = env.core_inductive.vec;
    let nat = env.core_inductive.nat;
    let (tail_term, tail_ty) = if is_nil_expr(tail) {
        let (_, elem) = infer_expr(env, head)?;
        let expected_nil = apply_family(vec, &[elem], &[seed::zero_ctor(nat)]);
        elab_expr(env, tail, Some(&expected_nil))?
    } else {
        infer_expr(env, tail)?
    };
    let (elem, n_pred) = peel_vec_instance(env, &tail_ty)?;
    let (head_term, _) = elab_expr(env, head, Some(&elem))?;
    let (tail_term, _) = elab_expr(env, tail, Some(&tail_ty))?;
    let result = apply_family(vec, &[elem], &[seed::succ_ctor(nat, n_pred.clone())]);
    Ok((
        seed::cons_ctor(vec, nat, n_pred, head_term, tail_term),
        result,
    ))
}

fn is_nil_expr(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(name) if name == "Nil")
        || matches!(expr, Expr::Call { name, args } if name == "Nil" && args.is_empty())
}

fn peel_vec_instance(env: &ElabEnv, ty: &Term) -> Result<(Term, Term), String> {
    let vec = env.core_inductive.vec;
    let (params, indices) = family_instance_parts(ty, vec, 1, 1)
        .ok_or_else(|| format!("expected Vec type, got `{ty:?}`"))?;
    Ok((params[0].clone(), indices[0].clone()))
}

fn elab_call(
    env: &mut ElabEnv,
    name: &str,
    args: &[Expr],
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    if let Some(kind) = env.resolved.lookup_type(name) {
        if let TypeDefKind::Struct(id) = kind {
            return elab_tuple_struct_call(env, id, args);
        }
    }

    if let Some(result) = elab_core_inductive_call(env, name, args, expected)? {
        return Ok(result);
    }

    if is_quantum_builtin_fn(name) {
        env.require_quant_scope(&format!("quantum operation `{name}`"))?;
        env.require_effect(Effect::Quantum, &format!("quantum operation `{name}`"))?;
        return crate::elab::quantum::elab_quantum_call(env, name, args);
    }

    if name == "println" {
        env.require_effect(Effect::IO, "`println`")?;
        if args.len() != 1 {
            return Err("`println` expects one argument".into());
        }
        let (arg, arg_ty) = infer_expr(env, &args[0])?;
        if let Term::Global(id) = &arg_ty {
            if let Some(println) = env.enum_println.get(id) {
                return Ok((app1(*println, arg), Term::Global(prim::UNIT)));
            }
            if let Some(println) = env.array_println.get(id) {
                return Ok((app1(*println, arg), Term::Global(prim::UNIT)));
            }
            if let Some(println) = env.struct_println.get(id) {
                return Ok((app1(*println, arg), Term::Global(prim::UNIT)));
            }
            if let Some(println) = env.ptr_println.get(id) {
                return Ok((app1(*println, arg), Term::Global(prim::UNIT)));
            }
            if let Some(println) = env.matrix_println.get(id) {
                return Ok((app1(*println, arg), Term::Global(prim::UNIT)));
            }
            if let Some(ops) = env.prelude.ops_for_prim(*id) {
                return Ok((app1(ops.println, arg), Term::Global(prim::UNIT)));
            }
        }
        return Err(format!("`println` not supported for `{arg_ty:?}`"));
    }

    if name == LEN {
        if args.len() != 1 {
            return Err("`len` expects one argument".into());
        }
        let (arr, arr_ty) = infer_expr(env, &args[0])?;
        let Term::Global(array_gid) = arr_ty else {
            return Err("`len` expects array".into());
        };
        let info = env
            .data
            .array_info(array_gid)
            .ok_or_else(|| "unknown array type".to_string())?;
        return Ok((
            Term::Len {
                elem_ty: array_gid,
                len: info.len,
                arr: Box::new(arr),
            },
            Term::Global(prim::I32),
        ));
    }
    if name == ALLOC {
        if args.len() != 1 {
            return Err("`alloc` expects one argument".into());
        }
        let (val, val_ty) = infer_expr(env, &args[0])?;
        let ptr_gid = env.register_ptr_type(&val_ty);
        return Ok((
            Term::HeapAlloc {
                ptr_ty: ptr_gid,
                value: Box::new(val),
            },
            Term::Global(ptr_gid),
        ));
    }
    if name == DEALLOC {
        if args.len() != 1 {
            return Err("`dealloc` expects one argument".into());
        }
        let (ptr, ptr_ty) = infer_expr(env, &args[0])?;
        let Term::Global(ptr_gid) = ptr_ty else {
            return Err("`dealloc` expects pointer".into());
        };
        return Ok((
            Term::HeapDealloc {
                ptr_ty: ptr_gid,
                ptr: Box::new(ptr),
            },
            Term::Global(prim::UNIT),
        ));
    }
    if name == REALLOC {
        if args.len() != 2 {
            return Err("`realloc` expects two arguments".into());
        }
        let (ptr, ptr_ty) = infer_expr(env, &args[0])?;
        let Term::Global(ptr_gid) = ptr_ty else {
            return Err("`realloc` first argument must be pointer".into());
        };
        let inner = env
            .data
            .ptr_info(ptr_gid)
            .ok_or_else(|| "unknown pointer type".to_string())?
            .inner
            .clone();
        let (val, _) = elab_expr(env, &args[1], Some(&inner))?;
        return Ok((
            Term::HeapRealloc {
                ptr_ty: ptr_gid,
                ptr: Box::new(ptr),
                value: Box::new(val),
            },
            Term::Global(ptr_gid),
        ));
    }
    if name == MATRIX_DROP {
        if args.len() != 1 {
            return Err(format!("`{MATRIX_DROP}` expects exactly 1 argument"));
        }
        let (_, m_ty) = infer_expr(env, &args[0])?;
        let Term::Global(matrix_gid) = m_ty else {
            return Err(format!("`{MATRIX_DROP}` expects Matrix, got `{m_ty:?}`"));
        };
        if env.data.matrix_info(matrix_gid).is_none() {
            return Err(format!("`{MATRIX_DROP}` expects Matrix, got `{m_ty:?}`"));
        }
        let (m, _) = elab_expr(env, &args[0], Some(&m_ty))?;
        return Ok((
            Term::MatrixDrop {
                matrix_ty: matrix_gid,
                matrix: Box::new(m),
            },
            Term::Global(prim::UNIT),
        ));
    }

    let fn_id = env
        .resolved
        .lookup_fn(name)
        .ok_or_else(|| format!("unknown function `{name}`"))?;
    let resolved_fn = env
        .resolved
        .fns
        .iter()
        .find(|f| f.id == fn_id)
        .ok_or_else(|| format!("missing function metadata for `{name}`"))?;
    if resolved_fn.def.is_quantum {
        env.require_quant_scope(&format!("call to quantum function `{name}`"))?;
    }
    let gid = env.fn_gid(fn_id);
    let callee_effect = env.globals.effect_of(gid).unwrap_or(Effect::Tot);
    env.check_call_effect(callee_effect, name)?;
    let fn_ty = env
        .globals
        .type_of(gid)
        .cloned()
        .ok_or_else(|| format!("missing type for function `{name}`"))?;
    let mut term = Term::Global(gid);
    let mut cur_ty = fn_ty;
    for arg in args {
        cur_ty = skip_implicit_pis(cur_ty);
        let (domain, codomain) = peel_pi(cur_ty)?;
        let (arg_term, _) = elab_expr(env, arg, Some(&domain))?;
        term = Term::App {
            fun: Box::new(term),
            arg: Box::new(arg_term),
        };
        cur_ty = codomain;
    }
    let ret = expected
        .cloned()
        .unwrap_or(cur_ty)
        .peel_computation_result();
    Ok((term, ret))
}

fn skip_implicit_pis(mut ty: Term) -> Term {
    loop {
        match ty {
            Term::Pi { binder, body }
                if binder.explicitness == Explicitness::Implicit =>
            {
                ty = *body;
            }
            other => return other,
        }
    }
}

fn peel_pi(ty: Term) -> Result<(Term, Term), String> {
    match ty {
        Term::Pi { binder, body } => Ok((*binder.ty, *body)),
        _ => Err(format!("expected function type, got `{ty:?}`")),
    }
}

fn elab_tuple_struct_call(
    env: &mut ElabEnv,
    struct_id: crate::frontend::resolve::DefId,
    args: &[Expr],
) -> Result<(Term, Term), String> {
    let s = env
        .resolved
        .struct_by_id(struct_id)
        .ok_or_else(|| "missing struct metadata".to_string())?;
    let gid = env.type_gid(TypeDefKind::Struct(struct_id));
    if args.len() != s.def.fields.len() {
        return Err(format!(
            "struct `{}` expects {} arguments, got {}",
            s.name,
            s.def.fields.len(),
            args.len()
        ));
    }
    let mut ctor_args = Vec::new();
    for (arg, (_, field_ty)) in args.iter().zip(s.def.fields.iter()) {
        let expected = elab_ty(env, field_ty)?;
        let (term, _) = elab_expr(env, arg, Some(&expected))?;
        ctor_args.push(term);
    }
    Ok((
        Term::DataCtor {
            type_def: gid,
            variant: 0,
            args: ctor_args,
        },
        Term::Global(gid),
    ))
}

fn elab_struct_lit(
    env: &mut ElabEnv,
    name: &str,
    fields: &[(String, Expr)],
) -> Result<(Term, Term), String> {
    let kind = env
        .resolved
        .lookup_type(name)
        .ok_or_else(|| format!("unknown struct `{name}`"))?;
    let TypeDefKind::Struct(id) = kind else {
        return Err(format!("`{name}` is not a struct"));
    };
    let s = env
        .resolved
        .struct_by_id(id)
        .ok_or_else(|| format!("missing struct `{name}`"))?;
    let gid = env.type_gid(kind);
    let mut args = Vec::new();
    for (field_name, field_ty) in &s.def.fields {
        let init = fields
            .iter()
            .find(|(n, _)| n == field_name)
            .map(|(_, e)| e)
            .ok_or_else(|| format!("missing field `{field_name}` in struct `{name}`"))?;
        let expected = elab_ty(env, field_ty)?;
        let (term, _) = elab_expr(env, init, Some(&expected))?;
        args.push(term);
    }
    Ok((
        Term::DataCtor {
            type_def: gid,
            variant: 0,
            args,
        },
        Term::Global(gid),
    ))
}

fn elab_field(
    env: &mut ElabEnv,
    receiver: &Expr,
    field: &str,
) -> Result<(Term, Term), String> {
    let (mut value, val_ty) = infer_expr(env, receiver)?;
    let type_def = match val_ty {
        Term::Global(id) if env.data.struct_fields(id).is_some() => id,
        Term::Global(ptr_gid) => {
            let info = env
                .data
                .ptrs
                .get(&ptr_gid)
                .ok_or_else(|| "field access on non-struct pointer".to_string())?;
            let Term::Global(struct_gid) = &info.inner else {
                return Err("field access through non-struct pointer".into());
            };
            value = Term::Deref {
                inner_ty: ptr_gid,
                ptr: Box::new(value),
            };
            *struct_gid
        }
        _ => return Err("field access on non-data value".into()),
    };
    let fields = env
        .data
        .struct_fields(type_def)
        .ok_or_else(|| "field access requires struct value".to_string())?;
    let index = if let Ok(idx) = field.parse::<usize>() {
        idx
    } else {
        let resolved_id = env
            .resolved
            .structs
            .iter()
            .find(|s| env.type_gid(TypeDefKind::Struct(s.id)) == type_def)
            .map(|s| s.id)
            .ok_or_else(|| "unknown struct for field access".to_string())?;
        let s = env.resolved.struct_by_id(resolved_id).unwrap();
        s.def
            .fields
            .iter()
            .position(|(n, _)| n == field)
            .ok_or_else(|| format!("struct has no field `{field}`"))?
    };
    let field_ty = fields
        .get(index)
        .cloned()
        .ok_or_else(|| format!("invalid field index `{field}`"))?;
    Ok((
        Term::DataProj {
            value: Box::new(value),
            type_def,
            field: index as u32,
        },
        field_ty,
    ))
}

fn elab_enum_ctor(
    env: &mut ElabEnv,
    enum_name: &str,
    variant: &str,
    args: &[Term],
) -> Result<(Term, Term), String> {
    let ctor = env
        .ctor_for_variant(enum_name, variant)
        .ok_or_else(|| format!("unknown variant `{enum_name}::{variant}`"))?;
    let gid = env.type_gid(TypeDefKind::Enum(ctor.enum_id));
    Ok((
        Term::DataCtor {
            type_def: gid,
            variant: ctor.variant_index as u32,
            args: args.to_vec(),
        },
        Term::Global(gid),
    ))
}

fn elab_match(
    env: &mut ElabEnv,
    scrutinee: &Expr,
    arms: &[(MatchPattern, Expr)],
    expected: Option<&Term>,
) -> Result<(Term, Term), String> {
    let (scrut_term, scrut_ty) = infer_expr(env, scrutinee)?;
    let family_gid = if let Some(g) = match_scrutinee_family(env, &scrut_ty) {
        g
    } else if let Term::Global(gid) = &scrut_ty {
        if env.data.enum_variants(*gid).is_some() {
            *gid
        } else {
            return Err("match scrutinee must be an enum or inductive value".into());
        }
    } else {
        return Err("match scrutinee must be an enum or inductive value".into());
    };
    let mut match_arms = Vec::new();
    let mut result_ty: Option<Term> = None;
    for (pat, body) in arms {
        let (variant_index, names, types) = if env.data.inductive(family_gid).is_some() {
            core_inductive_pattern_info(env, family_gid, pat, &scrut_ty)?
        } else {
            pattern_info(env, pat)?
        };
        let (body_term, body_ty) = env.with_locals_isolated(&names, &types, |env| {
            elab_expr(env, body, expected)
        })?;
        if let Some(prev) = &result_ty {
            if prev != &body_ty {
                return Err("match arms have different types".into());
            }
        } else {
            result_ty = Some(body_ty);
        }
        match_arms.push(crate::core::term::MatchArm {
            variant_index,
            body: body_term,
        });
    }
    let result_ty = result_ty.ok_or_else(|| "empty match".to_string())?;
    Ok((
        Term::DataMatch {
            scrutinee: Box::new(scrut_term),
            enum_def: family_gid,
            arms: match_arms,
        },
        result_ty,
    ))
}

fn match_scrutinee_family(env: &ElabEnv, scrut_ty: &Term) -> Option<DefId> {
    let vec = env.core_inductive.vec;
    let nat = env.core_inductive.nat;
    if family_instance_parts(scrut_ty, vec, 1, 1).is_some() {
        return Some(vec);
    }
    if scrut_ty == &Term::Global(nat) {
        return Some(nat);
    }
    if let Term::Global(id) = scrut_ty {
        if env.data.inductive(*id).is_some() {
            return Some(*id);
        }
    }
    None
}

fn core_inductive_family_name(env: &ElabEnv, family: DefId) -> Option<&'static str> {
    if family == env.core_inductive.nat {
        Some("Nat")
    } else if family == env.core_inductive.vec {
        Some("Vec")
    } else {
        None
    }
}

fn core_inductive_pattern_info(
    env: &ElabEnv,
    family: DefId,
    pat: &MatchPattern,
    scrut_ty: &Term,
) -> Result<(u32, Vec<String>, Vec<Term>), String> {
    let (enum_name, variant, mut bindings) = match pat {
        MatchPattern::Unit { enum_name, variant } => (enum_name, variant, vec![]),
        MatchPattern::Tuple {
            enum_name,
            variant,
            bindings,
        } => (enum_name, variant, bindings.clone()),
        MatchPattern::Struct {
            enum_name,
            variant,
            bindings,
        } => (enum_name, variant, bindings.clone()),
    };
    let expected_name = core_inductive_family_name(env, family).ok_or_else(|| {
        format!("unknown core inductive family `{family:?}` for match pattern")
    })?;
    if enum_name != expected_name {
        return Err(format!(
            "pattern `{enum_name}::{variant}` does not match scrutinee family `{expected_name}`"
        ));
    }
    let info = env
        .data
        .inductive(family)
        .ok_or_else(|| "missing inductive metadata".to_string())?;
    let param_count = info.params.len();
    let index_count = info.indices.len();
    let (params, _) = if param_count + index_count == 0 {
        (vec![], vec![])
    } else {
        family_instance_parts(scrut_ty, family, param_count, index_count).ok_or_else(|| {
            "match scrutinee is not a concrete family instance".to_string()
        })?
    };
    let variant_index = info
        .constructors
        .iter()
        .position(|c| c.name == *variant)
        .ok_or_else(|| format!("unknown variant `{enum_name}::{variant}`"))? as u32;
    let ctor = &info.constructors[variant_index as usize];
    let mut field_tys = ctor_arg_types(&ctor.ty).0;
    for ty in &mut field_tys {
        *ty = subst_family_params(info, &params, ty);
    }
    if family == env.core_inductive.vec && variant == "Cons" && bindings.len() == 2 {
        bindings.insert(0, "n".into());
    }
    if bindings.len() != field_tys.len() {
        return Err(format!(
            "pattern binding count mismatch for `{enum_name}::{variant}`"
        ));
    }
    Ok((variant_index, bindings, field_tys))
}

fn pattern_info(
    env: &ElabEnv,
    pat: &MatchPattern,
) -> Result<(u32, Vec<String>, Vec<Term>), String> {
    let (enum_name, variant, bindings) = match pat {
        MatchPattern::Unit { enum_name, variant } => (enum_name, variant, vec![]),
        MatchPattern::Tuple {
            enum_name,
            variant,
            bindings,
        } => (enum_name, variant, bindings.clone()),
        MatchPattern::Struct {
            enum_name,
            variant,
            bindings,
        } => (enum_name, variant, bindings.clone()),
    };
    let ctor = env
        .ctor_for_variant(enum_name, variant)
        .ok_or_else(|| format!("unknown variant `{enum_name}::{variant}`"))?;
    let gid = env.type_gid(TypeDefKind::Enum(ctor.enum_id));
    let variant_fields = env
        .data
        .variant_fields(gid, ctor.variant_index as u32)
        .ok_or_else(|| format!("missing metadata for `{enum_name}::{variant}`"))?;
    let types = match variant_fields {
        crate::core::data::VariantFields::Unit => vec![],
        crate::core::data::VariantFields::Tuple(ts) | crate::core::data::VariantFields::Struct(ts) => {
            if bindings.len() != ts.len() {
                return Err(format!(
                    "pattern binding count mismatch for `{enum_name}::{variant}`"
                ));
            }
            ts.to_vec()
        }
    };
    Ok((ctor.variant_index as u32, bindings, types))
}

fn app1(fun: crate::frontend::resolve::DefId, arg: Term) -> Term {
    Term::App {
        fun: Box::new(Term::Global(fun)),
        arg: Box::new(arg),
    }
}

fn app2(fun: crate::frontend::resolve::DefId, l: Term, r: Term) -> Term {
    Term::App {
        fun: Box::new(Term::App {
            fun: Box::new(Term::Global(fun)),
            arg: Box::new(l),
        }),
        arg: Box::new(r),
    }
}
