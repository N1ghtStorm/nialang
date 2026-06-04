use std::collections::{HashMap, HashSet};

use crate::ast::{
    Block, EnumDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt, StructDef, Ty, VectorDef,
    method_symbol,
};
use crate::nia_std::{
    ALLOC, CIS, COMPLEX_ADD, COMPLEX_DIV, COMPLEX_MUL, COMPLEX_NEW, COMPLEX_SCALE, COMPLEX_SUB,
    COS, DEALLOC, GATE_CCNOT, GATE_CCZ, GATE_CH, GATE_CNOT, GATE_CR1, GATE_CRX, GATE_CRY, GATE_CRZ,
    GATE_CS, GATE_CSDG, GATE_CSWAP, GATE_CT, GATE_CTDG, GATE_CY, GATE_CZ, GATE_H, GATE_I, GATE_R1,
    GATE_RX, GATE_RY, GATE_RZ, GATE_S, GATE_SDG, GATE_SWAP, GATE_T, GATE_TDG, GATE_X, GATE_Y,
    GATE_Z, LEN, LIST_CAPACITY, LIST_GET, LIST_LEN, LIST_NEW, LIST_PUSH, LIST_WITH_CAPACITY,
    MATRIX_CLONE, MATRIX_COLS, MATRIX_DROP, MATRIX_GET, MATRIX_LEN, MATRIX_NEW, MATRIX_REFCOUNT,
    MATRIX_ROWS, MATRIX_SET, MATRIX_TYPE, MEASURE, OUTER, PI, PRINTLN, QUBIT, READ, REALLOC,
    RECORD, RESULT, SIN, TO_ARRAY, TO_MATRIX, TO_VEC, VECTOR_CLONE, VECTOR_DROP, VECTOR_GET,
    VECTOR_LEN, VECTOR_REFCOUNT, VECTOR_SET,
};

const QUANT_SCOPE_MARKER: &str = "\0nia.quant.scope";

/// Canonical function signature table entry used across semantic passes.
///
/// We collect `FnSig` for every function before checking bodies so calls can be
/// validated in one pass (arity + argument types + return type), including
/// forward references and recursive calls.
pub struct FnSig {
    pub params: Vec<Ty>,
    pub ret: Option<Ty>,
    pub is_quantum: bool,
}

/// Resolves user-written type syntax into canonical semantic type form.
///
/// Why this exists:
/// - validates that referenced named types actually exist,
/// - disambiguates `Ty::Struct(name)` into `Ty::Enum(name)` when `name` is an enum,
/// - recursively normalizes nested types (`&T`, `[T; N]`) so later checks operate
///   on a consistent representation.
fn normalize_ty(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
) -> Result<Ty, String> {
    match t {
        Ty::Struct(name) => {
            if name == QUBIT {
                Ok(Ty::Qubit)
            } else if name == RESULT {
                Ok(Ty::Result)
            } else if name == MATRIX_TYPE {
                Err(format!(
                    "type `Matrix` is no longer a valid annotation; use `T[]` (e.g. `i32[]`)"
                ))
            } else if enums.contains_key(name) {
                Ok(Ty::Enum(name.clone()))
            } else if structs.contains_key(name) {
                Ok(Ty::Struct(name.clone()))
            } else if vectors.contains_key(name) {
                let v = vectors
                    .get(name)
                    .expect("checked vector existence before lookup");
                Ok(Ty::Vector(
                    name.clone(),
                    Box::new(normalize_ty(&v.ty, structs, enums, vectors)?),
                ))
            } else {
                Err(format!("unknown type `{name}`"))
            }
        }
        Ty::Ptr(inner) => Ok(Ty::Ptr(Box::new(normalize_ty(
            inner, structs, enums, vectors,
        )?))),
        Ty::Array(elem, n) => Ok(Ty::Array(
            Box::new(normalize_ty(elem, structs, enums, vectors)?),
            *n,
        )),
        Ty::AnonVector(elem, n) => Ok(Ty::AnonVector(
            Box::new(normalize_ty(elem, structs, enums, vectors)?),
            *n,
        )),
        Ty::HeapVector(elem) => Ok(Ty::HeapVector(Box::new(normalize_ty(
            elem, structs, enums, vectors,
        )?))),
        Ty::List(elem) => Ok(Ty::List(Box::new(normalize_ty(
            elem, structs, enums, vectors,
        )?))),
        Ty::Matrix(elem, shape) => {
            let norm = normalize_ty(elem, structs, enums, vectors)?;
            if matches!(norm, Ty::Matrix(_, _)) {
                return Err("matrix element type cannot itself be a matrix".into());
            }
            Ok(Ty::Matrix(Box::new(norm), *shape))
        }
        other => Ok(other.clone()),
    }
}

/// Assignment to index-chain is allowed for roots that are assignable array lvalues:
/// - local variable (`arr[i]`),
/// - dereference (`(*p)[i]`).
fn index_chain_root_is_assignable_array_lvalue(e: &Expr) -> bool {
    match e {
        Expr::Ident(_) | Expr::Deref(_) => true,
        Expr::Index(a, _) => index_chain_root_is_assignable_array_lvalue(a.as_ref()),
        _ => false,
    }
}

fn enum_variant<'a>(edef: &'a EnumDef, variant: &str) -> Option<&'a EnumVariantFields> {
    edef.variants
        .iter()
        .find(|v| v.name == variant)
        .map(|v| &v.fields)
}

fn is_in_quant_scope(env: &HashMap<String, Ty>) -> bool {
    env.contains_key(QUANT_SCOPE_MARKER)
}

fn enter_quant_scope(env: &HashMap<String, Ty>) -> HashMap<String, Ty> {
    let mut scoped = env.clone();
    scoped.insert(QUANT_SCOPE_MARKER.into(), Ty::Unit);
    scoped
}

fn is_single_qubit_gate(name: &str) -> bool {
    matches!(
        name,
        GATE_I | GATE_H | GATE_X | GATE_Y | GATE_Z | GATE_S | GATE_SDG | GATE_T | GATE_TDG
    )
}

fn is_two_qubit_gate(name: &str) -> bool {
    matches!(
        name,
        GATE_CNOT
            | GATE_CZ
            | GATE_SWAP
            | GATE_CH
            | GATE_CY
            | GATE_CS
            | GATE_CSDG
            | GATE_CT
            | GATE_CTDG
    )
}

fn is_three_qubit_gate(name: &str) -> bool {
    matches!(name, GATE_CCNOT | GATE_CCZ | GATE_CSWAP)
}

fn is_rotation_gate(name: &str) -> bool {
    matches!(name, GATE_RX | GATE_RY | GATE_RZ | GATE_R1)
}

fn is_controlled_rotation_gate(name: &str) -> bool {
    matches!(name, GATE_CRX | GATE_CRY | GATE_CRZ | GATE_CR1)
}

fn contains_quantum_ty(t: &Ty) -> bool {
    match t {
        Ty::Qubit | Ty::Result => true,
        Ty::Ptr(inner)
        | Ty::Array(inner, _)
        | Ty::AnonVector(inner, _)
        | Ty::HeapVector(inner)
        | Ty::List(inner)
        | Ty::Matrix(inner, _) => contains_quantum_ty(inner),
        _ => false,
    }
}

