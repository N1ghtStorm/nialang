use std::collections::HashMap;

use crate::ast::{Expr, FnDef, Stmt, StructDef, Ty};
use crate::nia_std::PRINTLN;

pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Option<Ty>,
}

pub fn collect_sigs(
    structs: &[StructDef],
    fns: &[FnDef],
) -> Result<(HashMap<String, StructDef>, HashMap<String, FnSig>), String> {
    let mut struct_map: HashMap<String, StructDef> = HashMap::new();
    for s in structs {
        if struct_map.insert(s.name.clone(), s.clone()).is_some() {
            return Err(format!("duplicate struct {}", s.name));
        }
    }
    let mut fn_sigs: HashMap<String, FnSig> = HashMap::new();
    for f in fns {
        if f.name == PRINTLN {
            return Err(format!(
                "function name `{PRINTLN}` is reserved for the standard library"
            ));
        }
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
    Ok((struct_map, fn_sigs))
}

pub fn check_fn(
    f: &FnDef,
    struct_fields: &HashMap<String, StructDef>,
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
        check_stmt(st, &mut env, struct_fields, fn_sigs, f.ret.as_ref())?;
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
        (Ty::I8, Ty::I8)
        | (Ty::U8, Ty::U8)
        | (Ty::I16, Ty::I16)
        | (Ty::U16, Ty::U16)
        | (Ty::I32, Ty::I32)
        | (Ty::I64, Ty::I64)
        | (Ty::U64, Ty::U64)
        | (Ty::I128, Ty::I128)
        | (Ty::Isize, Ty::Isize)
        | (Ty::Usize, Ty::Usize)
        | (Ty::U128, Ty::U128)
        | (Ty::Bool, Ty::Bool)
        | (Ty::Unit, Ty::Unit) => true,
        (Ty::Array(ax, an), Ty::Array(bx, bn)) => an == bn && types_equal(ax, bx),
        (Ty::Struct(x), Ty::Struct(y)) => x == y,
        (Ty::Ptr(x), Ty::Ptr(y)) => types_equal(x, y),
        _ => false,
    }
}

fn is_integer_ty(t: &Ty) -> bool {
    matches!(
        t,
        Ty::I8
            | Ty::U8
            | Ty::I16
            | Ty::U16
            | Ty::I32
            | Ty::I64
            | Ty::U64
            | Ty::I128
            | Ty::Isize
            | Ty::Usize
            | Ty::U128
    )
}

fn is_primitive_ty(t: &Ty) -> bool {
    is_integer_ty(t) || matches!(t, Ty::Bool)
}

