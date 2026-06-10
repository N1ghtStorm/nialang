//! Erasure: dependently-typed Core → runtime terms (phase 8.1).

use crate::core::data::{DataEnv, VariantFields};
use crate::core::quant::QuantKind;
use crate::core::term::{Explicitness, Term};
use crate::elab::{BinOp, CmpOp, CodegenSymbols, RuntimeBuiltin, RuntimeTy};
use crate::elab::{ElaboratedFn, ElaboratedModule};
use crate::frontend::resolve::DefId;

/// Runtime expression after erasing types and proofs.
#[derive(Debug, Clone, PartialEq)]
pub enum ErasedExpr {
    LitI32(i32),
    LitBool(bool),
    LitInt {
        value: i128,
        ty: RuntimeTy,
    },
    LitFloat {
        value: f64,
        ty: RuntimeTy,
    },
    LitStr(u32),
    StrEq(Box<ErasedExpr>, Box<ErasedExpr>),
    Var(u32),
    Let {
        value: Box<ErasedExpr>,
        body: Box<ErasedExpr>,
    },
    CallFn(String, Vec<ErasedExpr>),
    Quant {
        kind: QuantKind,
        args: Vec<ErasedExpr>,
    },
    BinOp(BinOp, RuntimeTy, Box<ErasedExpr>, Box<ErasedExpr>),
    Cmp(CmpOp, RuntimeTy, Box<ErasedExpr>, Box<ErasedExpr>),
    If(RuntimeTy, Box<ErasedExpr>, Box<ErasedExpr>, Box<ErasedExpr>),
    Println(RuntimeTy, Box<ErasedExpr>),
    StructAgg {
        name: String,
        fields: Vec<ErasedExpr>,
    },
    FieldGet {
        struct_name: String,
        value: Box<ErasedExpr>,
        index: u32,
    },
    EnumAgg {
        name: String,
        variant: u32,
        args: Vec<ErasedExpr>,
    },
    Match {
        enum_name: String,
        scrutinee: Box<ErasedExpr>,
        arms: Vec<ErasedArm>,
    },
    ArrayLit(RuntimeTy, Vec<ErasedExpr>),
    ArrayGet(RuntimeTy, Box<ErasedExpr>, Box<ErasedExpr>),
    ArraySet(RuntimeTy, Box<ErasedExpr>, Box<ErasedExpr>, Box<ErasedExpr>),
    AddrOf(RuntimeTy, Box<ErasedExpr>),
    Deref(RuntimeTy, Box<ErasedExpr>),
    Len(RuntimeTy, Box<ErasedExpr>),
    HeapAlloc(RuntimeTy, Box<ErasedExpr>),
    HeapDealloc(RuntimeTy, Box<ErasedExpr>),
    HeapRealloc(RuntimeTy, Box<ErasedExpr>, Box<ErasedExpr>),
    MatrixNew {
        elem: RuntimeTy,
        rows: u32,
        cols: u32,
        outer_rt: RuntimeTy,
        src: Box<ErasedExpr>,
    },
    MatrixToArray {
        elem: RuntimeTy,
        rows: u32,
        cols: u32,
        outer_rt: RuntimeTy,
        matrix: Box<ErasedExpr>,
    },
    MatrixDrop(RuntimeTy, Box<ErasedExpr>),
    While(Box<ErasedExpr>, Box<ErasedExpr>),
    Loop(Box<ErasedExpr>),
    For {
        var: String,
        start: Box<ErasedExpr>,
        end: Box<ErasedExpr>,
        body: Box<ErasedExpr>,
    },
    Break,
    Assign(Box<ErasedExpr>, Box<ErasedExpr>),
    Unit,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ErasedArm {
    pub variant: u32,
    pub bindings: u32,
    pub body: ErasedExpr,
}

#[derive(Debug, Clone)]
pub struct ErasedFn {
    pub name: String,
    pub params: Vec<(String, RuntimeTy)>,
    pub ret: RuntimeTy,
    pub body: ErasedExpr,
}

#[derive(Debug, Clone)]
pub struct ErasedModule {
    pub symbols: CodegenSymbols,
    pub fns: Vec<ErasedFn>,
    pub strings: Vec<String>,
}

pub fn erase_module(module: &ElaboratedModule) -> Result<ErasedModule, String> {
    let mut strings = Vec::new();
    let mut fns = Vec::new();
    for f in &module.fns {
        fns.push(erase_fn(f, &module.symbols, &module.data, &mut strings)?);
    }
    Ok(ErasedModule {
        symbols: module.symbols.clone(),
        fns,
        strings,
    })
}

fn intern_string(pool: &mut Vec<String>, s: &str) -> u32 {
    if let Some(i) = pool.iter().position(|x| x == s) {
        i as u32
    } else {
        pool.push(s.to_string());
        (pool.len() - 1) as u32
    }
}

fn erase_fn(
    f: &ElaboratedFn,
    symbols: &CodegenSymbols,
    data: &DataEnv,
    strings: &mut Vec<String>,
) -> Result<ErasedFn, String> {
    let (params, body) = peel_lams(&f.body, &f.sig, symbols)?;
    Ok(ErasedFn {
        name: f.name.clone(),
        params,
        ret: runtime_ty_from_term(peel_pi_return(&f.sig), symbols)?,
        body: erase_expr(&body, symbols, data, strings)?,
    })
}

fn peel_lams(
    term: &Term,
    sig: &Term,
    symbols: &CodegenSymbols,
) -> Result<(Vec<(String, RuntimeTy)>, Term), String> {
    let mut params = Vec::new();
    let mut cur_sig = sig.clone();
    let mut cur = term.clone();
    while let Term::Lam { binder, body } = cur {
        let (domain, rest) = peel_pi(cur_sig)?;
        params.push((
            binder.name_hint.clone(),
            runtime_ty_from_term(domain, symbols)?,
        ));
        cur_sig = rest;
        cur = *body;
    }
    Ok((params, cur))
}

fn peel_pi_return(sig: &Term) -> Term {
    let mut cur = sig.clone();
    while let Term::Pi { body, .. } = cur {
        cur = *body;
    }
    match cur {
        Term::Computation { result, .. } => *result,
        other => other,
    }
}

fn peel_pi(mut ty: Term) -> Result<(Term, Term), String> {
    loop {
        match ty {
            Term::Pi { binder, body }
                if binder.explicitness == Explicitness::Implicit =>
            {
                ty = *body;
            }
            Term::Pi { binder, body } => return Ok((*binder.ty, *body)),
            _ => return Err(format!("expected Pi type, got `{ty:?}`")),
        }
    }
}

fn runtime_ty_from_term(term: Term, symbols: &CodegenSymbols) -> Result<RuntimeTy, String> {
    match term {
        Term::Refinement { binder, .. } => runtime_ty_from_term(*binder.ty, symbols),
        Term::Computation { result, .. } => runtime_ty_from_term(*result, symbols),
        Term::Global(id) => {
            if let Some(rt) = RuntimeTy::from_prim(id) {
                return Ok(rt);
            }
            if let Some(name) = symbols.structs.get(&id) {
                return Ok(RuntimeTy::Struct(name.clone()));
            }
            if let Some(name) = symbols.enums.get(&id) {
                return Ok(RuntimeTy::Enum(name.clone()));
            }
            if let Some(rt) = symbols.arrays.get(&id) {
                return Ok(rt.clone());
            }
            if let Some(rt) = symbols.ptrs.get(&id) {
                return Ok(rt.clone());
            }
            if let Some(rt) = symbols.matrices.get(&id) {
                return Ok(rt.clone());
            }
            Err(format!("unknown type global `{id:?}`"))
        }
        _ => Err(format!("expected type global, got `{term:?}`")),
    }
}

fn erase_expr(
    term: &Term,
    symbols: &CodegenSymbols,
    data: &DataEnv,
    strings: &mut Vec<String>,
) -> Result<ErasedExpr, String> {
    match term {
        Term::I32(n) => Ok(ErasedExpr::LitI32(*n)),
        Term::Bool(b) => Ok(ErasedExpr::LitBool(*b)),
        Term::LitInt { value, ty } => Ok(ErasedExpr::LitInt {
            value: *value,
            ty: RuntimeTy::from_prim(*ty).ok_or_else(|| format!("unknown lit type `{ty:?}`"))?,
        }),
        Term::LitFloat { value, ty } => Ok(ErasedExpr::LitFloat {
            value: *value,
            ty: RuntimeTy::from_prim(*ty).ok_or_else(|| format!("unknown lit type `{ty:?}`"))?,
        }),
        Term::LitStr(s) => Ok(ErasedExpr::LitStr(intern_string(strings, s))),
        Term::Unit => Ok(ErasedExpr::Unit),
        Term::Var(level) => Ok(ErasedExpr::Var(level.0)),
        Term::Let { value, body, .. } => Ok(ErasedExpr::Let {
            value: Box::new(erase_expr(value, symbols, data, strings)?),
            body: Box::new(erase_expr(body, symbols, data, strings)?),
        }),
        Term::App { .. } => {
            let (head, args) = collect_app_args(term);
            erase_app(head, &args, symbols, data, strings)
        }
        Term::DataCtor {
            type_def,
            variant,
            args,
        } => {
            if let Some(name) = symbols.structs.get(type_def) {
                let fields = args
                    .iter()
                    .map(|a| erase_expr(a, symbols, data, strings))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(ErasedExpr::StructAgg {
                    name: name.clone(),
                    fields,
                });
            }
            if let Some(name) = symbols.enums.get(type_def) {
                let ctor_args = args
                    .iter()
                    .map(|a| erase_expr(a, symbols, data, strings))
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok(ErasedExpr::EnumAgg {
                    name: name.clone(),
                    variant: *variant,
                    args: ctor_args,
                });
            }
            Err(format!("unknown data ctor type `{type_def:?}`"))
        }
        Term::DataProj {
            value,
            type_def,
            field,
        } => {
            let name = symbols
                .structs
                .get(type_def)
                .ok_or_else(|| format!("projection on unknown struct `{type_def:?}`"))?
                .clone();
            Ok(ErasedExpr::FieldGet {
                struct_name: name,
                value: Box::new(erase_expr(value, symbols, data, strings)?),
                index: *field,
            })
        }
        Term::DataMatch {
            scrutinee,
            enum_def,
            arms,
        } => {
            let enum_name = symbols
                .enums
                .get(enum_def)
                .ok_or_else(|| format!("match on unknown enum `{enum_def:?}`"))?
                .clone();
            let mut erased_arms = Vec::new();
            for arm in arms {
                let bindings = variant_binding_count(data, *enum_def, arm.variant_index)?;
                erased_arms.push(ErasedArm {
                    variant: arm.variant_index,
                    bindings,
                    body: erase_expr(&arm.body, symbols, data, strings)?,
                });
            }
            Ok(ErasedExpr::Match {
                enum_name,
                scrutinee: Box::new(erase_expr(scrutinee, symbols, data, strings)?),
                arms: erased_arms,
            })
        }
        Term::Global(id) => {
            if let Some(name) = symbols.fns.get(id) {
                Ok(ErasedExpr::CallFn(name.clone(), vec![]))
            } else {
                Err(format!("bare global `{id:?}` in erased term"))
            }
        }
        Term::ArrayLit { elem_ty, elems } => {
            let rt = runtime_ty_from_gid(*elem_ty, symbols)?;
            let es = elems
                .iter()
                .map(|e| erase_expr(e, symbols, data, strings))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ErasedExpr::ArrayLit(rt, es))
        }
        Term::ArrayGet {
            elem_ty,
            arr,
            index,
            ..
        } => {
            let rt = runtime_ty_from_gid(*elem_ty, symbols)?;
            Ok(ErasedExpr::ArrayGet(
                rt,
                Box::new(erase_expr(arr, symbols, data, strings)?),
                Box::new(erase_expr(index, symbols, data, strings)?),
            ))
        }
        Term::ArraySet {
            elem_ty,
            arr,
            index,
            value,
            ..
        } => {
            let rt = runtime_ty_from_gid(*elem_ty, symbols)?;
            Ok(ErasedExpr::ArraySet(
                rt,
                Box::new(erase_expr(arr, symbols, data, strings)?),
                Box::new(erase_expr(index, symbols, data, strings)?),
                Box::new(erase_expr(value, symbols, data, strings)?),
            ))
        }
        Term::AddrOf { inner_ty, value } => {
            let rt = runtime_ty_from_gid(*inner_ty, symbols)?;
            Ok(ErasedExpr::AddrOf(
                rt,
                Box::new(erase_expr(value, symbols, data, strings)?),
            ))
        }
        Term::Deref { inner_ty, ptr } => {
            let rt = runtime_ty_from_gid(*inner_ty, symbols)?;
            Ok(ErasedExpr::Deref(
                rt,
                Box::new(erase_expr(ptr, symbols, data, strings)?),
            ))
        }
        Term::Len { elem_ty, arr, .. } => {
            let rt = runtime_ty_from_gid(*elem_ty, symbols)?;
            Ok(ErasedExpr::Len(
                rt,
                Box::new(erase_expr(arr, symbols, data, strings)?),
            ))
        }
        Term::HeapAlloc { ptr_ty, value } => {
            let rt = runtime_ty_from_gid(*ptr_ty, symbols)?;
            Ok(ErasedExpr::HeapAlloc(
                rt,
                Box::new(erase_expr(value, symbols, data, strings)?),
            ))
        }
        Term::HeapDealloc { ptr_ty, ptr } => {
            let rt = runtime_ty_from_gid(*ptr_ty, symbols)?;
            Ok(ErasedExpr::HeapDealloc(
                rt,
                Box::new(erase_expr(ptr, symbols, data, strings)?),
            ))
        }
        Term::HeapRealloc {
            ptr_ty,
            ptr,
            value,
        } => {
            let rt = runtime_ty_from_gid(*ptr_ty, symbols)?;
            Ok(ErasedExpr::HeapRealloc(
                rt,
                Box::new(erase_expr(ptr, symbols, data, strings)?),
                Box::new(erase_expr(value, symbols, data, strings)?),
            ))
        }
        Term::MatrixNew {
            matrix_ty,
            rows,
            cols,
            outer_array_ty,
            src,
            ..
        } => {
            let elem = matrix_cell_rt(runtime_ty_from_gid(*matrix_ty, symbols)?)?;
            let outer_rt = runtime_ty_from_gid(*outer_array_ty, symbols)?;
            Ok(ErasedExpr::MatrixNew {
                elem,
                rows: *rows,
                cols: *cols,
                outer_rt,
                src: Box::new(erase_expr(src, symbols, data, strings)?),
            })
        }
        Term::MatrixToArray {
            matrix_ty,
            rows,
            cols,
            outer_array_ty,
            matrix,
            ..
        } => {
            let elem = matrix_cell_rt(runtime_ty_from_gid(*matrix_ty, symbols)?)?;
            let outer_rt = runtime_ty_from_gid(*outer_array_ty, symbols)?;
            Ok(ErasedExpr::MatrixToArray {
                elem,
                rows: *rows,
                cols: *cols,
                outer_rt,
                matrix: Box::new(erase_expr(matrix, symbols, data, strings)?),
            })
        }
        Term::MatrixDrop { matrix_ty, matrix } => {
            let rt = runtime_ty_from_gid(*matrix_ty, symbols)?;
            Ok(ErasedExpr::MatrixDrop(
                rt,
                Box::new(erase_expr(matrix, symbols, data, strings)?),
            ))
        }
        Term::While { cond, body } => Ok(ErasedExpr::While(
            Box::new(erase_expr(cond, symbols, data, strings)?),
            Box::new(erase_expr(body, symbols, data, strings)?),
        )),
        Term::Loop { body } => Ok(ErasedExpr::Loop(Box::new(erase_expr(
            body, symbols, data, strings,
        )?))),
        Term::For {
            var,
            start,
            end,
            body,
        } => Ok(ErasedExpr::For {
            var: var.clone(),
            start: Box::new(erase_expr(start, symbols, data, strings)?),
            end: Box::new(erase_expr(end, symbols, data, strings)?),
            body: Box::new(erase_expr(body, symbols, data, strings)?),
        }),
        Term::Break => Ok(ErasedExpr::Break),
        Term::Assign { target, value } => Ok(ErasedExpr::Assign(
            Box::new(erase_expr(target, symbols, data, strings)?),
            Box::new(erase_expr(value, symbols, data, strings)?),
        )),
        Term::Admit { .. } => Ok(ErasedExpr::Unit),
        Term::Quant { kind, args } => {
            let erased_args = args
                .iter()
                .map(|a| erase_expr(a, symbols, data, strings))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ErasedExpr::Quant {
                kind: *kind,
                args: erased_args,
            })
        }
        Term::Computation { .. }
        | Term::Lam { .. }
        | Term::Pi { .. }
        | Term::Refinement { .. }
        | Term::Universe(_)
        | Term::Meta(_)
        | Term::Error => Err(format!("type-level term in erased expr: `{term:?}`")),
    }
}