fn reject_quantum_ty(t: &Ty, context: &str) -> Result<(), String> {
    if contains_quantum_ty(t) {
        return Err(format!(
            "{context} cannot use quantum types outside `quant` blocks"
        ));
    }
    Ok(())
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
/// - reserves builtin names `println`, `len`, heap helpers (user code cannot redefine them).
///
/// This pass is intentionally shallow: it does not typecheck function bodies.
pub fn collect_sigs(
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fns: &[FnDef],
) -> Result<
    (
        HashMap<String, StructDef>,
        HashMap<String, EnumDef>,
        HashMap<String, VectorDef>,
        HashMap<String, FnSig>,
    ),
    String,
> {
    let mut struct_map: HashMap<String, StructDef> = HashMap::new();
    for s in crate::nia_std::builtin_structs() {
        struct_map.insert(s.name.clone(), s);
    }
    for s in structs {
        if crate::nia_std::is_reserved_type_name(&s.name) {
            return Err(format!("type name `{}` is reserved", s.name));
        }
        if struct_map.insert(s.name.clone(), s.clone()).is_some() {
            return Err(format!("duplicate struct {}", s.name));
        }
    }
    let mut vector_map: HashMap<String, VectorDef> = HashMap::new();
    for v in vectors {
        if crate::nia_std::is_reserved_type_name(&v.name) {
            return Err(format!("type name `{}` is reserved", v.name));
        }
        if struct_map.contains_key(&v.name) {
            return Err(format!("duplicate type name {}", v.name));
        }
        if vector_map.insert(v.name.clone(), v.clone()).is_some() {
            return Err(format!("duplicate vector {}", v.name));
        }
    }
    let mut enum_map: HashMap<String, EnumDef> = HashMap::new();
    for e in enums {
        if crate::nia_std::is_reserved_type_name(&e.name) {
            return Err(format!("type name `{}` is reserved", e.name));
        }
        if struct_map.contains_key(&e.name) || vector_map.contains_key(&e.name) {
            return Err(format!("duplicate type name {}", e.name));
        }
        if enum_map.insert(e.name.clone(), e.clone()).is_some() {
            return Err(format!("duplicate enum {}", e.name));
        }
    }
    let mut normalized_structs = struct_map.clone();
    let mut normalized_enums = enum_map.clone();
    let mut normalized_vectors = vector_map.clone();

    for v in normalized_vectors.values_mut() {
        v.ty = normalize_ty(&v.ty, &struct_map, &enum_map, &vector_map)?;
        reject_quantum_ty(&v.ty, &format!("vector `{}` element type", v.name))?;
    }

    for e in normalized_enums.values_mut() {
        for v in &mut e.variants {
            match &mut v.fields {
                EnumVariantFields::Unit => {}
                EnumVariantFields::Tuple(ts) => {
                    for t in ts {
                        *t = normalize_ty(t, &struct_map, &enum_map, &vector_map)?;
                        reject_quantum_ty(
                            t,
                            &format!("enum `{}` variant `{}` field", e.name, v.name),
                        )?;
                    }
                }
                EnumVariantFields::Struct(fs) => {
                    for (_, t) in fs {
                        *t = normalize_ty(t, &struct_map, &enum_map, &vector_map)?;
                        reject_quantum_ty(
                            t,
                            &format!("enum `{}` variant `{}` field", e.name, v.name),
                        )?;
                    }
                }
            }
        }
    }

    for s in normalized_structs.values_mut() {
        for (_, t) in &mut s.fields {
            *t = normalize_ty(t, &struct_map, &enum_map, &vector_map)?;
            reject_quantum_ty(t, &format!("struct `{}` field", s.name))?;
        }
    }

    let mut fn_sigs: HashMap<String, FnSig> = HashMap::new();
    for f in fns {
        if crate::nia_std::is_reserved_fn_name(&f.name) {
            return Err(format!(
                "function name `{}` is reserved for the standard library",
                f.name
            ));
        }
        if f.is_quantum && f.is_extern {
            return Err(format!(
                "function `{}` cannot be both `quant` and `extern`",
                f.name
            ));
        }
        let params = f
            .params
            .iter()
            .map(|(_, t)| normalize_ty(t, &struct_map, &enum_map, &vector_map))
            .collect::<Result<Vec<_>, _>>()?;
        if !f.is_quantum {
            for ((pname, _), pty) in f.params.iter().zip(&params) {
                reject_quantum_ty(pty, &format!("function `{}` parameter `{pname}`", f.name))?;
            }
        }
        let ret = match &f.ret {
            Some(t) => {
                let ret_ty = normalize_ty(t, &struct_map, &enum_map, &vector_map)?;
                if !f.is_quantum {
                    reject_quantum_ty(&ret_ty, &format!("function `{}` return type", f.name))?;
                }
                Some(ret_ty)
            }
            None => None,
        };
        if fn_sigs
            .insert(
                f.name.clone(),
                FnSig {
                    params,
                    ret,
                    is_quantum: f.is_quantum,
                },
            )
            .is_some()
        {
            return Err(format!("duplicate function {}", f.name));
        }
    }
    Ok((
        normalized_structs,
        normalized_enums,
        normalized_vectors,
        fn_sigs,
    ))
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
    vectors: &HashMap<String, VectorDef>,
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<HashMap<String, Ty>, String> {
    let sig = fn_sigs
        .get(&f.name)
        .ok_or_else(|| format!("missing sig for {}", f.name))?;
    if sig.params.len() != f.params.len() {
        return Err("internal param len mismatch".into());
    }
    if f.is_extern {
        check_extern_c_abi(f, sig)?;
    }
    let mut env: HashMap<String, Ty> = if f.is_quantum {
        enter_quant_scope(&HashMap::new())
    } else {
        HashMap::new()
    };
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
            vectors,
            fn_sigs,
            sig.ret.as_ref(),
            0,
            false,
        )?;
    }
    if let Some(ret_ty) = &sig.ret {
        let tail = f
            .body
            .tail
            .as_ref()
            .ok_or_else(|| format!("function {} must end with an expression", f.name))?;
        let t = infer_expr(
            tail,
            &env,
            struct_fields,
            enums,
            vectors,
            fn_sigs,
            Some(ret_ty),
        )?;
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
        | (Ty::F16, Ty::F16)
        | (Ty::F32, Ty::F32)
        | (Ty::F64, Ty::F64)
        | (Ty::String, Ty::String)
        | (Ty::Qubit, Ty::Qubit)
        | (Ty::Result, Ty::Result)
        | (Ty::Unit, Ty::Unit) => true,
        (Ty::Array(ax, an), Ty::Array(bx, bn)) => an == bn && types_equal(ax, bx),
        (Ty::Struct(x), Ty::Struct(y)) => x == y,
        (Ty::Vector(xn, xt), Ty::Vector(yn, yt)) => xn == yn && types_equal(xt, yt),
        (Ty::AnonVector(xt, xn), Ty::AnonVector(yt, yn)) => xn == yn && types_equal(xt, yt),
        (Ty::HeapVector(x), Ty::HeapVector(y)) => types_equal(x, y),
        (Ty::List(x), Ty::List(y)) => types_equal(x, y),
        // Vector values are currently represented as struct-shaped aggregates in AST/codegen.
        // Accept name-equivalence across these forms at semantic boundaries.
        (Ty::Struct(x), Ty::Vector(y, _)) | (Ty::Vector(y, _), Ty::Struct(x)) => x == y,
        (Ty::Enum(x), Ty::Enum(y)) => x == y,
        (Ty::Ptr(x), Ty::Ptr(y)) => types_equal(x, y),
        (Ty::Matrix(x, _), Ty::Matrix(y, _)) => types_equal(x, y),
        _ => false,
    }
}

fn is_float_ty(t: &Ty) -> bool {
    matches!(t, Ty::F16 | Ty::F32 | Ty::F64)
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

fn is_numeric_ty(t: &Ty) -> bool {
    is_integer_ty(t) || is_float_ty(t)
}

fn is_c_abi_ty(t: &Ty) -> bool {
    match t {
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
        | Ty::Bool
        | Ty::F32
        | Ty::F64
        | Ty::String
        | Ty::Ptr(_) => true,
        _ => false,
    }
}

fn check_extern_c_abi(f: &FnDef, sig: &FnSig) -> Result<(), String> {
    for ((name, _), ty) in f.params.iter().zip(&sig.params) {
        if !is_c_abi_ty(ty) {
            return Err(format!(
                "extern fn `{}` parameter `{name}` has non-C-ABI type {ty:?}",
                f.name
            ));
        }
    }
    if let Some(ret) = &sig.ret {
        if !is_c_abi_ty(ret) {
            return Err(format!(
                "extern fn `{}` return type is non-C-ABI type {ret:?}",
                f.name
            ));
        }
    }
    Ok(())
}

/// Returns whether type is a primitive printable scalar (`int` or `bool`).
fn is_primitive_ty(t: &Ty) -> bool {
    is_integer_ty(t) || is_float_ty(t) || matches!(t, Ty::Bool)
}

/// Public printable-type predicate used for builtin `println`.
///
/// Includes recursive composites such as arrays/structs/enums of printable fields and pointers.
/// Vector values are typed as `Struct(name)` in the AST; they resolve via `vectors` when absent
/// from `structs`.
fn is_printable_ty(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
) -> bool {
    let mut seen = HashSet::new();
    is_printable_ty_inner(t, structs, enums, vectors, &mut seen)
}

/// Recursive implementation for printable-type checks with cycle protection.
fn is_printable_ty_inner(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    seen: &mut HashSet<String>,
) -> bool {
    match t {
        x if is_primitive_ty(x) => true,
        Ty::String => true,
        Ty::Array(elem, _) => is_printable_ty_inner(elem, structs, enums, vectors, seen),
        Ty::Ptr(_) => true,
        Ty::Matrix(_, _) => true,
        Ty::Vector(_, elem) => is_printable_ty_inner(elem, structs, enums, vectors, seen),
        Ty::AnonVector(elem, _) => is_printable_ty_inner(elem, structs, enums, vectors, seen),
        Ty::HeapVector(elem) => is_printable_ty_inner(elem, structs, enums, vectors, seen),
        Ty::Struct(name) => {
            if !seen.insert(name.clone()) {
                return true;
            }
            let ok = if let Some(def) = structs.get(name) {
                def.fields
                    .iter()
                    .all(|(_, ft)| is_printable_ty_inner(ft, structs, enums, vectors, seen))
            } else if let Some(vdef) = vectors.get(name) {
                is_printable_ty_inner(&vdef.ty, structs, enums, vectors, seen)
            } else {
                false
            };
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
                    .all(|ft| is_printable_ty_inner(ft, structs, enums, vectors, seen)),
                crate::ast::EnumVariantFields::Struct(fs) => fs
                    .iter()
                    .all(|(_, ft)| is_printable_ty_inner(ft, structs, enums, vectors, seen)),
            });
            seen.remove(&key);
            ok
        }
        _ => false,
    }
}