fn infer_expr(
    e: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    fns: &HashMap<String, FnSig>,
    hint: Option<&Ty>,
) -> Result<Ty, String> {
    match e {
        Expr::Int(_) => match hint {
            None => Ok(Ty::I32),
            Some(other) if is_integer_ty(other) => Ok(other.clone()),
            Some(Ty::Bool) => Err("integer literal cannot satisfy bool".into()),
            Some(Ty::Struct(name)) => Err(format!(
                "integer literal cannot satisfy struct type `{name}`"
            )),
            Some(Ty::Unit) => Err("integer literal cannot satisfy `()`".into()),
            Some(Ty::Ptr(_)) => Err("integer literal cannot satisfy a pointer type".into()),
            Some(Ty::Array(_, _)) => Err("integer literal cannot satisfy array type".into()),
            Some(other) => Err(format!("integer literal cannot satisfy {other:?}")),
        },
        Expr::Bool(_) => match hint {
            Some(Ty::Bool) | None => Ok(Ty::Bool),
            Some(other) => Err(format!("bool literal cannot satisfy {other:?}")),
        },
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown variable `{name}`")),
        Expr::Add(l, r) => {
            let tl = infer_expr(l, env, structs, fns, None)?;
            if matches!(tl, Ty::Unit) {
                return Err("void value on the left of `+`".into());
            }
            if matches!(tl, Ty::Ptr(_)) {
                return Err("cannot use `+` on a pointer value".into());
            }
            if !is_integer_ty(&tl) {
                return Err(format!("cannot use `+` on non-integer type {tl:?}"));
            }
            let tr = infer_expr(r, env, structs, fns, Some(&tl))?;
            if matches!(tr, Ty::Unit) {
                return Err("void value on the right of `+`".into());
            }
            if matches!(tr, Ty::Ptr(_)) {
                return Err("cannot use `+` on a pointer value".into());
            }
            if !is_integer_ty(&tr) {
                return Err(format!("cannot use `+` on non-integer type {tr:?}"));
            }
            if !types_equal(&tl, &tr) {
                return Err(format!("add operands differ: {tl:?} vs {tr:?}"));
            }
            Ok(tl)
        }
        Expr::Call { name, args } => {
            if name == PRINTLN {
                if args.len() != 1 {
                    return Err(format!(
                        "`{PRINTLN}` expects exactly 1 primitive argument, got {}",
                        args.len()
                    ));
                }
                let t = infer_expr(&args[0], env, structs, fns, None)?;
                if !is_primitive_ty(&t) {
                    return Err(format!(
                        "`{PRINTLN}` expects primitive type, got {t:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if let Some(def) = structs.get(name) {
                if !def.is_tuple {
                    return Err(format!(
                        "`{name}` is a named-field struct; use `{name} {{ ... }}` literal syntax"
                    ));
                }
                if args.len() != def.fields.len() {
                    return Err(format!(
                        "tuple struct `{name}`: expected {} args, got {}",
                        def.fields.len(),
                        args.len()
                    ));
                }
                for (a, (_, ft)) in args.iter().zip(&def.fields) {
                    let at = infer_expr(a, env, structs, fns, Some(ft))?;
                    if !types_equal(&at, ft) {
                        return Err(format!(
                            "tuple struct `{name}`: field type mismatch: expected {ft:?}, got {at:?}"
                        ));
                    }
                }
                return Ok(Ty::Struct(name.clone()));
            }
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
            Ok(match &sig.ret {
                Some(t) => t.clone(),
                None => Ty::Unit,
            })
        }
        Expr::StructLit { name, fields } => {
            let def = structs
                .get(name)
                .ok_or_else(|| format!("unknown struct `{name}`"))?;
            let def_fields = &def.fields;
            if def.is_tuple {
                return Err(format!(
                    "tuple struct `{name}` must use constructor syntax `{name}(...)`"
                ));
            }
            for (fname, _) in fields {
                if !def_fields.iter().any(|(n, _)| n == fname) {
                    return Err(format!("struct `{name}` has no field `{fname}`"));
                }
            }
            if fields.len() != def_fields.len() {
                return Err(format!(
                    "struct `{name}` literal: expected {} fields, got {}",
                    def_fields.len(),
                    fields.len()
                ));
            }
            for (dfn, dty) in def_fields {
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
        Expr::ArrayLit(elems) => match hint {
            Some(Ty::Array(elem_ty, n)) => {
                if elems.len() != *n {
                    return Err(format!(
                        "array literal length mismatch: expected {n}, got {}",
                        elems.len()
                    ));
                }
                for e in elems {
                    let et = infer_expr(e, env, structs, fns, Some(elem_ty))?;
                    if !types_equal(&et, elem_ty) {
                        return Err(format!(
                            "array element type mismatch: expected {elem_ty:?}, got {et:?}"
                        ));
                    }
                }
                Ok(Ty::Array(elem_ty.clone(), *n))
            }
            Some(other) => Err(format!("array literal cannot satisfy {other:?}")),
            None => {
                let Some(first) = elems.first() else {
                    return Err("cannot infer type of empty array literal".into());
                };
                let first_ty = infer_expr(first, env, structs, fns, None)?;
                for e in elems.iter().skip(1) {
                    let et = infer_expr(e, env, structs, fns, Some(&first_ty))?;
                    if !types_equal(&et, &first_ty) {
                        return Err(format!(
                            "array elements differ: expected {first_ty:?}, got {et:?}"
                        ));
                    }
                }
                Ok(Ty::Array(Box::new(first_ty), elems.len()))
            }
        },
        Expr::Field(obj, fname) => {
            let bt = infer_expr(obj, env, structs, fns, None)?;
            let Ty::Struct(sname) = bt else {
                return Err("field access on non-struct".into());
            };
            let def = structs
                .get(&sname)
                .ok_or_else(|| format!("unknown struct `{sname}`"))?;
            def.fields
                .iter()
                .find(|(n, _)| n == fname)
                .map(|(_, t)| t.clone())
                .ok_or_else(|| format!("struct `{sname}` has no field `{fname}`"))
        }
        Expr::Index(arr, idx) => {
            let at = infer_expr(arr, env, structs, fns, None)?;
            let it = infer_expr(idx, env, structs, fns, Some(&Ty::I32))?;
            if !matches!(it, Ty::I32) {
                return Err(format!("array index must be i32, got {it:?}"));
            }
            match at {
                Ty::Array(elem, _) => Ok((*elem).clone()),
                other => Err(format!("indexing requires array, got {other:?}")),
            }
        }
        Expr::AddrOf(inner) => match inner.as_ref() {
            Expr::Ident(n) => {
                let t = env
                    .get(n)
                    .ok_or_else(|| format!("unknown variable `{n}` in address-of"))?;
                Ok(Ty::Ptr(Box::new(t.clone())))
            }
            _ => Err("address-of is only supported for a simple variable (e.g. `&x`)".into()),
        },
        Expr::Deref(inner) => {
            let ti = infer_expr(inner, env, structs, fns, None)?;
            match ti {
                Ty::Ptr(p) => Ok((*p).clone()),
                _ => Err(format!("dereference requires a pointer, got {ti:?}")),
            }
        }
    }
}

fn check_stmt(
    st: &Stmt,
    env: &mut HashMap<String, Ty>,
    struct_fields: &HashMap<String, StructDef>,
    fn_sigs: &HashMap<String, FnSig>,
    fn_ret: Option<&Ty>,
) -> Result<(), String> {
    match st {
        Stmt::Let {
            name,
            ty: ann,
            init,
        } => {
            let hint = ann.as_ref();
            let t = infer_expr(init, env, struct_fields, fn_sigs, hint)?;
            if matches!(t, Ty::Unit) {
                return Err(format!(
                    "let {name}: cannot bind a void value (missing return?)"
                ));
            }
            if let Some(a) = ann {
                if !types_equal(a, &t) {
                    return Err(format!(
                        "let {name}: type annotation mismatch: expected {a:?}, got {t:?}"
                    ));
                }
            }
            env.insert(name.clone(), t);
        }
        Stmt::Expr(e) => {
            infer_expr(e, env, struct_fields, fn_sigs, None)?;
        }
        Stmt::Return(e) => {
            let Some(ret_ty) = fn_ret else {
                return Err("`return` is not allowed in void functions".into());
            };
            let t = infer_expr(e, env, struct_fields, fn_sigs, Some(ret_ty))?;
            if !types_equal(&t, ret_ty) {
                return Err(format!(
                    "`return` type mismatch: expected {ret_ty:?}, got {t:?}"
                ));
            }
        }
        Stmt::If { cond, then_block } => {
            let t = infer_expr(cond, env, struct_fields, fn_sigs, Some(&Ty::Bool))?;
            if !types_equal(&t, &Ty::Bool) {
                return Err(format!("`if` condition must be bool, got {t:?}"));
            }
            let mut then_env = env.clone();
            for st in &then_block.stmts {
                check_stmt(st, &mut then_env, struct_fields, fn_sigs, fn_ret)?;
            }
            if let Some(tail) = &then_block.tail {
                infer_expr(tail, &then_env, struct_fields, fn_sigs, None)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Parser, tokenize};

    fn parse(src: &str) -> (Vec<StructDef>, Vec<FnDef>) {
        Parser::new(tokenize(src)).parse_file().expect("parse success")
    }

    fn check_all(src: &str) -> Result<(), String> {
        let (structs, fns) = parse(src);
        let (struct_map, fn_sigs) = collect_sigs(&structs, &fns)?;
        for f in &fns {
            check_fn(f, &struct_map, &fn_sigs)?;
        }
        Ok(())
    }

    #[test]
    fn typecheck_ok_fixtures() {
        let ok_files = [
            include_str!("../examples/tests/ok_minimal.nia"),
            include_str!("../examples/tests/ok_if_return.nia"),
            include_str!("../examples/tests/ok_tuple_struct.nia"),
            include_str!("../examples/tests/ok_struct_named.nia"),
            include_str!("../examples/tests/ok_print_primitives.nia"),
            include_str!("../examples/tests/ok_pointers.nia"),
            include_str!("../examples/tests/ok_nested_if.nia"),
            include_str!("../examples/tests/ok_tuple_named_mix.nia"),
            include_str!("../examples/tests/ok_array.nia"),
            include_str!("../examples/tests/ok_array_index.nia"),
        ];
        for src in ok_files {
            let r = check_all(src);
            assert!(r.is_ok(), "{r:?}");
        }
    }

    #[test]
    fn typecheck_detects_mismatch_fixture() {
        let src = include_str!("../examples/tests/err_type_mismatch.nia");
        let r = check_all(src);
        assert!(r.is_err());
    }

    #[test]
    fn typecheck_detects_add_bool_fixture() {
        let src = include_str!("../examples/tests/err_type_add_bool.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_detects_if_non_bool_fixture() {
        let src = include_str!("../examples/tests/err_type_if_non_bool.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_detects_tuple_named_literal_fixture() {
        let src = include_str!("../examples/tests/err_type_tuple_with_named_literal.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_detects_array_len_mismatch_fixture() {
        let src = include_str!("../examples/tests/err_array_len_mismatch.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_index_on_non_array() {
        let src = r#"
fn main() i32 {
    let x: i32 = 1;
    x[0]
}
"#;
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_wrong_tuple_arity() {
        let src = r#"
struct Foo (u8, i32)
fn main() i32 {
    let f = Foo(1);
    f.1
}
"#;
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_return_in_void_fn() {
        let src = r#"
fn f() {
    return 1
}
"#;
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_deref_non_pointer() {
        let src = r#"
fn main() i32 {
    let a: i32 = 1;
    *a
}
"#;
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }
}