fn runtime_ty_from_gid(id: DefId, symbols: &CodegenSymbols) -> Result<RuntimeTy, String> {
    runtime_ty_from_term_global(id, symbols)
}

fn matrix_cell_rt(rt: RuntimeTy) -> Result<RuntimeTy, String> {
    match rt {
        RuntimeTy::Matrix { elem } => Ok(*elem),
        other => Err(format!("expected matrix runtime type, got `{other:?}`")),
    }
}

pub fn runtime_ty_from_term_global(id: DefId, symbols: &CodegenSymbols) -> Result<RuntimeTy, String> {
    runtime_ty_from_term(Term::Global(id), symbols)
}

fn variant_binding_count(data: &DataEnv, enum_id: DefId, variant: u32) -> Result<u32, String> {
    let fields = data
        .variant_fields(enum_id, variant)
        .ok_or_else(|| format!("unknown enum variant {variant}"))?;
    Ok(match fields {
        VariantFields::Unit => 0,
        VariantFields::Tuple(ts) | VariantFields::Struct(ts) => ts.len() as u32,
    })
}

fn collect_app_args(term: &Term) -> (&Term, Vec<&Term>) {
    let mut args = Vec::new();
    let mut cur = term;
    while let Term::App { fun, arg } = cur {
        args.push(arg.as_ref());
        cur = fun.as_ref();
    }
    args.reverse();
    (cur, args)
}

