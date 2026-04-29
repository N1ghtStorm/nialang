use std::collections::HashMap;

use crate::ast::{Expr, FnDef, Stmt, StructDef, Ty};

pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Option<Ty>,
}

pub fn collect_sigs(
    structs: &[StructDef],
    fns: &[FnDef],
) -> Result<(HashMap<String, Vec<(String, Ty)>>, HashMap<String, FnSig>), String> {
    let mut struct_fields: HashMap<String, Vec<(String, Ty)>> = HashMap::new();
    for s in structs {
        if struct_fields.insert(s.name.clone(), s.fields.clone()).is_some() {
            return Err(format!("duplicate struct {}", s.name));
        }
    }
    let mut fn_sigs: HashMap<String, FnSig> = HashMap::new();
    for f in fns {
        if fn_sigs
            .insert(
                f.name.clone(),
                FnSig {
                    params: f.params.iter().map(|(_, t)| t.clone()).collect(),
                    ret: f.ret.clone(),
                },
            )
            .is_some()
        {
            return Err(format!("duplicate function {}", f.name));
        }
    }
    Ok((struct_fields, fn_sigs))
}

pub fn check_fn(
    f: &FnDef,
    struct_fields: &HashMap<String, Vec<(String, Ty)>>,
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<HashMap<String, Ty>, String> {
    let sig = fn_sigs
        .get(&f.name)
        .ok_or_else(|| format!("missing sig for {}", f.name))?;
    if sig.params.len() != f.params.len() {
        return Err("internal param len mismatch".into());
    }
    let mut env: HashMap<String, Ty> = HashMap::new();
    for ((pname, pty), _) in f.params.iter().zip(&sig.params) {
        env.insert(pname.clone(), pty.clone());
    }
    for st in &f.body.stmts {
        let Stmt::Let { name, ty: ann, init } = st;
        let hint = ann.as_ref();
        let t = infer_expr(init, &env, struct_fields, fn_sigs, hint)?;
        if let Some(a) = ann {
            if !types_equal(a, &t) {
                return Err(format!(
                    "let {name}: type annotation mismatch: expected {a:?}, got {t:?}"
                ));
            }
        }
        env.insert(name.clone(), t);
    }
    if let Some(ret_ty) = &f.ret {
        let tail = f
            .body
            .tail
            .as_ref()
            .ok_or_else(|| format!("function {} must end with an expression", f.name))?;
        let t = infer_expr(tail, &env, struct_fields, fn_sigs, Some(ret_ty))?;
        if !types_equal(ret_ty, &t) {
            return Err(format!(
                "function {} return type mismatch: expected {ret_ty:?}, got {t:?}",
                f.name
            ));
        }
    } else if f.body.tail.is_some() {
        return Err(format!(
            "function {} is void but has a trailing expression",
            f.name
        ));
    }
    Ok(env)
}

fn types_equal(a: &Ty, b: &Ty) -> bool {
    match (a, b) {
        (Ty::I32, Ty::I32) | (Ty::U128, Ty::U128) => true,
        (Ty::Struct(x), Ty::Struct(y)) => x == y,
        _ => false,
    }
}

fn infer_expr(
    e: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, Vec<(String, Ty)>>,
    fns: &HashMap<String, FnSig>,
    hint: Option<&Ty>,
) -> Result<Ty, String> {
    match e {
        Expr::Int(_) => match hint {
            Some(Ty::U128) => Ok(Ty::U128),
            Some(Ty::I32) | None => Ok(Ty::I32),
            Some(Ty::Struct(name)) => Err(format!(
                "integer literal cannot satisfy struct type `{name}`"
            )),
        },
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown variable `{name}`")),
        Expr::Add(l, r) => {
            let tl = infer_expr(l, env, structs, fns, None)?;
            let tr = infer_expr(r, env, structs, fns, Some(&tl))?;
            if !types_equal(&tl, &tr) {
                return Err(format!("add operands differ: {tl:?} vs {tr:?}"));
            }
            Ok(tl)
        }
        Expr::Call { name, args } => {
            let sig = fns
                .get(name)
                .ok_or_else(|| format!("unknown function `{name}`"))?;
            if args.len() != sig.params.len() {
                return Err(format!(
                    "call `{name}`: expected {} args, got {}",
                    sig.params.len(),
                    args.len()
                ));
            }
            for (a, pt) in args.iter().zip(&sig.params) {
                let at = infer_expr(a, env, structs, fns, Some(pt))?;
                if !types_equal(&at, pt) {
                    return Err(format!(
                        "call `{name}`: arg type mismatch: expected {pt:?}, got {at:?}"
                    ));
                }
            }
            sig
                .ret
                .clone()
                .ok_or_else(|| format!("call `{name}`: callee has no return value"))
        }
        Expr::StructLit { name, fields } => {
            let def = structs
                .get(name)
                .ok_or_else(|| format!("unknown struct `{name}`"))?;
            for (fname, _) in fields {
                if !def.iter().any(|(n, _)| n == fname) {
                    return Err(format!("struct `{name}` has no field `{fname}`"));
                }
            }
            if fields.len() != def.len() {
                return Err(format!(
                    "struct `{name}` literal: expected {} fields, got {}",
                    def.len(),
                    fields.len()
                ));
            }
            for (dfn, dty) in def {
                let Some((_, fe)) = fields.iter().find(|(n, _)| n == dfn) else {
                    return Err(format!("struct `{name}` missing field `{dfn}`"));
                };
                let ft = infer_expr(fe, env, structs, fns, Some(dty))?;
                if !types_equal(dty, &ft) {
                    return Err(format!(
                        "struct `{name}` field `{dfn}`: expected {dty:?}, got {ft:?}"
                    ));
                }
            }
            Ok(Ty::Struct(name.clone()))
        }
        Expr::Field(obj, fname) => {
            let bt = infer_expr(obj, env, structs, fns, None)?;
            let Ty::Struct(sname) = bt else {
                return Err("field access on non-struct".into());
            };
            let def = structs
                .get(&sname)
                .ok_or_else(|| format!("unknown struct `{sname}`"))?;
            def
                .iter()
                .find(|(n, _)| n == fname)
                .map(|(_, t)| t.clone())
                .ok_or_else(|| format!("struct `{sname}` has no field `{fname}`"))
        }
    }
}
