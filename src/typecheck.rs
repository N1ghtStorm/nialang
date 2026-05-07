use std::collections::{HashMap, HashSet};

use crate::ast::{
    Block, EnumDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt, StructDef, Ty,
};
use crate::nia_std::{ALLOC, DEALLOC, PRINTLN, REALLOC};

pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Option<Ty>,
}

fn normalize_ty(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> Result<Ty, String> {
    match t {
        Ty::Struct(name) => {
            if enums.contains_key(name) {
                Ok(Ty::Enum(name.clone()))
            } else if structs.contains_key(name) {
                Ok(Ty::Struct(name.clone()))
            } else {
                Err(format!("unknown type `{name}`"))
            }
        }
        Ty::Ptr(inner) => Ok(Ty::Ptr(Box::new(normalize_ty(inner, structs, enums)?))),
        Ty::Array(elem, n) => Ok(Ty::Array(Box::new(normalize_ty(elem, structs, enums)?), *n)),
        other => Ok(other.clone()),
    }
}

/// `arr[i][j] = …` is allowed only when the chain is rooted at a simple variable (`arr`, not `f()[i]`).
fn index_chain_root_is_local_var(e: &Expr) -> bool {
    match e {
        Expr::Ident(_) => true,
        Expr::Index(a, _) => index_chain_root_is_local_var(a.as_ref()),
        _ => false,
    }
}

fn enum_variant<'a>(edef: &'a EnumDef, variant: &str) -> Option<&'a EnumVariantFields> {
    edef.variants
        .iter()
        .find(|v| v.name == variant)
        .map(|v| &v.fields)
}

/// Builds global symbol tables used by later typechecking passes.
///
/// ## Outputs
/// - `struct_map`: struct name -> full `StructDef`
/// - `fn_sigs`: function name -> parameter/return signature
///
/// ## Validations in this stage
/// - rejects duplicate struct names,
/// - rejects duplicate function names,
/// - reserves builtin name `println` (user code cannot redefine it).
///
/// This pass is intentionally shallow: it does not typecheck function bodies.
pub fn collect_sigs(
    structs: &[StructDef],
    enums: &[EnumDef],
    fns: &[FnDef],
) -> Result<(HashMap<String, StructDef>, HashMap<String, EnumDef>, HashMap<String, FnSig>), String>
{
    let mut struct_map: HashMap<String, StructDef> = HashMap::new();
    for s in structs {
        if struct_map.insert(s.name.clone(), s.clone()).is_some() {
            return Err(format!("duplicate struct {}", s.name));
        }
    }
    let mut enum_map: HashMap<String, EnumDef> = HashMap::new();
    for e in enums {
        if struct_map.contains_key(&e.name) {
            return Err(format!("duplicate type name {}", e.name));
        }
        if enum_map.insert(e.name.clone(), e.clone()).is_some() {
            return Err(format!("duplicate enum {}", e.name));
        }
    }
    let mut normalized_structs = struct_map.clone();
    let mut normalized_enums = enum_map.clone();
    for e in normalized_enums.values_mut() {
        for v in &mut e.variants {
            match &mut v.fields {
                EnumVariantFields::Unit => {}
                EnumVariantFields::Tuple(ts) => {
                    for t in ts {
                        *t = normalize_ty(t, &struct_map, &enum_map)?;
                    }
                }
                EnumVariantFields::Struct(fs) => {
                    for (_, t) in fs {
                        *t = normalize_ty(t, &struct_map, &enum_map)?;
                    }
                }
            }
        }
    }

    for s in normalized_structs.values_mut() {
        for (_, t) in &mut s.fields {
            *t = normalize_ty(t, &struct_map, &enum_map)?;
        }
    }

    let mut fn_sigs: HashMap<String, FnSig> = HashMap::new();
    for f in fns {
        if f.name == PRINTLN || f.name == ALLOC || f.name == DEALLOC || f.name == REALLOC {
            return Err(format!(
                "function name `{}` is reserved for the standard library",
                f.name
            ));
        }
        if fn_sigs
            .insert(
                f.name.clone(),
                FnSig {
                    params: f
                        .params
                        .iter()
                        .map(|(_, t)| normalize_ty(t, &struct_map, &enum_map))
                        .collect::<Result<Vec<_>, _>>()?,
                    ret: match &f.ret {
                        Some(t) => Some(normalize_ty(t, &struct_map, &enum_map)?),
                        None => None,
                    },
                },
            )
            .is_some()
        {
            return Err(format!("duplicate function {}", f.name));
        }
    }
    Ok((normalized_structs, normalized_enums, fn_sigs))
}