fn erase_app(
    head: &Term,
    args: &[&Term],
    symbols: &CodegenSymbols,
    data: &DataEnv,
    strings: &mut Vec<String>,
) -> Result<ErasedExpr, String> {
    let Term::Global(id) = head else {
        return Err(format!("application head must be global, got `{head:?}`"));
    };

    if let Some(builtin) = symbols.builtins.get(id) {
        let erased_args = args
            .iter()
            .map(|a| erase_expr(a, symbols, data, strings))
            .collect::<Result<Vec<_>, _>>()?;
        return match builtin {
            RuntimeBuiltin::BinOp(op, ty) => {
                if erased_args.len() != 2 {
                    return Err("binary builtin arity mismatch".into());
                }
                Ok(ErasedExpr::BinOp(
                    *op,
                    ty.clone(),
                    Box::new(erased_args[0].clone()),
                    Box::new(erased_args[1].clone()),
                ))
            }
            RuntimeBuiltin::Cmp(op, ty) => {
                if erased_args.len() != 2 {
                    return Err("comparison builtin arity mismatch".into());
                }
                Ok(ErasedExpr::Cmp(
                    *op,
                    ty.clone(),
                    Box::new(erased_args[0].clone()),
                    Box::new(erased_args[1].clone()),
                ))
            }
            RuntimeBuiltin::If(ty) => {
                if erased_args.len() != 3 {
                    return Err("if builtin arity mismatch".into());
                }
                Ok(ErasedExpr::If(
                    ty.clone(),
                    Box::new(erased_args[0].clone()),
                    Box::new(erased_args[1].clone()),
                    Box::new(erased_args[2].clone()),
                ))
            }
            RuntimeBuiltin::Println(ty) => {
                if erased_args.len() != 1 {
                    return Err("println arity mismatch".into());
                }
                Ok(ErasedExpr::Println(
                    ty.clone(),
                    Box::new(erased_args[0].clone()),
                ))
            }
            RuntimeBuiltin::StrEq => {
                if erased_args.len() != 2 {
                    return Err("str_eq arity mismatch".into());
                }
                Ok(ErasedExpr::StrEq(
                    Box::new(erased_args[0].clone()),
                    Box::new(erased_args[1].clone()),
                ))
            }
        };
    }

    if let Some(name) = symbols.fns.get(id) {
        let call_args = args
            .iter()
            .map(|a| erase_expr(a, symbols, data, strings))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(ErasedExpr::CallFn(name.clone(), call_args));
    }

    Err(format!("unknown global application `{id:?}`"))
}