fn expect_arg_ty(
    name: &str,
    args: &[Expr],
    idx: usize,
    expected: &Ty,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<Ty, String> {
    let got = infer_expr(
        &args[idx],
        env,
        structs,
        enums,
        vectors,
        fns,
        Some(expected),
    )?;
    if !types_equal(&got, expected) {
        return Err(format!(
            "`{name}` argument {} type mismatch: expected {expected:?}, got {got:?}",
            idx + 1
        ));
    }
    Ok(got)
}

fn infer_matrix_source(
    expr: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<(Ty, (usize, usize)), String> {
    if let Expr::ArrayLit(rows) = expr {
        if rows.is_empty() {
            return Err("`matrix` expects a non-empty array of rows".into());
        }
        let mut expected_cols: Option<usize> = None;
        let mut expected_ty: Option<Ty> = None;
        for row in rows {
            let Expr::ArrayLit(cells) = row else {
                return Err("`matrix` expects an array of arrays".into());
            };
            if cells.is_empty() {
                return Err("`matrix` rows must not be empty".into());
            }
            if let Some(cols) = expected_cols {
                if cells.len() != cols {
                    return Err(format!(
                        "`matrix` rows must have the same length: expected {cols}, got {}",
                        cells.len()
                    ));
                }
            } else {
                expected_cols = Some(cells.len());
            }
            for cell in cells {
                let ty = infer_expr(cell, env, structs, enums, vectors, fns, None)?;
                if !is_numeric_ty(&ty) {
                    return Err(format!("`matrix` cells must be numeric, got {ty:?}"));
                }
                if let Some(expected) = &expected_ty {
                    if !types_equal(expected, &ty) {
                        return Err(format!(
                            "`matrix` cells must have one type: expected {expected:?}, got {ty:?}"
                        ));
                    }
                } else {
                    expected_ty = Some(ty);
                }
            }
        }
        let elem_ty = expected_ty.ok_or_else(|| "`matrix` rows must not be empty".to_string())?;
        let cols = expected_cols.ok_or_else(|| "`matrix` rows must not be empty".to_string())?;
        return Ok((elem_ty, (rows.len(), cols)));
    }

    let ty = infer_expr(expr, env, structs, enums, vectors, fns, None)?;
    let Ty::Array(row_ty, rows) = ty else {
        return Err(format!("`matrix` expects an array of arrays, got {ty:?}"));
    };
    if rows == 0 {
        return Err("`matrix` expects a non-empty array of rows".into());
    }
    let Ty::Array(cell_ty, cols) = row_ty.as_ref() else {
        return Err(format!(
            "`matrix` expects an array of arrays, got {row_ty:?}"
        ));
    };
    if *cols == 0 {
        return Err("`matrix` rows must not be empty".into());
    }
    if !is_numeric_ty(cell_ty) {
        return Err(format!("`matrix` cells must be numeric, got {cell_ty:?}"));
    }
    Ok((cell_ty.as_ref().clone(), (rows, *cols)))
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
/// - Builtins `println` / `len` / heap helpers are special-cased (`println` → unit, `len` → i32).
/// - Function/struct constructor calls validate arity and per-arg compatibility.
fn infer_arithmetic_bin(
    l: &Expr,
    r: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
    op: &str,
) -> Result<Ty, String> {
    let tl = infer_expr(l, env, structs, enums, vectors, fns, None)?;
    if matches!(tl, Ty::Unit) {
        return Err(format!("void value on the left of `{op}`"));
    }
    if matches!(tl, Ty::Ptr(_)) {
        return Err(format!("cannot use `{op}` on a pointer value"));
    }
    let tr = infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?;
    if matches!(tr, Ty::Unit) {
        return Err(format!("void value on the right of `{op}`"));
    }
    if matches!(tr, Ty::Ptr(_)) {
        return Err(format!("cannot use `{op}` on a pointer value"));
    }
    if !types_equal(&tl, &tr) {
        return Err(format!("`{op}` operands differ: {tl:?} vs {tr:?}"));
    }
    if let (Ty::Matrix(left_elem, left_shape), Ty::Matrix(right_elem, right_shape)) = (&tl, &tr) {
        if op != "+" && op != "-" && op != "*" {
            return Err(format!(
                "cannot use `{op}` on Matrix values (only `+`, `-`, and `*` are supported)"
            ));
        }
        if matches!(left_elem.as_ref(), Ty::Unit) || matches!(right_elem.as_ref(), Ty::Unit) {
            return Err(format!(
                "cannot use `{op}` on Matrix values with unknown element type"
            ));
        }
        if let (Some(left_shape), Some(right_shape)) = (left_shape, right_shape) {
            if left_shape != right_shape {
                return Err(format!(
                    "`{op}` on matrices requires the same shape; got {:?} and {:?}",
                    left_shape, right_shape
                ));
            }
        }
        let shape = (*left_shape).or(*right_shape);
        return Ok(Ty::Matrix(left_elem.clone(), shape));
    }
    if let Ty::HeapVector(elem_ty) = &tl {
        if !is_numeric_ty(elem_ty) {
            return Err(format!(
                "cannot use `{op}` on heap vectors with non-numeric element type {elem_ty:?}"
            ));
        }
        if op == "+" || op == "-" {
            return Ok(tl);
        }
        return Err(format!(
            "cannot use `{op}` on heap vector values (only `+` and `-` are supported)"
        ));
    }
    // Component-wise linear algebra on fixed-size `vector` types (`vector Name Ty [ ... ]`).
    if is_nia_vector_ty(&tl, vectors) {
        let et = nia_vector_elem_ty(&tl, vectors).expect("vector type must exist in map");
        if !is_integer_ty(et) && !is_float_ty(et) {
            return Err(format!(
                "cannot use `{op}` on vectors with non-numeric axis type {et:?}"
            ));
        }
        if op == "+" || op == "-" {
            return Ok(tl);
        }
        return Err(format!(
            "cannot use `{op}` on vector values (only `+` and `-` are supported)"
        ));
    }
    if is_float_ty(&tl) {
        return Ok(tl);
    }
    if !is_integer_ty(&tl) {
        return Err(format!("cannot use `{op}` on non-integer type {tl:?}"));
    }
    if !is_integer_ty(&tr) {
        return Err(format!("cannot use `{op}` on non-integer type {tr:?}"));
    }
    Ok(tl)
}

/// `*` : scalar × scalar; scalar × vector / vector × scalar (axis type `T`); component-wise vector × vector (same type).
fn infer_mul_bin(
    l: &Expr,
    r: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<Ty, String> {
    let tl = infer_expr(l, env, structs, enums, vectors, fns, None)?;
    if matches!(tl, Ty::Unit) {
        return Err("void value on the left of `*`".into());
    }
    if matches!(tl, Ty::Ptr(_)) {
        return Err("cannot use `*` on a pointer value".into());
    }

    if let Ty::Matrix(elem_ty, _) = &tl {
        if matches!(elem_ty.as_ref(), Ty::Unit) {
            return Err("cannot use `*` on Matrix values with unknown element type".into());
        }
        let tr = infer_expr(r, env, structs, enums, vectors, fns, None)?;
        if matches!(tr, Ty::Unit) {
            return Err("void value on the right of `*`".into());
        }
        if matches!(tr, Ty::Ptr(_)) {
            return Err("cannot use `*` on a pointer value".into());
        }
        if matches!(tr, Ty::Matrix(_, _)) {
            if types_equal(&tl, &tr) {
                return Ok(tl);
            }
            return Err(format!(
                "`*` on matrices requires the same element type; got {tl:?} and {tr:?}"
            ));
        }
        if is_numeric_ty(&tr) && types_equal(&tr, elem_ty) {
            return Ok(tl);
        }
        return Err(format!(
            "matrix `*` expects a Matrix with the same element type or scalar {elem_ty:?}, got {tr:?}"
        ));
    }

    if let Ty::HeapVector(elem_ty) = &tl {
        if !is_numeric_ty(elem_ty) {
            return Err(format!(
                "cannot use `*` on heap vectors with non-numeric element type {elem_ty:?}"
            ));
        }
        let tr = match r {
            Expr::AnonVectorLit(_) => infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?,
            _ => infer_expr(r, env, structs, enums, vectors, fns, Some(elem_ty))?,
        };
        if matches!(tr, Ty::Unit) {
            return Err("void value on the right of `*`".into());
        }
        if matches!(tr, Ty::Ptr(_)) {
            return Err("cannot use `*` on a pointer value".into());
        }
        if matches!(tr, Ty::HeapVector(_)) {
            if types_equal(&tl, &tr) {
                return Ok(tl);
            }
            return Err(format!(
                "`*` on heap vectors requires the same element type; got {tl:?} and {tr:?}"
            ));
        }
        if types_equal(&tr, elem_ty) {
            return Ok(tl);
        }
        return Err(format!(
            "heap vector `*` expects scalar of element type {elem_ty:?} or a heap vector of the same element type, got {tr:?}"
        ));
    }

    if is_nia_vector_ty(&tl, vectors) {
        let et = nia_vector_elem_ty(&tl, vectors).expect("vector type must exist in map");
        if !is_integer_ty(et) && !is_float_ty(et) {
            return Err(format!(
                "cannot use `*` on vectors with non-numeric axis type {et:?}"
            ));
        }
        let tr = match r {
            Expr::AnonVectorLit(_) => infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?,
            _ => infer_expr(r, env, structs, enums, vectors, fns, Some(et))?,
        };
        if matches!(tr, Ty::Unit) {
            return Err("void value on the right of `*`".into());
        }
        if matches!(tr, Ty::Ptr(_)) {
            return Err("cannot use `*` on a pointer value".into());
        }
        if is_nia_vector_ty(&tr, vectors) {
            if types_equal(&tl, &tr) {
                return Ok(tl);
            }
            return Err(format!(
                "`*` on vectors requires the same vector type; got {tl:?} and {tr:?}"
            ));
        }
        if types_equal(&tr, et) {
            return Ok(tl);
        }
        return Err(format!(
            "vector `*` expects scalar of axis type {et:?} or a vector of the same type, got {tr:?}"
        ));
    }

    let tr = match r {
        Expr::AnonVectorLit(_) => infer_expr(r, env, structs, enums, vectors, fns, None)?,
        _ => infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?,
    };
    if matches!(tr, Ty::Unit) {
        return Err("void value on the right of `*`".into());
    }
    if matches!(tr, Ty::Ptr(_)) {
        return Err("cannot use `*` on a pointer value".into());
    }

    if let Ty::Matrix(elem_ty, _) = &tr {
        if matches!(elem_ty.as_ref(), Ty::Unit) {
            return Err("cannot use `*` on Matrix values with unknown element type".into());
        }
        if is_numeric_ty(&tl) && types_equal(&tl, elem_ty) {
            return Ok(tr);
        }
        return Err(format!(
            "matrix `*` expects scalar {elem_ty:?} on the left, got {tl:?}"
        ));
    }

    if let Ty::HeapVector(elem_ty) = &tr {
        if !is_numeric_ty(elem_ty) {
            return Err(format!(
                "cannot use `*` on heap vectors with non-numeric element type {elem_ty:?}"
            ));
        }
        let tl = infer_expr(l, env, structs, enums, vectors, fns, Some(elem_ty))?;
        if matches!(tl, Ty::Unit) {
            return Err("void value on the left of `*`".into());
        }
        if matches!(tl, Ty::Ptr(_)) {
            return Err("cannot use `*` on a pointer value".into());
        }
        if matches!(tl, Ty::HeapVector(_)) {
            if types_equal(&tl, &tr) {
                return Ok(tr);
            }
            return Err(format!(
                "`*` on heap vectors requires the same element type; got {tl:?} and {tr:?}"
            ));
        }
        if types_equal(&tl, elem_ty) {
            return Ok(tr);
        }
        return Err(format!(
            "heap vector `*` expects scalar of element type {elem_ty:?} or a heap vector of the same element type, got {tl:?}"
        ));
    }

    if is_nia_vector_ty(&tr, vectors) {
        let et = nia_vector_elem_ty(&tr, vectors).expect("vector type must exist in map");
        if !is_integer_ty(et) && !is_float_ty(et) {
            return Err(format!(
                "cannot use `*` on vectors with non-numeric axis type {et:?}"
            ));
        }
        let tl = infer_expr(l, env, structs, enums, vectors, fns, Some(et))?;
        if matches!(tl, Ty::Unit) {
            return Err("void value on the left of `*`".into());
        }
        if matches!(tl, Ty::Ptr(_)) {
            return Err("cannot use `*` on a pointer value".into());
        }
        if is_nia_vector_ty(&tl, vectors) {
            if types_equal(&tl, &tr) {
                return Ok(tr);
            }
            return Err(format!(
                "`*` on vectors requires the same vector type; got {tl:?} and {tr:?}"
            ));
        }
        if types_equal(&tl, et) {
            return Ok(tr);
        }
        return Err(format!(
            "vector `*` expects scalar of axis type {et:?} or a vector of the same type, got {tl:?}"
        ));
    }

    infer_arithmetic_bin(l, r, env, structs, enums, vectors, fns, "*")
}

/// `u @ v` — dot product for vectors, matrix multiplication for matrices,
/// plus matrix/vector products when a fixed vector result type can be inferred.
fn infer_vec_dot_bin(
    l: &Expr,
    r: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
    hint: Option<&Ty>,
) -> Result<Ty, String> {
    let tl = infer_expr(l, env, structs, enums, vectors, fns, None)?;
    if matches!(tl, Ty::Unit) {
        return Err("void value on the left of `@`".into());
    }
    if matches!(tl, Ty::Ptr(_)) {
        return Err("cannot use `@` on a pointer value".into());
    }
    if let Ty::Matrix(left_elem, left_shape) = &tl {
        if matches!(left_elem.as_ref(), Ty::Unit) {
            return Err("cannot use `@` on Matrix values with unknown element type".into());
        }
        let right_hint = left_shape.map(|(_, cols)| Ty::AnonVector(left_elem.clone(), cols));
        let tr = match (r, right_hint.as_ref()) {
            (Expr::AnonVectorLit(_), Some(hint_ty)) => {
                infer_expr(r, env, structs, enums, vectors, fns, Some(hint_ty))?
            }
            _ => infer_expr(r, env, structs, enums, vectors, fns, None)?,
        };
        if matches!(tr, Ty::Unit) {
            return Err("void value on the right of `@`".into());
        }
        if matches!(tr, Ty::Ptr(_)) {
            return Err("cannot use `@` on a pointer value".into());
        }
        if let Ty::Matrix(right_elem, right_shape) = &tr {
            if matches!(right_elem.as_ref(), Ty::Unit) {
                return Err("cannot use `@` on Matrix values with unknown element type".into());
            }
            if !types_equal(left_elem, right_elem) {
                return Err(format!(
                    "`@` on matrices requires the same element type; got {left_elem:?} and {right_elem:?}"
                ));
            }
            if let (Some((_, left_cols)), Some((right_rows, _))) = (left_shape, right_shape) {
                if left_cols != right_rows {
                    return Err(format!(
                        "`@` matrix multiplication shape mismatch: left columns {left_cols}, right rows {right_rows}"
                    ));
                }
            }
            let shape = match (left_shape, right_shape) {
                (Some((rows, _)), Some((_, cols))) => Some((*rows, *cols)),
                _ => None,
            };
            return Ok(Ty::Matrix(left_elem.clone(), shape));
        }
        if !is_nia_vector_ty(&tr, vectors) {
            return Err(format!(
                "`@` with Matrix on the left requires a Matrix or vector on the right, got {tr:?}"
            ));
        }
        let right_elem = nia_vector_elem_ty(&tr, vectors).expect("checked vector type");
        let right_len = vector_len(&tr, vectors).expect("checked vector type");
        if !types_equal(left_elem, right_elem) {
            return Err(format!(
                "`@` Matrix-vector product requires matching element types; got {left_elem:?} and {right_elem:?}"
            ));
        }
        if let Some((_, cols)) = left_shape {
            if *cols != right_len {
                return Err(format!(
                    "`@` Matrix-vector shape mismatch: matrix columns {cols}, vector length {right_len}"
                ));
            }
        }
        if let Some((hint_elem, hint_len, hint_ty)) = vector_hint_meta(hint, vectors) {
            if !types_equal(left_elem, &hint_elem) {
                return Err(format!(
                    "`@` Matrix-vector result element type mismatch: expected {hint_elem:?}, got {left_elem:?}"
                ));
            }
            if let Some((rows, _)) = left_shape {
                if *rows != hint_len {
                    return Err(format!(
                        "`@` Matrix-vector result length mismatch: matrix rows {rows}, result vector length {hint_len}"
                    ));
                }
            }
            return Ok(hint_ty);
        }
        let Some((rows, _)) = left_shape else {
            return Err(
                "cannot infer result vector length for `Matrix @ vector`; add a result vector annotation"
                    .into(),
            );
        };
        if *rows == right_len && matches!(tr, Ty::Struct(_) | Ty::Vector(_, _)) {
            return Ok(tr);
        }
        return Ok(Ty::AnonVector(left_elem.clone(), *rows));
    }
    if let Ty::HeapVector(elem_ty) = &tl {
        if !is_numeric_ty(elem_ty) {
            return Err(format!(
                "cannot use `@` on heap vectors with non-numeric element type {elem_ty:?}"
            ));
        }
        let tr = match r {
            Expr::AnonVectorLit(_) => infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?,
            _ => infer_expr(r, env, structs, enums, vectors, fns, None)?,
        };
        if matches!(tr, Ty::Unit) {
            return Err("void value on the right of `@`".into());
        }
        if matches!(tr, Ty::Ptr(_)) {
            return Err("cannot use `@` on a pointer value".into());
        }
        if !types_equal(&tl, &tr) {
            return Err(format!(
                "`@` operands must be heap vectors with the same element type; got {tl:?} and {tr:?}"
            ));
        }
        return Ok(elem_ty.as_ref().clone());
    }
    if !is_nia_vector_ty(&tl, vectors) {
        return Err(format!(
            "`@` requires a vector or Matrix on the left, got {tl:?}"
        ));
    }
    let et = nia_vector_elem_ty(&tl, vectors).expect("vector type must exist in map");
    let left_len = vector_len(&tl, vectors).expect("vector type must exist in map");
    if !is_integer_ty(et) && !is_float_ty(et) {
        return Err(format!(
            "cannot use `@` on vectors with non-numeric axis type {et:?}"
        ));
    }
    let tr = infer_expr(r, env, structs, enums, vectors, fns, None)?;
    if matches!(tr, Ty::Unit) {
        return Err("void value on the right of `@`".into());
    }
    if matches!(tr, Ty::Ptr(_)) {
        return Err("cannot use `@` on a pointer value".into());
    }
    if let Ty::Matrix(matrix_elem, matrix_shape) = &tr {
        if matches!(matrix_elem.as_ref(), Ty::Unit) {
            return Err("cannot use `@` on Matrix values with unknown element type".into());
        }
        if !types_equal(et, matrix_elem) {
            return Err(format!(
                "`@` vector-Matrix product requires matching element types; got {et:?} and {matrix_elem:?}"
            ));
        }
        if let Some((rows, _)) = matrix_shape {
            if *rows != left_len {
                return Err(format!(
                    "`@` vector-Matrix shape mismatch: vector length {left_len}, matrix rows {rows}"
                ));
            }
        }
        if let Some((hint_elem, hint_len, hint_ty)) = vector_hint_meta(hint, vectors) {
            if !types_equal(et, &hint_elem) {
                return Err(format!(
                    "`@` vector-Matrix result element type mismatch: expected {hint_elem:?}, got {et:?}"
                ));
            }
            if let Some((_, cols)) = matrix_shape {
                if *cols != hint_len {
                    return Err(format!(
                        "`@` vector-Matrix result length mismatch: matrix columns {cols}, result vector length {hint_len}"
                    ));
                }
            }
            return Ok(hint_ty);
        }
        let Some((_, cols)) = matrix_shape else {
            return Err(
                "cannot infer result vector length for `vector @ Matrix`; add a result vector annotation"
                    .into(),
            );
        };
        if *cols == left_len && matches!(tl, Ty::Struct(_) | Ty::Vector(_, _)) {
            return Ok(tl);
        }
        return Ok(Ty::AnonVector(Box::new(et.clone()), *cols));
    }
    if !types_equal(&tl, &tr) {
        return Err(format!(
            "`@` operands must be the same vector type; got {tl:?} and {tr:?}"
        ));
    }
    Ok(et.clone())
}

/// True if `t` is a user `vector` declaration (surface syntax uses `Struct(name)` for values).
fn is_nia_vector_ty(t: &Ty, vectors: &HashMap<String, VectorDef>) -> bool {
    match t {
        Ty::Struct(n) => vectors.contains_key(n),
        Ty::Vector(n, _) => vectors.contains_key(n),
        Ty::AnonVector(_, _) => true,
        _ => false,
    }
}

fn nia_vector_elem_ty<'a>(t: &'a Ty, vectors: &'a HashMap<String, VectorDef>) -> Option<&'a Ty> {
    match t {
        Ty::Struct(n) => vectors.get(n).map(|v| &v.ty),
        Ty::Vector(_, e) => Some(e.as_ref()),
        Ty::AnonVector(e, _) => Some(e.as_ref()),
        _ => None,
    }
}

fn vector_len(t: &Ty, vectors: &HashMap<String, VectorDef>) -> Option<usize> {
    match t {
        Ty::Struct(n) => vectors.get(n).map(|v| v.fields.len()),
        Ty::Vector(n, _) => vectors.get(n).map(|v| v.fields.len()),
        Ty::AnonVector(_, n) => Some(*n),
        _ => None,
    }
}

fn vector_hint_meta(
    hint: Option<&Ty>,
    vectors: &HashMap<String, VectorDef>,
) -> Option<(Ty, usize, Ty)> {
    let hint = hint?;
    let elem_ty = nia_vector_elem_ty(hint, vectors)?.clone();
    let len = vector_len(hint, vectors)?;
    Some((elem_ty, len, hint.clone()))
}

fn method_receiver_owner_ty(t: &Ty) -> &Ty {
    match t {
        Ty::Ptr(inner) => inner.as_ref(),
        _ => t,
    }
}

fn method_self_accepts_receiver(receiver: &Ty, self_param: &Ty) -> bool {
    types_equal(receiver, self_param)
        || matches!(self_param, Ty::Ptr(inner) if types_equal(receiver, inner))
        || matches!(receiver, Ty::Ptr(inner) if types_equal(inner, self_param))
}

fn infer_comparison_bin(
    l: &Expr,
    r: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
    op: &str,
    order_only: bool,
) -> Result<Ty, String> {
    let tl = infer_expr(l, env, structs, enums, vectors, fns, None)?;
    let tr = infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?;
    if !types_equal(&tl, &tr) {
        return Err(format!("`{op}` operands differ: {tl:?} vs {tr:?}"));
    }
    if order_only {
        if !is_integer_ty(&tl) && !is_float_ty(&tl) {
            return Err(format!(
                "cannot use `{op}` on non-integer/non-float type {tl:?}"
            ));
        }
    } else if !(is_integer_ty(&tl)
        || is_float_ty(&tl)
        || matches!(tl, Ty::Bool | Ty::Ptr(_) | Ty::String))
    {
        return Err(format!(
            "cannot use `{op}` on type {tl:?}; supported: integers, floats, bool, pointers, strings"
        ));
    }
    Ok(Ty::Bool)
}

fn infer_expr(
    e: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
    hint: Option<&Ty>,
) -> Result<Ty, String> {
    match e {
        Expr::Int(_) => match hint {
            None => Ok(Ty::I32),
            Some(other) if is_integer_ty(other) => Ok(other.clone()),
            Some(other) if is_float_ty(other) => Ok(other.clone()),
            Some(Ty::Bool) => Err("integer literal cannot satisfy bool".into()),
            Some(Ty::Struct(name)) => Err(format!(
                "integer literal cannot satisfy struct type `{name}`"
            )),
            Some(Ty::Unit) => Err("integer literal cannot satisfy `()`".into()),
            Some(Ty::Ptr(_)) => Err("integer literal cannot satisfy a pointer type".into()),
            Some(Ty::Array(_, _)) => Err("integer literal cannot satisfy array type".into()),
            Some(Ty::List(_)) => Err("integer literal cannot satisfy list type".into()),
            Some(other) => Err(format!("integer literal cannot satisfy {other:?}")),
        },
        Expr::Float(_) => match hint {
            None => Ok(Ty::F64),
            Some(other) if is_float_ty(other) => Ok(other.clone()),
            Some(Ty::Bool) => Err("float literal cannot satisfy bool".into()),
            Some(Ty::Struct(name)) => {
                Err(format!("float literal cannot satisfy struct type `{name}`"))
            }
            Some(Ty::Unit) => Err("float literal cannot satisfy `()`".into()),
            Some(Ty::Ptr(_)) => Err("float literal cannot satisfy a pointer type".into()),
            Some(Ty::Array(_, _)) => Err("float literal cannot satisfy array type".into()),
            Some(Ty::List(_)) => Err("float literal cannot satisfy list type".into()),
            Some(other) if is_integer_ty(other) => Err(format!(
                "float literal cannot satisfy integer type {other:?}"
            )),
            Some(other) => Err(format!("float literal cannot satisfy {other:?}")),
        },
        Expr::Bool(_) => match hint {
            Some(Ty::Bool) | None => Ok(Ty::Bool),
            Some(other) => Err(format!("bool literal cannot satisfy {other:?}")),
        },
        Expr::String(_) => match hint {
            Some(Ty::String) | None => Ok(Ty::String),
            Some(other) => Err(format!("string literal cannot satisfy {other:?}")),
        },
        Expr::Ident(name) => env
            .get(name)
            .cloned()
            .or_else(|| (name == PI).then_some(Ty::F64))
            .ok_or_else(|| format!("unknown variable `{name}`")),
        Expr::Neg(inner) => {
            let t = infer_expr(inner, env, structs, enums, vectors, fns, None)?;
            if matches!(t, Ty::Unit) {
                return Err("void value in unary `-`".into());
            }
            if matches!(t, Ty::Ptr(_)) {
                return Err("cannot negate a pointer value".into());
            }
            if !is_integer_ty(&t) && !is_float_ty(&t) {
                return Err(format!("cannot negate non-numeric type {t:?}"));
            }
            Ok(t)
        }
        Expr::Add(l, r) => infer_arithmetic_bin(l, r, env, structs, enums, vectors, fns, "+"),
        Expr::Sub(l, r) => infer_arithmetic_bin(l, r, env, structs, enums, vectors, fns, "-"),
        Expr::Mul(l, r) => infer_mul_bin(l, r, env, structs, enums, vectors, fns),
        Expr::VecDot(l, r) => infer_vec_dot_bin(l, r, env, structs, enums, vectors, fns, hint),
        Expr::Div(l, r) => {
            if matches!(r.as_ref(), Expr::Int(0)) {
                let tl = infer_expr(l, env, structs, enums, vectors, fns, None)?;
                if is_integer_ty(&tl) {
                    return Err("division by zero".into());
                }
            }
            infer_arithmetic_bin(l, r, env, structs, enums, vectors, fns, "/")
        }
        Expr::Eq(l, r) => {
            infer_comparison_bin(l, r, env, structs, enums, vectors, fns, "==", false)
        }
        Expr::Ne(l, r) => {
            infer_comparison_bin(l, r, env, structs, enums, vectors, fns, "!=", false)
        }
        Expr::Lt(l, r) => infer_comparison_bin(l, r, env, structs, enums, vectors, fns, "<", true),
        Expr::Le(l, r) => infer_comparison_bin(l, r, env, structs, enums, vectors, fns, "<=", true),
        Expr::Gt(l, r) => infer_comparison_bin(l, r, env, structs, enums, vectors, fns, ">", true),
        Expr::Ge(l, r) => infer_comparison_bin(l, r, env, structs, enums, vectors, fns, ">=", true),
        Expr::Call { name, args } => {
            if name == QUBIT {
                if args.len() != 0 {
                    return Err(format!(
                        "`{QUBIT}` expects exactly 0 arguments, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!("`{QUBIT}()` is only allowed inside `quant` blocks"));
                }
                return Ok(Ty::Qubit);
            }
            if is_single_qubit_gate(name) {
                if args.len() != 1 {
                    return Err(format!(
                        "`{name}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{name}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                let t = infer_expr(
                    &args[0],
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    Some(&Ty::Qubit),
                )?;
                if !types_equal(&t, &Ty::Qubit) {
                    return Err(format!("`{name}` expects a qubit argument, got {t:?}"));
                }
                return Ok(Ty::Unit);
            }
            if is_two_qubit_gate(name) {
                if args.len() != 2 {
                    return Err(format!(
                        "`{name}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{name}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                for (idx, arg) in args.iter().enumerate() {
                    let t = infer_expr(arg, env, structs, enums, vectors, fns, Some(&Ty::Qubit))?;
                    if !types_equal(&t, &Ty::Qubit) {
                        return Err(format!(
                            "`{name}` argument {} expects a qubit, got {t:?}",
                            idx + 1
                        ));
                    }
                }
                return Ok(Ty::Unit);
            }
            if is_three_qubit_gate(name) {
                if args.len() != 3 {
                    return Err(format!(
                        "`{name}` expects exactly 3 arguments, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{name}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                for (idx, arg) in args.iter().enumerate() {
                    let t = infer_expr(arg, env, structs, enums, vectors, fns, Some(&Ty::Qubit))?;
                    if !types_equal(&t, &Ty::Qubit) {
                        return Err(format!(
                            "`{name}` argument {} expects a qubit, got {t:?}",
                            idx + 1
                        ));
                    }
                }
                return Ok(Ty::Unit);
            }
            if is_rotation_gate(name) {
                if args.len() != 2 {
                    return Err(format!(
                        "`{name}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{name}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                let theta =
                    infer_expr(&args[0], env, structs, enums, vectors, fns, Some(&Ty::F64))?;
                if !types_equal(&theta, &Ty::F64) {
                    return Err(format!(
                        "`{name}` argument 1 expects an f64 angle, got {theta:?}"
                    ));
                }
                let qubit = infer_expr(
                    &args[1],
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    Some(&Ty::Qubit),
                )?;
                if !types_equal(&qubit, &Ty::Qubit) {
                    return Err(format!(
                        "`{name}` argument 2 expects a qubit, got {qubit:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if is_controlled_rotation_gate(name) {
                if args.len() != 3 {
                    return Err(format!(
                        "`{name}` expects exactly 3 arguments, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{name}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                let theta =
                    infer_expr(&args[0], env, structs, enums, vectors, fns, Some(&Ty::F64))?;
                if !types_equal(&theta, &Ty::F64) {
                    return Err(format!(
                        "`{name}` argument 1 expects an f64 angle, got {theta:?}"
                    ));
                }
                for (idx, arg) in args.iter().enumerate().skip(1) {
                    let t = infer_expr(arg, env, structs, enums, vectors, fns, Some(&Ty::Qubit))?;
                    if !types_equal(&t, &Ty::Qubit) {
                        return Err(format!(
                            "`{name}` argument {} expects a qubit, got {t:?}",
                            idx + 1
                        ));
                    }
                }
                return Ok(Ty::Unit);
            }
            if name == MEASURE {
                if args.len() != 1 {
                    return Err(format!(
                        "`{MEASURE}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{MEASURE}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                let t = infer_expr(
                    &args[0],
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    Some(&Ty::Qubit),
                )?;
                if !types_equal(&t, &Ty::Qubit) {
                    return Err(format!("`{MEASURE}` expects a qubit argument, got {t:?}"));
                }
                return Ok(Ty::Result);
            }
            if name == RECORD {
                if args.len() != 1 {
                    return Err(format!(
                        "`{RECORD}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{RECORD}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                let t = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !types_equal(&t, &Ty::Result) && !types_equal(&t, &Ty::Bool) {
                    return Err(format!(
                        "`{RECORD}` expects a result or bool argument, got {t:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if name == READ {
                if args.len() != 1 {
                    return Err(format!(
                        "`{READ}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                if !is_in_quant_scope(env) {
                    return Err(format!(
                        "`{READ}(...)` is only allowed inside `quant` blocks"
                    ));
                }
                let t = infer_expr(
                    &args[0],
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    Some(&Ty::Result),
                )?;
                if !types_equal(&t, &Ty::Result) {
                    return Err(format!("`{READ}` expects a result argument, got {t:?}"));
                }
                return Ok(Ty::Bool);
            }
            if name == PRINTLN {
                if args.len() != 1 {
                    return Err(format!(
                        "`{PRINTLN}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let t = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !is_printable_ty(&t, structs, enums, vectors) {
                    return Err(format!("`{PRINTLN}` expects printable type, got {t:?}"));
                }
                return Ok(Ty::Unit);
            }
            if name == LEN {
                if args.len() != 1 {
                    return Err(format!(
                        "`{LEN}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let t = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                return match t {
                    Ty::Array(_, _) | Ty::HeapVector(_) => Ok(Ty::I32),
                    _ => Err(format!(
                        "`{LEN}` expects an array or heap vector, got {t:?}"
                    )),
                };
            }
            if name == SIN || name == COS {
                if args.len() != 1 {
                    return Err(format!(
                        "`{name}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                expect_arg_ty(name, args, 0, &Ty::F64, env, structs, enums, vectors, fns)?;
                return Ok(Ty::F64);
            }
            if name == COMPLEX_NEW {
                if args.len() != 2 {
                    return Err(format!(
                        "`{COMPLEX_NEW}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                expect_arg_ty(name, args, 0, &Ty::F64, env, structs, enums, vectors, fns)?;
                expect_arg_ty(name, args, 1, &Ty::F64, env, structs, enums, vectors, fns)?;
                return Ok(crate::nia_std::complex_ty());
            }
            if name == COMPLEX_ADD
                || name == COMPLEX_SUB
                || name == COMPLEX_MUL
                || name == COMPLEX_DIV
            {
                if args.len() != 2 {
                    return Err(format!(
                        "`{name}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                let complex_ty = crate::nia_std::complex_ty();
                expect_arg_ty(
                    name,
                    args,
                    0,
                    &complex_ty,
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                )?;
                expect_arg_ty(
                    name,
                    args,
                    1,
                    &complex_ty,
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                )?;
                return Ok(complex_ty);
            }
            if name == COMPLEX_SCALE {
                if args.len() != 2 {
                    return Err(format!(
                        "`{COMPLEX_SCALE}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                let complex_ty = crate::nia_std::complex_ty();
                expect_arg_ty(
                    name,
                    args,
                    0,
                    &complex_ty,
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                )?;
                expect_arg_ty(name, args, 1, &Ty::F64, env, structs, enums, vectors, fns)?;
                return Ok(complex_ty);
            }
            if name == CIS {
                if args.len() != 1 {
                    return Err(format!(
                        "`{CIS}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                expect_arg_ty(name, args, 0, &Ty::F64, env, structs, enums, vectors, fns)?;
                return Ok(crate::nia_std::complex_ty());
            }
            if name == LIST_NEW || name == LIST_WITH_CAPACITY {
                return Err(format!(
                    "`{name}` requires a type argument, e.g. `{name}[u8]()`"
                ));
            }
            if name == ALLOC {
                if args.len() != 1 {
                    return Err(format!(
                        "`{ALLOC}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let t = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
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
                let t = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
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
                let pt = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                let Ty::Ptr(pointee) = pt else {
                    return Err(format!(
                        "`{REALLOC}` first argument must be pointer, got {pt:?}"
                    ));
                };
                let vt = infer_expr(&args[1], env, structs, enums, vectors, fns, Some(&pointee))?;
                if !types_equal(&vt, &pointee) {
                    return Err(format!(
                        "`{REALLOC}` value type mismatch: expected {pointee:?}, got {vt:?}"
                    ));
                }
                return Ok(Ty::Ptr(pointee));
            }
            if name == MATRIX_NEW {
                if args.len() != 1 {
                    return Err(format!(
                        "`{MATRIX_NEW}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let (elem_ty, shape) =
                    infer_matrix_source(&args[0], env, structs, enums, vectors, fns)?;
                return Ok(Ty::Matrix(Box::new(elem_ty), Some(shape)));
            }
            if name == MATRIX_GET {
                if args.len() != 3 {
                    return Err(format!(
                        "`{MATRIX_GET}` expects exactly 3 arguments, got {}",
                        args.len()
                    ));
                }
                let matrix_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                let Ty::Matrix(elem_ty, _) = matrix_ty else {
                    return Err(format!(
                        "`{MATRIX_GET}` argument 1 type mismatch: expected Matrix, got {matrix_ty:?}"
                    ));
                };
                expect_arg_ty(name, args, 1, &Ty::I32, env, structs, enums, vectors, fns)?;
                expect_arg_ty(name, args, 2, &Ty::I32, env, structs, enums, vectors, fns)?;
                if matches!(elem_ty.as_ref(), Ty::Unit) {
                    return Err(format!(
                        "`{MATRIX_GET}` needs a Matrix with a known element type"
                    ));
                }
                return Ok((*elem_ty).clone());
            }
            if name == MATRIX_SET {
                if args.len() != 4 {
                    return Err(format!(
                        "`{MATRIX_SET}` expects exactly 4 arguments, got {}",
                        args.len()
                    ));
                }
                let matrix_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                let Ty::Matrix(elem_ty, _) = matrix_ty else {
                    return Err(format!(
                        "`{MATRIX_SET}` argument 1 type mismatch: expected Matrix, got {matrix_ty:?}"
                    ));
                };
                expect_arg_ty(name, args, 1, &Ty::I32, env, structs, enums, vectors, fns)?;
                expect_arg_ty(name, args, 2, &Ty::I32, env, structs, enums, vectors, fns)?;
                if matches!(elem_ty.as_ref(), Ty::Unit) {
                    return Err(format!(
                        "`{MATRIX_SET}` needs a Matrix with a known element type"
                    ));
                }
                let value_ty =
                    infer_expr(&args[3], env, structs, enums, vectors, fns, Some(&elem_ty))?;
                if !is_numeric_ty(&value_ty) {
                    return Err(format!(
                        "`{MATRIX_SET}` value must be numeric, got {value_ty:?}"
                    ));
                }
                if !types_equal(&value_ty, &elem_ty) {
                    return Err(format!(
                        "`{MATRIX_SET}` value type mismatch: expected {elem_ty:?}, got {value_ty:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if name == MATRIX_ROWS
                || name == MATRIX_COLS
                || name == MATRIX_LEN
                || name == MATRIX_REFCOUNT
            {
                if args.len() != 1 {
                    return Err(format!(
                        "`{name}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let matrix_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !matches!(matrix_ty, Ty::Matrix(_, _)) {
                    return Err(format!(
                        "`{name}` argument 1 type mismatch: expected Matrix, got {matrix_ty:?}"
                    ));
                }
                return Ok(Ty::I32);
            }
            if name == MATRIX_CLONE {
                if args.len() != 1 {
                    return Err(format!(
                        "`{MATRIX_CLONE}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let matrix_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !matches!(matrix_ty, Ty::Matrix(_, _)) {
                    return Err(format!(
                        "`{MATRIX_CLONE}` argument 1 type mismatch: expected Matrix, got {matrix_ty:?}"
                    ));
                }
                return Ok(matrix_ty);
            }
            if name == MATRIX_DROP {
                if args.len() != 1 {
                    return Err(format!(
                        "`{MATRIX_DROP}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let matrix_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !matches!(matrix_ty, Ty::Matrix(_, _)) {
                    return Err(format!(
                        "`{MATRIX_DROP}` argument 1 type mismatch: expected Matrix, got {matrix_ty:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if name == VECTOR_GET {
                if args.len() != 2 {
                    return Err(format!(
                        "`{VECTOR_GET}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                let vector_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                let Ty::HeapVector(elem_ty) = vector_ty else {
                    return Err(format!(
                        "`{VECTOR_GET}` argument 1 type mismatch: expected heap vector, got {vector_ty:?}"
                    ));
                };
                expect_arg_ty(name, args, 1, &Ty::I32, env, structs, enums, vectors, fns)?;
                return Ok((*elem_ty).clone());
            }
            if name == VECTOR_SET {
                if args.len() != 3 {
                    return Err(format!(
                        "`{VECTOR_SET}` expects exactly 3 arguments, got {}",
                        args.len()
                    ));
                }
                let vector_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                let Ty::HeapVector(elem_ty) = vector_ty else {
                    return Err(format!(
                        "`{VECTOR_SET}` argument 1 type mismatch: expected heap vector, got {vector_ty:?}"
                    ));
                };
                expect_arg_ty(name, args, 1, &Ty::I32, env, structs, enums, vectors, fns)?;
                let value_ty =
                    infer_expr(&args[2], env, structs, enums, vectors, fns, Some(&elem_ty))?;
                if !types_equal(&value_ty, &elem_ty) {
                    return Err(format!(
                        "`{VECTOR_SET}` value type mismatch: expected {elem_ty:?}, got {value_ty:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if name == VECTOR_LEN || name == VECTOR_REFCOUNT {
                if args.len() != 1 {
                    return Err(format!(
                        "`{name}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let vector_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !matches!(vector_ty, Ty::HeapVector(_)) {
                    return Err(format!(
                        "`{name}` argument 1 type mismatch: expected heap vector, got {vector_ty:?}"
                    ));
                }
                return Ok(Ty::I32);
            }
            if name == VECTOR_CLONE {
                if args.len() != 1 {
                    return Err(format!(
                        "`{VECTOR_CLONE}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let vector_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !matches!(vector_ty, Ty::HeapVector(_)) {
                    return Err(format!(
                        "`{VECTOR_CLONE}` argument 1 type mismatch: expected heap vector, got {vector_ty:?}"
                    ));
                }
                return Ok(vector_ty);
            }
            if name == VECTOR_DROP {
                if args.len() != 1 {
                    return Err(format!(
                        "`{VECTOR_DROP}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                let vector_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !matches!(vector_ty, Ty::HeapVector(_)) {
                    return Err(format!(
                        "`{VECTOR_DROP}` argument 1 type mismatch: expected heap vector, got {vector_ty:?}"
                    ));
                }
                return Ok(Ty::Unit);
            }
            if name == OUTER {
                if args.len() != 2 {
                    return Err(format!(
                        "`{OUTER}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                let left_ty = infer_expr(&args[0], env, structs, enums, vectors, fns, None)?;
                if !is_nia_vector_ty(&left_ty, vectors) {
                    return Err(format!(
                        "`{OUTER}` argument 1 type mismatch: expected vector, got {left_ty:?}"
                    ));
                }
                let left_elem = nia_vector_elem_ty(&left_ty, vectors)
                    .expect("checked vector type")
                    .clone();
                if !is_numeric_ty(&left_elem) {
                    return Err(format!(
                        "`{OUTER}` vector elements must be numeric, got {left_elem:?}"
                    ));
                }

                let right_ty = infer_expr(&args[1], env, structs, enums, vectors, fns, None)?;
                if !is_nia_vector_ty(&right_ty, vectors) {
                    return Err(format!(
                        "`{OUTER}` argument 2 type mismatch: expected vector, got {right_ty:?}"
                    ));
                }
                let right_elem = nia_vector_elem_ty(&right_ty, vectors)
                    .expect("checked vector type")
                    .clone();
                if !is_numeric_ty(&right_elem) {
                    return Err(format!(
                        "`{OUTER}` vector elements must be numeric, got {right_elem:?}"
                    ));
                }
                if !types_equal(&left_elem, &right_elem) {
                    return Err(format!(
                        "`{OUTER}` vector element types must match exactly; got {left_elem:?} and {right_elem:?}"
                    ));
                }
                let rows = vector_len(&left_ty, vectors).expect("checked vector type");
                let cols = vector_len(&right_ty, vectors).expect("checked vector type");
                return Ok(Ty::Matrix(Box::new(left_elem), Some((rows, cols))));
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
                    let at = infer_expr(a, env, structs, enums, vectors, fns, Some(ft))?;
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
            if sig.is_quantum && !is_in_quant_scope(env) {
                return Err(format!(
                    "quantum function `{name}` can only be called inside `quant` blocks"
                ));
            }
            if args.len() != sig.params.len() {
                return Err(format!(
                    "call `{name}`: expected {} args, got {}",
                    sig.params.len(),
                    args.len()
                ));
            }
            for (a, pt) in args.iter().zip(&sig.params) {
                let at = infer_expr(a, env, structs, enums, vectors, fns, Some(pt))?;
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
        Expr::GenericCall {
            name,
            ty_args,
            args,
        } => {
            if name != LIST_NEW && name != LIST_WITH_CAPACITY {
                return Err(format!(
                    "generic calls are only supported for `{LIST_NEW}` and `{LIST_WITH_CAPACITY}`"
                ));
            }
            if ty_args.len() != 1 {
                return Err(format!("`{name}` expects exactly 1 type argument"));
            }
            let elem_ty = normalize_ty(&ty_args[0], structs, enums, vectors)?;
            if matches!(elem_ty, Ty::Unit) {
                return Err(format!("`{name}` cannot create `List[()]`"));
            }
            if name == LIST_NEW {
                if !args.is_empty() {
                    return Err(format!(
                        "`{LIST_NEW}` expects exactly 0 arguments, got {}",
                        args.len()
                    ));
                }
            } else {
                if args.len() != 1 {
                    return Err(format!(
                        "`{LIST_WITH_CAPACITY}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                expect_arg_ty(name, args, 0, &Ty::I32, env, structs, enums, vectors, fns)?;
            }
            Ok(Ty::List(Box::new(elem_ty)))
        }
        Expr::MethodCall {
            receiver,
            name,
            args,
        } => {
            if name == TO_MATRIX {
                if !args.is_empty() {
                    return Err(format!(
                        "method `{TO_MATRIX}`: expected 0 args, got {}",
                        args.len()
                    ));
                }
                let recv_ty = infer_expr(receiver, env, structs, enums, vectors, fns, None)?;
                return match recv_ty {
                    Ty::Array(row_ty, rows) => {
                        if rows == 0 {
                            return Err(format!(
                                "method `{TO_MATRIX}` expects a non-empty array of rows"
                            ));
                        }
                        let Ty::Array(cell_ty, cols) = row_ty.as_ref() else {
                            return Err(format!(
                                "method `{TO_MATRIX}` expects an array of arrays, got {row_ty:?}"
                            ));
                        };
                        if *cols == 0 {
                            return Err(format!("method `{TO_MATRIX}` rows must not be empty"));
                        }
                        if !is_numeric_ty(cell_ty) {
                            return Err(format!(
                                "method `{TO_MATRIX}` cells must be numeric, got {cell_ty:?}"
                            ));
                        }
                        Ok(Ty::Matrix(cell_ty.clone(), Some((rows, *cols))))
                    }
                    other => Err(format!("unknown method `{TO_MATRIX}` for type {other:?}")),
                };
            }
            if name == TO_ARRAY {
                if !args.is_empty() {
                    return Err(format!(
                        "method `{TO_ARRAY}`: expected 0 args, got {}",
                        args.len()
                    ));
                }
                let receiver_hint = match hint {
                    Some(Ty::Array(row_ty, rows)) if matches!(row_ty.as_ref(), Ty::Array(_, _)) => {
                        let Ty::Array(cell_ty, cols) = row_ty.as_ref() else {
                            unreachable!("guarded above")
                        };
                        Some(Ty::Matrix(cell_ty.clone(), Some((*rows, *cols))))
                    }
                    Some(Ty::Array(elem_ty, n)) => Some(Ty::AnonVector(elem_ty.clone(), *n)),
                    _ => None,
                };
                let recv_ty = infer_expr(
                    receiver,
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    receiver_hint.as_ref(),
                )?;
                return match recv_ty {
                    Ty::Matrix(elem_ty, Some((rows, cols))) if is_numeric_ty(&elem_ty) => {
                        Ok(Ty::Array(Box::new(Ty::Array(elem_ty, cols)), rows))
                    }
                    Ty::Matrix(elem_ty, Some(_)) => Err(format!(
                        "method `{TO_ARRAY}` matrix cells must be numeric, got {elem_ty:?}"
                    )),
                    Ty::Matrix(_, None) => Err(format!(
                        "method `{TO_ARRAY}` needs a Matrix with a known shape"
                    )),
                    Ty::AnonVector(elem_ty, n) if is_numeric_ty(&elem_ty) => {
                        Ok(Ty::Array(elem_ty, n))
                    }
                    Ty::AnonVector(elem_ty, _) => Err(format!(
                        "method `{TO_ARRAY}` vector elements must be numeric, got {elem_ty:?}"
                    )),
                    Ty::HeapVector(_) => Err(format!(
                        "method `{TO_ARRAY}` is only supported for fixed-size anonymous vectors"
                    )),
                    other => Err(format!("unknown method `{TO_ARRAY}` for type {other:?}")),
                };
            }
            if name == TO_VEC {
                if !args.is_empty() {
                    return Err(format!(
                        "method `{TO_VEC}`: expected 0 args, got {}",
                        args.len()
                    ));
                }
                let receiver_hint = match hint {
                    Some(Ty::AnonVector(elem_ty, n)) => Some(Ty::Array(elem_ty.clone(), *n)),
                    _ => None,
                };
                let recv_ty = infer_expr(
                    receiver,
                    env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    receiver_hint.as_ref(),
                )?;
                return match recv_ty {
                    Ty::Array(elem_ty, n) if is_numeric_ty(&elem_ty) => {
                        Ok(Ty::AnonVector(elem_ty, n))
                    }
                    Ty::Array(elem_ty, _) => Err(format!(
                        "method `{TO_VEC}` array elements must be numeric, got {elem_ty:?}"
                    )),
                    other => Err(format!("unknown method `{TO_VEC}` for type {other:?}")),
                };
            }
            let recv_ty = infer_expr(receiver, env, structs, enums, vectors, fns, None)?;
            if let Ty::List(elem_ty) = &recv_ty {
                let elem_ty = elem_ty.as_ref();
                if name == LIST_LEN || name == LIST_CAPACITY {
                    if !args.is_empty() {
                        return Err(format!(
                            "method `{name}`: expected 0 args, got {}",
                            args.len()
                        ));
                    }
                    return Ok(Ty::I32);
                }
                if name == LIST_PUSH {
                    if args.len() != 1 {
                        return Err(format!(
                            "method `{LIST_PUSH}`: expected 1 arg, got {}",
                            args.len()
                        ));
                    }
                    let value_ty =
                        infer_expr(&args[0], env, structs, enums, vectors, fns, Some(elem_ty))?;
                    if !types_equal(&value_ty, elem_ty) {
                        return Err(format!(
                            "method `{LIST_PUSH}` arg type mismatch: expected {elem_ty:?}, got {value_ty:?}"
                        ));
                    }
                    return Ok(Ty::Unit);
                }
                if name == LIST_GET {
                    if args.len() != 1 {
                        return Err(format!(
                            "method `{LIST_GET}`: expected 1 arg, got {}",
                            args.len()
                        ));
                    }
                    expect_arg_ty(
                        LIST_GET,
                        args,
                        0,
                        &Ty::I32,
                        env,
                        structs,
                        enums,
                        vectors,
                        fns,
                    )?;
                    return Ok(elem_ty.clone());
                }
            }
            let owner_ty = method_receiver_owner_ty(&recv_ty);
            if name == "det" {
                if let Ty::Matrix(elem_ty, _) = owner_ty {
                    if !args.is_empty() {
                        return Err(format!("method `det`: expected 0 args, got {}", args.len()));
                    }
                    if matches!(elem_ty.as_ref(), Ty::Unit) {
                        return Err("method `det` needs a Matrix with a known element type".into());
                    }
                    if !is_numeric_ty(elem_ty) {
                        return Err(format!(
                            "method `det` matrix cells must be numeric, got {elem_ty:?}"
                        ));
                    }
                    return Ok(elem_ty.as_ref().clone());
                }
            }
            let symbol = method_symbol(owner_ty, name);
            let sig = fns
                .get(&symbol)
                .ok_or_else(|| format!("unknown method `{name}` for type {owner_ty:?}"))?;
            if sig.params.is_empty() {
                return Err(format!(
                    "method `{name}` for type {owner_ty:?} has no `self` parameter"
                ));
            }
            if !method_self_accepts_receiver(&recv_ty, &sig.params[0]) {
                return Err(format!(
                    "method `{name}` self type mismatch: expected {:?}, got {recv_ty:?}",
                    sig.params[0]
                ));
            }
            if args.len() + 1 != sig.params.len() {
                return Err(format!(
                    "method `{name}`: expected {} args, got {}",
                    sig.params.len() - 1,
                    args.len()
                ));
            }
            for (a, pt) in args.iter().zip(sig.params.iter().skip(1)) {
                let at = infer_expr(a, env, structs, enums, vectors, fns, Some(pt))?;
                if !types_equal(&at, pt) {
                    return Err(format!(
                        "method `{name}`: arg type mismatch: expected {pt:?}, got {at:?}"
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
                return Err(format!("enum `{enum_name}` has no variant `{variant}`"));
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
                return Err(format!("enum `{enum_name}` has no variant `{variant}`"));
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
                let at = infer_expr(a, env, structs, enums, vectors, fns, Some(t))?;
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
                return Err(format!("enum `{enum_name}` has no variant `{variant}`"));
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
                let et = infer_expr(fe, env, structs, enums, vectors, fns, Some(fty))?;
                if !types_equal(&et, fty) {
                    return Err(format!(
                        "enum variant `{enum_name}::{variant}` field `{fname}` mismatch: expected {fty:?}, got {et:?}"
                    ));
                }
            }
            Ok(Ty::Enum(enum_name.clone()))
        }
        Expr::Match { scrutinee, arms } => {
            let st = infer_expr(scrutinee, env, structs, enums, vectors, fns, None)?;
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
                    MatchPattern::Unit { enum_name, variant } => (enum_name, variant, None),
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
                        ));
                    }
                    (EnumVariantFields::Tuple(_), _) => {
                        return Err(format!(
                            "tuple variant `{enum_name}::{pat_variant}` requires tuple pattern"
                        ));
                    }
                    (EnumVariantFields::Struct(_), _) => {
                        return Err(format!(
                            "struct variant `{enum_name}::{pat_variant}` requires struct pattern"
                        ));
                    }
                }
                let at = infer_expr(arm_expr, &arm_env, structs, enums, vectors, fns, hint)?;
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
        Expr::Quant { body } => {
            if block_contains_return(body) {
                return Err("`return` is not allowed inside `quant` expressions".into());
            }
            if block_has_break(body) {
                return Err("`break` is not allowed inside `quant` expressions".into());
            }
            let mut body_env = enter_quant_scope(env);
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    None,
                    0,
                    false,
                )?;
            }
            if let Some(tail) = &body.tail {
                let tail_ty = infer_expr(tail, &body_env, structs, enums, vectors, fns, hint)?;
                if contains_quantum_ty(&tail_ty) {
                    return Err(format!(
                        "`quant` expressions cannot return quantum type `{QUBIT}`"
                    ));
                }
                Ok(tail_ty)
            } else {
                Ok(Ty::Unit)
            }
        }
        Expr::Gpu { body } => {
            if block_contains_return(body) {
                return Err("`return` is not allowed inside `gpu` expressions".into());
            }
            if block_has_break(body) {
                return Err("`break` is not allowed inside `gpu` expressions".into());
            }
            let mut body_env = env.clone();
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    structs,
                    enums,
                    vectors,
                    fns,
                    None,
                    0,
                    false,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(tail, &body_env, structs, enums, vectors, fns, hint)
            } else {
                Ok(Ty::Unit)
            }
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
                let ft = infer_expr(fe, env, structs, enums, vectors, fns, Some(dty))?;
                if !types_equal(dty, &ft) {
                    return Err(format!(
                        "struct `{name}` field `{dfn}`: expected {dty:?}, got {ft:?}"
                    ));
                }
            }
            Ok(Ty::Struct(name.clone()))
        }
        Expr::VectorLit { name, fields } => {
            let def = vectors
                .get(name)
                .ok_or_else(|| format!("unknown vector `{name}`"))?;
            let def_fields = &def.fields;
            for (fname, _) in fields {
                if !def_fields.iter().any(|n| n == fname) {
                    return Err(format!("vector `{name}` has no field `{fname}`"));
                }
            }
            if fields.len() != def_fields.len() {
                return Err(format!(
                    "vector `{name}` literal: expected {} fields, got {}",
                    def_fields.len(),
                    fields.len()
                ));
            }
            for dfn in def_fields {
                let Some((_, fe)) = fields.iter().find(|(n, _)| n == dfn) else {
                    return Err(format!("vector `{name}` missing field `{dfn}`"));
                };
                let ty = &def.ty;
                let ft = infer_expr(fe, env, structs, enums, vectors, fns, Some(ty))?;
                if !types_equal(&def.ty, &ft) {
                    return Err(format!(
                        "vector `{name}` field `{dfn}`: expected {ty:?}, got {ft:?}"
                    ));
                }
            }
            Ok(Ty::Struct(name.clone()))
        }
        Expr::AnonVectorLit(elems) => {
            if elems.is_empty() {
                return Err("anonymous vector literal must not be empty".into());
            }
            match hint {
                Some(Ty::AnonVector(elem_ty, n)) => {
                    if elems.len() != *n {
                        return Err(format!(
                            "anonymous vector literal length mismatch: expected {n}, got {}",
                            elems.len()
                        ));
                    }
                    for e in elems {
                        let et = infer_expr(e, env, structs, enums, vectors, fns, Some(elem_ty))?;
                        if !types_equal(&et, elem_ty) {
                            return Err(format!(
                                "anonymous vector element type mismatch: expected {elem_ty:?}, got {et:?}"
                            ));
                        }
                    }
                    Ok(Ty::AnonVector(elem_ty.clone(), *n))
                }
                Some(Ty::HeapVector(elem_ty)) => {
                    for e in elems {
                        let et = infer_expr(e, env, structs, enums, vectors, fns, Some(elem_ty))?;
                        if !types_equal(&et, elem_ty) {
                            return Err(format!(
                                "heap vector element type mismatch: expected {elem_ty:?}, got {et:?}"
                            ));
                        }
                    }
                    Ok(Ty::HeapVector(elem_ty.clone()))
                }
                Some(other) => Err(format!("anonymous vector literal cannot satisfy {other:?}")),
                None => {
                    let first_ty = infer_expr(&elems[0], env, structs, enums, vectors, fns, None)?;
                    if matches!(first_ty, Ty::Unit) {
                        return Err("anonymous vector elements cannot be void".into());
                    }
                    for e in elems.iter().skip(1) {
                        let et = infer_expr(e, env, structs, enums, vectors, fns, Some(&first_ty))?;
                        if !types_equal(&et, &first_ty) {
                            return Err(format!(
                                "anonymous vector elements differ: expected {first_ty:?}, got {et:?}"
                            ));
                        }
                    }
                    Ok(Ty::AnonVector(Box::new(first_ty), elems.len()))
                }
            }
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
                    let et = infer_expr(e, env, structs, enums, vectors, fns, Some(elem_ty))?;
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
                let first_ty = infer_expr(first, env, structs, enums, vectors, fns, None)?;
                for e in elems.iter().skip(1) {
                    let et = infer_expr(e, env, structs, enums, vectors, fns, Some(&first_ty))?;
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
            let bt = infer_expr(obj, env, structs, enums, vectors, fns, None)?;
            let sname = match &bt {
                Ty::Struct(sname) | Ty::Vector(sname, _) => sname,
                Ty::Ptr(inner) => match inner.as_ref() {
                    Ty::Struct(sname) | Ty::Vector(sname, _) => sname,
                    _ => return Err("field access on non-struct".into()),
                },
                _ => return Err("field access on non-struct".into()),
            };
            if let Some(def) = structs.get(sname) {
                return def
                    .fields
                    .iter()
                    .find(|(n, _)| n == fname)
                    .map(|(_, t)| t.clone())
                    .ok_or_else(|| format!("struct `{sname}` has no field `{fname}`"));
            }
            if let Some(def) = vectors.get(sname) {
                if def.fields.iter().any(|n| n == fname) {
                    return Ok(def.ty.clone());
                }
                return Err(format!("vector `{sname}` has no field `{fname}`"));
            }
            Err(format!("unknown struct `{sname}`"))
        }
        Expr::Index(arr, idx) => {
            let at = infer_expr(arr, env, structs, enums, vectors, fns, None)?;
            let it = infer_expr(idx, env, structs, enums, vectors, fns, Some(&Ty::I32))?;
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
            let ti = infer_expr(inner, env, structs, enums, vectors, fns, None)?;
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
        Stmt::Quant { body } => block_contains_return(body),
        Stmt::Gpu { body } => block_contains_return(body),
        Stmt::Let { .. } | Stmt::Expr(_) | Stmt::Assign { .. } | Stmt::Break => false,
    }
}

fn stmt_has_break(st: &Stmt) -> bool {
    match st {
        Stmt::Break => true,
        Stmt::If { then_block, .. } => block_has_break(then_block),
        Stmt::While { body, .. }
        | Stmt::Loop { body }
        | Stmt::For { body, .. }
        | Stmt::Quant { body }
        | Stmt::Gpu { body } => block_has_break(body),
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
    vectors: &HashMap<String, VectorDef>,
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
                Some(t) => Some(normalize_ty(t, struct_fields, enums, vectors)?),
                None => None,
            };
            if let Some(a) = &ann_norm {
                if !is_in_quant_scope(env) {
                    reject_quantum_ty(a, &format!("let `{name}` type annotation"))?;
                }
            }
            let t = infer_expr(
                init,
                env,
                struct_fields,
                enums,
                vectors,
                fn_sigs,
                ann_norm.as_ref(),
            )?;
            if matches!(t, Ty::Unit) {
                return Err(format!(
                    "let {name}: cannot bind a void value (missing return?)"
                ));
            }
            if !is_in_quant_scope(env) {
                reject_quantum_ty(&t, &format!("let `{name}` initializer"))?;
            }
            if let Some(a_raw) = ann {
                let a = normalize_ty(a_raw, struct_fields, enums, vectors)?;
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
            infer_expr(e, env, struct_fields, enums, vectors, fn_sigs, None)?;
        }
        Stmt::Assign { target, value } => {
            let tt = infer_expr(target, env, struct_fields, enums, vectors, fn_sigs, None)?;
            match target {
                Expr::Ident(_) | Expr::Deref(_) => {}
                Expr::Index(_, _) if index_chain_root_is_assignable_array_lvalue(target) => {}
                _ => {
                    return Err(
                        "assignment target must be variable, dereference, or indexed local array (e.g. `x`, `*p`, `a[i]`)"
                            .into(),
                    )
                }
            }
            let vt = infer_expr(
                value,
                env,
                struct_fields,
                enums,
                vectors,
                fn_sigs,
                Some(&tt),
            )?;
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
            let t = infer_expr(e, env, struct_fields, enums, vectors, fn_sigs, Some(ret_ty))?;
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
            let t = infer_expr(
                cond,
                env,
                struct_fields,
                enums,
                vectors,
                fn_sigs,
                Some(&Ty::Bool),
            )?;
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
                    vectors,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    break_inside_while_or_for,
                )?;
            }
            if let Some(tail) = &then_block.tail {
                infer_expr(
                    tail,
                    &then_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    None,
                )?;
            }
        }
        Stmt::While { cond, body } => {
            let t = infer_expr(
                cond,
                env,
                struct_fields,
                enums,
                vectors,
                fn_sigs,
                Some(&Ty::Bool),
            )?;
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
                    vectors,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    true,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(
                    tail,
                    &body_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    None,
                )?;
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
                    vectors,
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
                    vectors,
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
            let ts = infer_expr(start, env, struct_fields, enums, vectors, fn_sigs, None)?;
            if !is_integer_ty(&ts) {
                return Err(format!(
                    "`for` range start must be an integer type, got {ts:?}"
                ));
            }
            let te = infer_expr(end, env, struct_fields, enums, vectors, fn_sigs, Some(&ts))?;
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
                    vectors,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    true,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(
                    tail,
                    &body_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    None,
                )?;
            }
        }
        Stmt::Quant { body } => {
            let mut body_env = enter_quant_scope(env);
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    break_inside_while_or_for,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(
                    tail,
                    &body_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    None,
                )?;
            }
        }
        Stmt::Gpu { body } => {
            let mut body_env = env.clone();
            for st in &body.stmts {
                check_stmt(
                    st,
                    &mut body_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    fn_ret,
                    loop_depth,
                    break_inside_while_or_for,
                )?;
            }
            if let Some(tail) = &body.tail {
                infer_expr(
                    tail,
                    &body_env,
                    struct_fields,
                    enums,
                    vectors,
                    fn_sigs,
                    None,
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