/// Typechecks one function against global symbol tables.
///
/// ## Responsibilities
/// - seeds local env with parameter types,
/// - validates each statement in order (mutating local env),
/// - validates tail expression for non-void functions,
/// - rejects trailing expression in void functions.
///
/// Returns final local environment (useful for tests/debugging).
pub fn check_fn(
    f: &FnDef,
    struct_fields: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<HashMap<String, Ty>, String> {
    let sig = fn_sigs
        .get(&f.name)
        .ok_or_else(|| format!("missing sig for {}", f.name))?;
    if sig.params.len() != f.params.len() {
        return Err("internal param len mismatch".into());
    }
    let mut env: HashMap<String, Ty> = HashMap::new();
    for ((pname, _), pty) in f.params.iter().zip(&sig.params) {
        if env.contains_key(pname) {
            return Err(format!(
                "duplicate parameter `{pname}` in function `{}`",
                f.name
            ));
        }
        env.insert(pname.clone(), pty.clone());
    }
    for st in &f.body.stmts {
        check_stmt(
            st,
            &mut env,
            struct_fields,
            enums,
            fn_sigs,
            f.ret.as_ref(),
            0,
            false,
        )?;
    }
    if let Some(ret_ty) = &f.ret {
        let tail = f
            .body
            .tail
            .as_ref()
            .ok_or_else(|| format!("function {} must end with an expression", f.name))?;
        let t = infer_expr(tail, &env, struct_fields, enums, fn_sigs, Some(ret_ty))?;
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

/// Structural type equality used by semantic checks and assertions.
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
        (Ty::Enum(x), Ty::Enum(y)) => x == y,
        (Ty::Ptr(x), Ty::Ptr(y)) => types_equal(x, y),
        _ => false,
    }
}

/// Returns whether type is one of supported integer primitives.
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

/// Returns whether type is a primitive printable scalar (`int` or `bool`).
fn is_primitive_ty(t: &Ty) -> bool {
    is_integer_ty(t) || matches!(t, Ty::Bool)
}

/// Public printable-type predicate used for builtin `println`.
///
/// Includes recursive composites such as arrays/structs/enums of printable fields and pointers.
fn is_printable_ty(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
) -> bool {
    let mut seen = HashSet::new();
    is_printable_ty_inner(t, structs, enums, &mut seen)
}

/// Recursive implementation for printable-type checks with cycle protection.
fn is_printable_ty_inner(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    seen: &mut HashSet<String>,
) -> bool {
    match t {
        x if is_primitive_ty(x) => true,
        Ty::Array(elem, _) => is_printable_ty_inner(elem, structs, enums, seen),
        Ty::Ptr(_) => true,
        Ty::Struct(name) => {
            if !seen.insert(name.clone()) {
                return true;
            }
            let Some(def) = structs.get(name) else {
                return false;
            };
            let ok = def
                .fields
                .iter()
                .all(|(_, ft)| is_printable_ty_inner(ft, structs, enums, seen));
            seen.remove(name);
            ok
        }
        Ty::Enum(name) => {
            let key = format!("enum:{name}");
            if !seen.insert(key.clone()) {
                return true;
            }
            let Some(edef) = enums.get(name) else {
                return false;
            };
            let ok = edef.variants.iter().all(|v| match &v.fields {
                crate::ast::EnumVariantFields::Unit => true,
                crate::ast::EnumVariantFields::Tuple(ts) => ts
                    .iter()
                    .all(|ft| is_printable_ty_inner(ft, structs, enums, seen)),
                crate::ast::EnumVariantFields::Struct(fs) => fs
                    .iter()
                    .all(|(_, ft)| is_printable_ty_inner(ft, structs, enums, seen)),
            });
            seen.remove(&key);
            ok
        }
        _ => false,
    }
}

/// Infers and validates expression type under current context.
///
/// ## Inputs
/// - `env`: local variable type environment
/// - `structs` / `fns`: global symbol tables
/// - `hint`: optional expected type from surrounding context
///
/// ## Notes
/// - Integer literals use `hint` for width/sign selection.
/// - Builtin `println` is treated as a special-case call and returns `Ty::Unit`.
/// - Function/struct constructor calls validate arity and per-arg compatibility.
fn infer_arithmetic_bin(
    l: &Expr,
    r: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    fns: &HashMap<String, FnSig>,
    op: &str,
) -> Result<Ty, String> {
    let tl = infer_expr(l, env, structs, enums, fns, None)?;
    if matches!(tl, Ty::Unit) {
        return Err(format!("void value on the left of `{op}`"));
    }
    if matches!(tl, Ty::Ptr(_)) {
        return Err(format!("cannot use `{op}` on a pointer value"));
    }
    if !is_integer_ty(&tl) {
        return Err(format!("cannot use `{op}` on non-integer type {tl:?}"));
    }
    let tr = infer_expr(r, env, structs, enums, fns, Some(&tl))?;
    if matches!(tr, Ty::Unit) {
        return Err(format!("void value on the right of `{op}`"));
    }
    if matches!(tr, Ty::Ptr(_)) {
        return Err(format!("cannot use `{op}` on a pointer value"));
    }
    if !is_integer_ty(&tr) {
        return Err(format!("cannot use `{op}` on non-integer type {tr:?}"));
    }
    if !types_equal(&tl, &tr) {
        return Err(format!("`{op}` operands differ: {tl:?} vs {tr:?}"));
    }
    Ok(tl)
}

fn infer_expr(
    e: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
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
        Expr::Neg(inner) => {
            let t = infer_expr(inner, env, structs, enums, fns, None)?;
            if matches!(t, Ty::Unit) {
                return Err("void value in unary `-`".into());
            }
            if matches!(t, Ty::Ptr(_)) {
                return Err("cannot negate a pointer value".into());
            }
            if !is_integer_ty(&t) {
                return Err(format!("cannot negate non-integer type {t:?}"));
            }
            Ok(t)
        }
        Expr::Add(l, r) => infer_arithmetic_bin(l, r, env, structs, enums, fns, "+"),
        Expr::Sub(l, r) => infer_arithmetic_bin(l, r, env, structs, enums, fns, "-"),
        Expr::Mul(l, r) => infer_arithmetic_bin(l, r, env, structs, enums, fns, "*"),
        Expr::Div(l, r) => {
            if matches!(r.as_ref(), Expr::Int(0)) {
                return Err("division by zero".into());
            }
            infer_arithmetic_bin(l, r, env, structs, enums, fns, "/")
        }
        Expr::Call { name, args } => {
            if name == PRINTLN {
                if args.len() != 1 {
                    return Err(format!(
                        "`{PRINTLN}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let t = infer_expr(&args[0], env, structs, enums, fns, None)?;
                if !is_printable_ty(&t, structs, enums) {
                    return Err(format!(
                        "`{PRINTLN}` expects printable type, got {t:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if name == ALLOC {
                if args.len() != 1 {
                    return Err(format!("`{ALLOC}` expects exactly 1 argument, got {}", args.len()));
                }
                let t = infer_expr(&args[0], env, structs, enums, fns, None)?;
                if matches!(t, Ty::Unit) {
                    return Err(format!("`{ALLOC}` cannot allocate `()`"));
                }
                return Ok(Ty::Ptr(Box::new(t)));
            }
            if name == DEALLOC {
                if args.len() != 1 {
                    return Err(format!(
                        "`{DEALLOC}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let t = infer_expr(&args[0], env, structs, enums, fns, None)?;
                if !matches!(t, Ty::Ptr(_)) {
                    return Err(format!("`{DEALLOC}` expects a pointer, got {t:?}"));
                }
                return Ok(Ty::Unit);
            }
            if name == REALLOC {
                if args.len() != 2 {
                    return Err(format!(
                        "`{REALLOC}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                let pt = infer_expr(&args[0], env, structs, enums, fns, None)?;
                let Ty::Ptr(pointee) = pt else {
                    return Err(format!("`{REALLOC}` first argument must be pointer, got {pt:?}"));
                };
                let vt = infer_expr(&args[1], env, structs, enums, fns, Some(&pointee))?;
                if !types_equal(&vt, &pointee) {
                    return Err(format!(
                        "`{REALLOC}` value type mismatch: expected {pointee:?}, got {vt:?}"
                    ));
                }
                return Ok(Ty::Ptr(pointee));
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
                    let at = infer_expr(a, env, structs, enums, fns, Some(ft))?;
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
                let at = infer_expr(a, env, structs, enums, fns, Some(pt))?;
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
        Expr::EnumVariant { enum_name, variant } => {
            let edef = enums
                .get(enum_name)
                .ok_or_else(|| format!("unknown enum `{enum_name}`"))?;
            let Some(fields) = enum_variant(edef, variant) else {
                return Err(format!(
                    "enum `{enum_name}` has no variant `{variant}`"
                ));
            };
            if !matches!(fields, EnumVariantFields::Unit) {
                return Err(format!(
                    "enum variant `{enum_name}::{variant}` requires payload"
                ));
            }
            Ok(Ty::Enum(enum_name.clone()))
        }
        Expr::EnumTuple {
            enum_name,
            variant,
            args,
        } => {
            let edef = enums
                .get(enum_name)
                .ok_or_else(|| format!("unknown enum `{enum_name}`"))?;
            let Some(fields) = enum_variant(edef, variant) else {
                return Err(format!(
                    "enum `{enum_name}` has no variant `{variant}`"
                ));
            };
            let EnumVariantFields::Tuple(ts) = fields else {
                return Err(format!(
                    "enum variant `{enum_name}::{variant}` is not tuple-style"
                ));
            };
            if args.len() != ts.len() {
                return Err(format!(
                    "enum variant `{enum_name}::{variant}` expects {} args, got {}",
                    ts.len(),
                    args.len()
                ));
            }
            for (a, t) in args.iter().zip(ts) {
                let at = infer_expr(a, env, structs, enums, fns, Some(t))?;
                if !types_equal(&at, t) {
                    return Err(format!(
                        "enum variant `{enum_name}::{variant}` arg mismatch: expected {t:?}, got {at:?}"
                    ));
                }
            }
            Ok(Ty::Enum(enum_name.clone()))
        }
        Expr::EnumStruct {
            enum_name,
            variant,
            fields,
        } => {
            let edef = enums
                .get(enum_name)
                .ok_or_else(|| format!("unknown enum `{enum_name}`"))?;
            let Some(vfields) = enum_variant(edef, variant) else {
                return Err(format!(
                    "enum `{enum_name}` has no variant `{variant}`"
                ));
            };
            let EnumVariantFields::Struct(def_fields) = vfields else {
                return Err(format!(
                    "enum variant `{enum_name}::{variant}` is not struct-style"
                ));
            };
            if fields.len() != def_fields.len() {
                return Err(format!(
                    "enum variant `{enum_name}::{variant}` expects {} fields, got {}",
                    def_fields.len(),
                    fields.len()
                ));
            }
            for (fname, fty) in def_fields {
                let Some((_, fe)) = fields.iter().find(|(n, _)| n == fname) else {
                    return Err(format!(
                        "enum variant `{enum_name}::{variant}` missing field `{fname}`"
                    ));
                };
                let et = infer_expr(fe, env, structs, enums, fns, Some(fty))?;
                if !types_equal(&et, fty) {
                    return Err(format!(
                        "enum variant `{enum_name}::{variant}` field `{fname}` mismatch: expected {fty:?}, got {et:?}"
                    ));
                }
            }
            Ok(Ty::Enum(enum_name.clone()))
        }
        Expr::Match { scrutinee, arms } => {
            let st = infer_expr(scrutinee, env, structs, enums, fns, None)?;
            let Ty::Enum(enum_name) = st else {
                return Err("`match` scrutinee must be enum".into());
            };
            let edef = enums
                .get(&enum_name)
                .ok_or_else(|| format!("unknown enum `{enum_name}`"))?;
            let mut seen = HashSet::new();
            let mut out_ty: Option<Ty> = None;
            for (pat, arm_expr) in arms {
                let (pat_enum, pat_variant, pat_fields) = match pat {
                    MatchPattern::Unit {
                        enum_name,
                        variant,
                    } => (enum_name, variant, None),
                    MatchPattern::Tuple {
                        enum_name,
                        variant,
                        bindings,
                    } => (enum_name, variant, Some((true, bindings))),
                    MatchPattern::Struct {
                        enum_name,
                        variant,
                        bindings,
                    } => (enum_name, variant, Some((false, bindings))),
                };
                if pat_enum != &enum_name {
                    return Err(format!(
                        "match pattern enum mismatch: expected `{enum_name}`, got `{pat_enum}`"
                    ));
                }
                let Some(vfields) = enum_variant(edef, pat_variant) else {
                    return Err(format!("enum `{enum_name}` has no variant `{pat_variant}`"));
                };
                if !seen.insert(pat_variant.to_string()) {
                    return Err(format!("duplicate match arm `{enum_name}::{pat_variant}`"));
                }
                let mut arm_env = env.clone();
                match (vfields, pat_fields) {
                    (EnumVariantFields::Unit, None) => {}
                    (EnumVariantFields::Tuple(ts), Some((true, bindings))) => {
                        if ts.len() != bindings.len() {
                            return Err(format!(
                                "match tuple pattern `{enum_name}::{pat_variant}` expects {} bindings, got {}",
                                ts.len(),
                                bindings.len()
                            ));
                        }
                        for (b, t) in bindings.iter().zip(ts) {
                            if arm_env.contains_key(b) {
                                return Err(format!(
                                    "match pattern: `{b}` is already bound (duplicate binding or shadowing is not allowed)"
                                ));
                            }
                            arm_env.insert(b.clone(), t.clone());
                        }
                    }
                    (EnumVariantFields::Struct(fs), Some((false, bindings))) => {
                        if fs.len() != bindings.len() {
                            return Err(format!(
                                "match struct pattern `{enum_name}::{pat_variant}` expects {} bindings, got {}",
                                fs.len(),
                                bindings.len()
                            ));
                        }
                        for b in bindings {
                            let Some((_, t)) = fs.iter().find(|(n, _)| n == b) else {
                                return Err(format!(
                                    "match struct pattern `{enum_name}::{pat_variant}` unknown field `{b}`"
                                ));
                            };
                            if arm_env.contains_key(b) {
                                return Err(format!(
                                    "match pattern: `{b}` is already bound (duplicate binding or shadowing is not allowed)"
                                ));
                            }
                            arm_env.insert(b.clone(), t.clone());
                        }
                    }
                    (EnumVariantFields::Unit, Some(_)) => {
                        return Err(format!(
                            "unit variant `{enum_name}::{pat_variant}` cannot bind fields"
                        ))
                    }
                    (EnumVariantFields::Tuple(_), _) => {
                        return Err(format!(
                            "tuple variant `{enum_name}::{pat_variant}` requires tuple pattern"
                        ))
                    }
                    (EnumVariantFields::Struct(_), _) => {
                        return Err(format!(
                            "struct variant `{enum_name}::{pat_variant}` requires struct pattern"
                        ))
                    }
                }
                let at = infer_expr(arm_expr, &arm_env, structs, enums, fns, hint)?;
                if let Some(prev) = &out_ty {
                    if !types_equal(prev, &at) {
                        return Err(format!(
                            "match arm types differ: expected {prev:?}, got {at:?}"
                        ));
                    }
                } else {
                    out_ty = Some(at);
                }
            }
            if seen.len() != edef.variants.len() {
                return Err(format!("non-exhaustive match on enum `{enum_name}`"));
            }
            Ok(out_ty.unwrap_or(Ty::Unit))
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
                let ft = infer_expr(fe, env, structs, enums, fns, Some(dty))?;
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
                    let et = infer_expr(e, env, structs, enums, fns, Some(elem_ty))?;
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
                let first_ty = infer_expr(first, env, structs, enums, fns, None)?;
                for e in elems.iter().skip(1) {
                    let et = infer_expr(e, env, structs, enums, fns, Some(&first_ty))?;
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
            let bt = infer_expr(obj, env, structs, enums, fns, None)?;
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
            let at = infer_expr(arr, env, structs, enums, fns, None)?;
            let it = infer_expr(idx, env, structs, enums, fns, Some(&Ty::I32))?;
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
            let ti = infer_expr(inner, env, structs, enums, fns, None)?;
            match ti {
                Ty::Ptr(p) => Ok((*p).clone()),
                _ => Err(format!("dereference requires a pointer, got {ti:?}")),
            }
        }
    }
}

fn stmt_contains_return(st: &Stmt) -> bool {
    match st {
        Stmt::Return(_) => true,
        Stmt::If { then_block, .. } => block_contains_return(then_block),
        Stmt::While { body, .. } => block_contains_return(body),
        Stmt::Loop { body } => block_contains_return(body),
        Stmt::For { body, .. } => block_contains_return(body),
        Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Assign { .. } | Stmt::Break => false,
    }
}

fn stmt_has_break(st: &Stmt) -> bool {
    match st {
        Stmt::Break => true,
        Stmt::If { then_block, .. } => block_has_break(then_block),
        Stmt::While { body, .. } | Stmt::Loop { body } | Stmt::For { body, .. } => {
            block_has_break(body)
        }
        Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Assign { .. } | Stmt::Return(_) => false,
    }
}

fn block_has_break(b: &Block) -> bool {
    b.stmts.iter().any(stmt_has_break)
}

fn block_contains_return(b: &Block) -> bool {
    b.stmts.iter().any(stmt_contains_return)
}

/// Typechecks one statement and updates local env for following statements.
///
/// Statement order matters: `let` bindings become available only after they are checked.
/// `return` checks against function declared return type.
///
/// `loop_depth` counts enclosing `loop` bodies; `break` requires `loop_depth > 0`.
/// `break_inside_while_or_for` is true inside `while` / `for` bodies (`break` is not
/// supported there yet — unlike Rust).
fn check_stmt(
    st: &Stmt,
    env: &mut HashMap<String, Ty>,
    struct_fields: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    fn_sigs: &HashMap<String, FnSig>,
    fn_ret: Option<&Ty>,
    loop_depth: u32,
    break_inside_while_or_for: bool,
) -> Result<(), String> {
    match st {
        Stmt::Let {
            name,
            ty: ann,
            init,
        } => {
            let ann_norm = match ann {
                Some(t) => Some(normalize_ty(t, struct_fields, enums)?),
                None => None,
            };
            let t = infer_expr(init, env, struct_fields, enums, fn_sigs, ann_norm.as_ref())?;
            if matches!(t, Ty::Unit) {
                return Err(format!(
                    "let {name}: cannot bind a void value (missing return?)"
                ));
            }
            if let Some(a_raw) = ann {
                let a = normalize_ty(a_raw, struct_fields, enums)?;
                if !types_equal(&a, &t) {
                    return Err(format!(
                        "let {name}: type annotation mismatch: expected {a:?}, got {t:?}"
                    ));
                }
            }
            if env.contains_key(name) {
                return Err(format!(
                    "variable `{name}` shadows an existing binding; shadowing is not allowed"
                ));
            }
            env.insert(name.clone(), t);
        }
        Stmt::Expr(e) => {
            infer_expr(e, env, struct_fields, enums, fn_sigs, None)?;
        }
        Stmt::Assign { target, value } => {
            let tt = infer_expr(target, env, struct_fields, enums, fn_sigs, None)?;
            match target {
                Expr::Ident(_) | Expr::Deref(_) => {}
                Expr::Index(_, _) if index_chain_root_is_local_var(target) => {}
                _ => {
                    return Err(
                        "assignment target must be variable, dereference, or indexed local array (e.g. `x`, `*p`, `a[i]`)"
                            .into(),
                    )
                }
            }
            let vt = infer_expr(value, env, struct_fields, enums, fn_sigs, Some(&tt))?;
            if !types_equal(&tt, &vt) {
                return Err(format!(
                    "assignment type mismatch: target {tt:?}, value {vt:?}"
                ));
            }
        }
        Stmt::Return(e) => {
            let Some(ret_ty) = fn_ret else {
                return Err("`return` is not allowed in void functions".into());
            };
            let t = infer_expr(e, env, struct_fields, enums, fn_sigs, Some(ret_ty))?;
            if !types_equal(&t, ret_ty) {
                return Err(format!(
                    "`return` type mismatch: expected {ret_ty:?}, got {t:?}"
                ));
            }
        }
        Stmt::Break => {
            if loop_depth == 0 {
                return Err("`break` is only allowed inside a `loop` body".into());
            }
            if break_inside_while_or_for {
                return Err("`break` inside `while` / `for` is not supported yet".into());
            }
        }
        Stmt::If { cond, then_block } => {
            let t = infer_expr(cond, env, struct_fields, enums, fn_sigs, Some(&Ty::Bool))?;
            if !types_equal(&t, &Ty::Bool) {
                return Err(format!("`if` condition must be bool, got {t:?}"));
            }
            let mut then_env = env.clone();
            for st in &then_block.stmts {
                check_stmt(
                    st,
                    &mut then_env,
                    struct_fields,
                    enums,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    break_inside_while_or_for,
                )?;
            }
            if let Some(tail) = &then_block.tail {
                infer_expr(tail, &then_env, struct_fields, enums, fn_sigs, None)?;
            }
        }
        Stmt::While { cond, body } => {
            let t = infer_expr(cond, env, struct_fields, enums, fn_sigs, Some(&Ty::Bool))?;
            if !types_equal(&t, &Ty::Bool) {
                return Err(format!("`while` condition must be bool, got {t:?}"));
            }
            let mut body_env = env.clone();
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    struct_fields,
                    enums,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    true,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(tail, &body_env, struct_fields, enums, fn_sigs, None)?;
            }
        }
        Stmt::Loop { body } => {
            if !block_has_break(body) {
                return Err(
                    "`loop` body must contain at least one `break` (required for correct codegen)"
                        .into(),
                );
            }
            let mut body_env = env.clone();
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    struct_fields,
                    enums,
                    fn_sigs,
                    fn_ret,
                    loop_depth.saturating_add(1),
                    false,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(
                    tail,
                    &body_env,
                    struct_fields,
                    enums,
                    fn_sigs,
                    None,
                )?;
            }
        }
        Stmt::For {
            var,
            start,
            end,
            body,
        } => {
            let ts = infer_expr(start, env, struct_fields, enums, fn_sigs, None)?;
            if !is_integer_ty(&ts) {
                return Err(format!(
                    "`for` range start must be an integer type, got {ts:?}"
                ));
            }
            let te = infer_expr(end, env, struct_fields, enums, fn_sigs, Some(&ts))?;
            if !types_equal(&ts, &te) {
                return Err(format!(
                    "`for` range end type must match start ({ts:?}), got {te:?}"
                ));
            }
            if env.contains_key(var) {
                return Err(format!(
                    "`for` variable `{var}` shadows an existing binding; shadowing is not allowed"
                ));
            }
            if block_contains_return(body) {
                return Err("`return` is not allowed inside `for` loop bodies".into());
            }
            let mut body_env = env.clone();
            body_env.insert(var.clone(), ts.clone());
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    struct_fields,
                    enums,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    true,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(tail, &body_env, struct_fields, enums, fn_sigs, None)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Parser, tokenize};

    fn parse(src: &str) -> (Vec<StructDef>, Vec<EnumDef>, Vec<FnDef>) {
        Parser::new(tokenize(src)).parse_file().expect("parse success")
    }

    fn check_all(src: &str) -> Result<(), String> {
        let (structs, enums, fns) = parse(src);
        let (struct_map, enum_map, fn_sigs) = collect_sigs(&structs, &enums, &fns)?;
        for f in &fns {
            check_fn(f, &struct_map, &enum_map, &fn_sigs)?;
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
            include_str!("../examples/tests/ok_array_index_store.nia"),
            include_str!("../examples/tests/ok_array_reverse.nia"),
            include_str!("../examples/tests/ok_print_array.nia"),
            include_str!("../examples/tests/ok_print_structs.nia"),
            include_str!("../examples/tests/ok_alloc_heap.nia"),
            include_str!("../examples/tests/ok_ptr_write.nia"),
            include_str!("../examples/tests/ok_enum_match.nia"),
            include_str!("../examples/tests/ok_enum_payload_match.nia"),
            include_str!("../examples/tests/ok_print_enum.nia"),
            include_str!("../examples/tests/ok_for_range.nia"),
            include_str!("../examples/tests/ok_while.nia"),
            include_str!("../examples/tests/ok_loop.nia"),
            include_str!("../examples/tests/ok_compound_assign.nia"),
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
    fn typecheck_rejects_shadowing_let_fixture() {
        let src = include_str!("../examples/tests/err_shadow_let.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_for_range_non_integer_fixture() {
        let src = include_str!("../examples/tests/err_for_range_bool.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_return_inside_for_fixture() {
        let src = include_str!("../examples/tests/err_for_return_in_for.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_while_cond_non_bool_fixture() {
        let src = include_str!("../examples/tests/err_while_cond_int.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_loop_without_break_fixture() {
        let src = include_str!("../examples/tests/err_loop_no_break.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_break_outside_loop_fixture() {
        let src = include_str!("../examples/tests/err_break_outside_loop.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_break_inside_while_fixture() {
        let src = include_str!("../examples/tests/err_break_in_while.nia");
        let r = check_all(src);
        assert!(r.is_err(), "{r:?}");
    }

    #[test]
    fn typecheck_rejects_div_by_zero_literal_fixture() {
        let src = include_str!("../examples/tests/err_div_by_zero.nia");
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
