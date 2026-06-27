use std::collections::{HashMap, HashSet};

use crate::ast::{
    Ability, Block, EnumDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt, StructDef, Ty,
    VectorDef, method_symbol,
};
use crate::nia_std::{
    ALLOC, CIS, COMPLEX_ADD, COMPLEX_DIV, COMPLEX_MUL, COMPLEX_NEW, COMPLEX_SCALE, COMPLEX_SUB,
    COMPLEX_TYPE, COS, DEALLOC, DIGEST_EQ, GATE_CCNOT, GATE_CCZ, GATE_CH, GATE_CNOT, GATE_CR1,
    GATE_CRX, GATE_CRY, GATE_CRZ, GATE_CS, GATE_CSDG, GATE_CSWAP, GATE_CT, GATE_CTDG, GATE_CY,
    GATE_CZ, GATE_H, GATE_I, GATE_R1, GATE_RX, GATE_RY, GATE_RZ, GATE_S, GATE_SDG, GATE_SWAP,
    GATE_T, GATE_TDG, GATE_X, GATE_Y, GATE_Z, LEN, LIST_CAPACITY, LIST_GET, LIST_LEN, LIST_NEW,
    LIST_PUSH, LIST_WITH_CAPACITY, MATRIX_CLONE, MATRIX_COLS, MATRIX_DROP, MATRIX_GET, MATRIX_LEN,
    MATRIX_NEW, MATRIX_REFCOUNT, MATRIX_ROWS, MATRIX_SET, MATRIX_TYPE, MEASURE, MERKLE_LEAF_HASH,
    MERKLE_NODE_HASH, MERKLE_ROOT, MERKLE_ROOT_FROM_DATA, MERKLE_VERIFY, OUTER, PI, PRINTLN, QUBIT,
    READ, REALLOC, RECORD, RESULT, SHA256, SIN, TO_ARRAY, TO_MATRIX, TO_VEC, VECTOR_CLONE,
    VECTOR_DROP, VECTOR_GET, VECTOR_LEN, VECTOR_REFCOUNT, VECTOR_SET,
};

const QUANT_SCOPE_MARKER: &str = "\0nia.quant.scope";
const CLONE_METHOD: &str = "clone";

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
        Ty::Fn(params, ret) => Ok(Ty::Fn(
            params
                .iter()
                .map(|t| normalize_ty(t, structs, enums, vectors))
                .collect::<Result<Vec<_>, _>>()?,
            Box::new(normalize_ty(ret, structs, enums, vectors)?),
        )),
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

fn split_variant_path(path: &str) -> Option<(&str, &str)> {
    path.rsplit_once("::")
        .filter(|(enum_name, variant)| !enum_name.is_empty() && !variant.is_empty())
}

fn path_leaf(path: &str) -> &str {
    path.rsplit("::").next().unwrap_or(path)
}

fn has_declared_ability(abilities: &[Ability], ability: Ability) -> bool {
    abilities.contains(&ability)
}

fn ability_label(ability: Ability) -> &'static str {
    match ability {
        Ability::Copy => "copy",
        Ability::Clone => "clone",
        Ability::Drop => "drop",
        Ability::Deref => "deref",
    }
}

fn is_legacy_scalar_ability_carveout(t: &Ty, ability: Ability) -> bool {
    if matches!(ability, Ability::Deref) {
        return false;
    }
    matches!(
        t,
        Ty::Unit
            | Ty::Bool
            | Ty::I8
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
            | Ty::F16
            | Ty::F32
            | Ty::F64
    )
}

fn has_formal_ability(
    t: &Ty,
    ability: Ability,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
) -> bool {
    match t {
        Ty::Struct(name) if vectors.contains_key(name) => vectors
            .get(name)
            .is_some_and(|v| has_declared_ability(&v.abilities, ability)),
        Ty::Struct(name) => structs
            .get(name)
            .is_some_and(|s| has_declared_ability(&s.abilities, ability)),
        Ty::Enum(name) => enums
            .get(name)
            .is_some_and(|e| has_declared_ability(&e.abilities, ability)),
        Ty::Vector(name, _) => vectors
            .get(name)
            .is_some_and(|v| has_declared_ability(&v.abilities, ability)),
        Ty::Array(elem, _) | Ty::AnonVector(elem, _) => {
            !matches!(ability, Ability::Deref)
                && has_formal_ability(elem, ability, structs, enums, vectors)
        }
        _ => false,
    }
}

fn supports_clone_method(
    t: &Ty,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
) -> bool {
    match t {
        Ty::Struct(name) if vectors.contains_key(name) => vectors
            .get(name)
            .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Clone)),
        Ty::Struct(name) => structs
            .get(name)
            .is_some_and(|s| has_declared_ability(&s.abilities, Ability::Clone)),
        Ty::Enum(name) => enums
            .get(name)
            .is_some_and(|e| has_declared_ability(&e.abilities, Ability::Clone)),
        Ty::Vector(name, _) => vectors
            .get(name)
            .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Clone)),
        Ty::Array(elem, _) | Ty::AnonVector(elem, _) => {
            supports_clone_method(elem, structs, enums, vectors)
        }
        _ => false,
    }
}

fn supports_decl_ability(
    t: &Ty,
    ability: Ability,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
) -> bool {
    if has_formal_ability(t, ability, structs, enums, vectors)
        || is_legacy_scalar_ability_carveout(t, ability)
    {
        return true;
    }
    match t {
        Ty::Array(elem, _) | Ty::AnonVector(elem, _) if !matches!(ability, Ability::Deref) => {
            supports_decl_ability(elem, ability, structs, enums, vectors)
        }
        _ => false,
    }
}

fn validate_copy_implies_clone(owner: &str, abilities: &[Ability]) -> Result<(), String> {
    if has_declared_ability(abilities, Ability::Copy)
        && !has_declared_ability(abilities, Ability::Clone)
    {
        return Err(format!(
            "`{owner}` has `copy` but is missing required `clone` ability"
        ));
    }
    Ok(())
}

fn custom_method_name(owner: &str, method: &str) -> String {
    method_symbol(&Ty::Struct(owner.into()), method)
}

fn validate_custom_drop_sig(s: &StructDef, fn_sigs: &HashMap<String, FnSig>) -> Result<(), String> {
    let method = custom_method_name(&s.name, "drop");
    let Some(sig) = fn_sigs.get(&method) else {
        return Err(format!(
            "struct `{}` has `drop` but does not define `fn drop(self)`",
            s.name
        ));
    };
    if sig.params.len() != 1 || sig.params[0] != Ty::Struct(s.name.clone()) {
        return Err(format!(
            "struct `{}` custom drop must have signature `fn drop(self) ()`",
            s.name
        ));
    }
    if !matches!(sig.ret, None | Some(Ty::Unit)) {
        return Err(format!("struct `{}` custom drop must return `()`", s.name));
    }
    Ok(())
}

fn validate_custom_deref_sig(
    s: &StructDef,
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<(), String> {
    let method = custom_method_name(&s.name, "deref");
    let Some(sig) = fn_sigs.get(&method) else {
        return Err(format!(
            "struct `{}` has `deref` but does not define `fn deref(&self) &Target`",
            s.name
        ));
    };
    if sig.params.len() != 1 || sig.params[0] != Ty::Ptr(Box::new(Ty::Struct(s.name.clone()))) {
        return Err(format!(
            "struct `{}` custom deref must have signature `fn deref(&self) &Target`",
            s.name
        ));
    }
    if !matches!(sig.ret, Some(Ty::Ptr(_))) {
        return Err(format!(
            "struct `{}` custom deref must return a pointer/reference type",
            s.name
        ));
    }
    Ok(())
}

fn validate_abilities(
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<(), String> {
    for s in structs.values() {
        validate_copy_implies_clone(&format!("struct `{}`", s.name), &s.abilities)?;
        for ability in s.abilities.clone() {
            match ability {
                Ability::Copy | Ability::Clone => {
                    for (field, ty) in &s.fields {
                        if !supports_decl_ability(ty, ability, structs, enums, vectors) {
                            let ability = ability_label(ability);
                            return Err(format!(
                                "struct `{}` has `{ability}` but field `{field}` does not support it",
                                s.name
                            ));
                        }
                    }
                }
                Ability::Drop => validate_custom_drop_sig(s, fn_sigs)?,
                Ability::Deref => validate_custom_deref_sig(s, fn_sigs)?,
            }
        }
    }

    for e in enums.values() {
        validate_copy_implies_clone(&format!("enum `{}`", e.name), &e.abilities)?;
        if has_declared_ability(&e.abilities, Ability::Deref) {
            return Err(format!(
                "enum `{}` declares `deref`, but enum deref is not supported yet",
                e.name
            ));
        }
        for ability in e.abilities.clone() {
            if matches!(ability, Ability::Deref) {
                continue;
            }
            for variant in &e.variants {
                match &variant.fields {
                    EnumVariantFields::Unit => {}
                    EnumVariantFields::Tuple(fields) => {
                        for (idx, ty) in fields.iter().enumerate() {
                            if !supports_decl_ability(ty, ability, structs, enums, vectors) {
                                let ability = ability_label(ability);
                                return Err(format!(
                                    "enum `{}` has `{ability}` but variant `{}` field `{idx}` does not support it",
                                    e.name, variant.name
                                ));
                            }
                        }
                    }
                    EnumVariantFields::Struct(fields) => {
                        for (field, ty) in fields {
                            if !supports_decl_ability(ty, ability, structs, enums, vectors) {
                                let ability = ability_label(ability);
                                return Err(format!(
                                    "enum `{}` has `{ability}` but variant `{}` field `{field}` does not support it",
                                    e.name, variant.name
                                ));
                            }
                        }
                    }
                }
            }
        }
    }

    for v in vectors.values() {
        validate_copy_implies_clone(&format!("vector `{}`", v.name), &v.abilities)?;
        if has_declared_ability(&v.abilities, Ability::Deref) {
            return Err(format!(
                "vector `{}` declares `deref`, but vector deref is not supported yet",
                v.name
            ));
        }
        for ability in v.abilities.clone() {
            if matches!(ability, Ability::Deref) {
                continue;
            }
            if !supports_decl_ability(&v.ty, ability, structs, enums, vectors) {
                let ability = ability_label(ability);
                return Err(format!(
                    "vector `{}` has `{ability}` but element type does not support it",
                    v.name
                ));
            }
        }
    }

    Ok(())
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
        Ty::Fn(params, ret) => params.iter().any(contains_quantum_ty) || contains_quantum_ty(ret),
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
        if crate::nia_std::is_reserved_type_name(path_leaf(&s.name)) {
            return Err(format!("type name `{}` is reserved", s.name));
        }
        if struct_map.insert(s.name.clone(), s.clone()).is_some() {
            return Err(format!("duplicate struct {}", s.name));
        }
    }
    let mut vector_map: HashMap<String, VectorDef> = HashMap::new();
    for v in vectors {
        if crate::nia_std::is_reserved_type_name(path_leaf(&v.name)) {
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
        if crate::nia_std::is_reserved_type_name(path_leaf(&e.name)) {
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
        if crate::nia_std::is_reserved_fn_name(path_leaf(&f.name)) {
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
    validate_abilities(
        &normalized_structs,
        &normalized_enums,
        &normalized_vectors,
        &fn_sigs,
    )?;
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
    check_fn_moves(f, struct_fields, enums, vectors, fn_sigs)?;
    Ok(env)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalMoveState {
    Available,
    Moved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ExprMoveMode {
    Consume,
    ReadOnly,
}

struct MoveCtx<'a> {
    structs: &'a HashMap<String, StructDef>,
    enums: &'a HashMap<String, EnumDef>,
    vectors: &'a HashMap<String, VectorDef>,
    fns: &'a HashMap<String, FnSig>,
}

type MoveStates = HashMap<String, LocalMoveState>;

fn check_fn_moves(
    f: &FnDef,
    struct_fields: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<(), String> {
    let sig = fn_sigs
        .get(&f.name)
        .ok_or_else(|| format!("missing sig for {}", f.name))?;
    let ctx = MoveCtx {
        structs: struct_fields,
        enums,
        vectors,
        fns: fn_sigs,
    };
    let mut env: HashMap<String, Ty> = if f.is_quantum {
        enter_quant_scope(&HashMap::new())
    } else {
        HashMap::new()
    };
    let mut states = MoveStates::new();
    for ((pname, _), pty) in f.params.iter().zip(&sig.params) {
        env.insert(pname.clone(), pty.clone());
        states.insert(pname.clone(), LocalMoveState::Available);
    }
    check_moves_block(
        &f.body,
        &mut env,
        &mut states,
        &ctx,
        sig.ret.as_ref(),
        sig.ret.as_ref(),
    )
}

fn check_moves_block(
    block: &Block,
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
    fn_ret: Option<&Ty>,
    tail_hint: Option<&Ty>,
) -> Result<(), String> {
    for st in &block.stmts {
        check_moves_stmt(st, env, states, ctx, fn_ret)?;
    }
    if let Some(tail) = &block.tail {
        check_moves_expr(tail, env, states, ctx, tail_hint, ExprMoveMode::Consume)?;
    }
    Ok(())
}

fn merge_moved_from_child(parent: &mut MoveStates, child: &MoveStates) {
    let names = parent.keys().cloned().collect::<Vec<_>>();
    for name in names {
        if matches!(child.get(&name), Some(LocalMoveState::Moved)) {
            parent.insert(name, LocalMoveState::Moved);
        }
    }
}

fn check_moves_stmt(
    st: &Stmt,
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
    fn_ret: Option<&Ty>,
) -> Result<(), String> {
    match st {
        Stmt::Let {
            name,
            ty: ann,
            init,
        } => {
            let ann_norm = match ann {
                Some(t) => Some(normalize_ty(t, ctx.structs, ctx.enums, ctx.vectors)?),
                None => None,
            };
            check_moves_expr(
                init,
                env,
                states,
                ctx,
                ann_norm.as_ref(),
                ExprMoveMode::Consume,
            )?;
            let t = infer_expr(
                init,
                env,
                ctx.structs,
                ctx.enums,
                ctx.vectors,
                ctx.fns,
                ann_norm.as_ref(),
            )?;
            env.insert(name.clone(), t);
            states.insert(name.clone(), LocalMoveState::Available);
        }
        Stmt::Expr(e) => {
            check_moves_expr(e, env, states, ctx, None, ExprMoveMode::Consume)?;
        }
        Stmt::Assign { target, value } => {
            let target_ty = infer_expr(
                target,
                env,
                ctx.structs,
                ctx.enums,
                ctx.vectors,
                ctx.fns,
                None,
            )?;
            check_moves_lvalue(target, env, states, ctx, true)?;
            check_moves_expr(
                value,
                env,
                states,
                ctx,
                Some(&target_ty),
                ExprMoveMode::Consume,
            )?;
            if let Expr::Ident(name) = target {
                if env.contains_key(name) {
                    states.insert(name.clone(), LocalMoveState::Available);
                }
            }
        }
        Stmt::Return(e) => {
            check_moves_expr(e, env, states, ctx, fn_ret, ExprMoveMode::Consume)?;
        }
        Stmt::Break => {}
        Stmt::If { cond, then_block } => {
            check_moves_expr(
                cond,
                env,
                states,
                ctx,
                Some(&Ty::Bool),
                ExprMoveMode::Consume,
            )?;
            let mut then_env = env.clone();
            let mut then_states = states.clone();
            check_moves_block(
                then_block,
                &mut then_env,
                &mut then_states,
                ctx,
                fn_ret,
                None,
            )?;
            merge_moved_from_child(states, &then_states);
        }
        Stmt::While { cond, body } => {
            check_moves_expr(
                cond,
                env,
                states,
                ctx,
                Some(&Ty::Bool),
                ExprMoveMode::Consume,
            )?;
            let mut body_env = env.clone();
            let mut body_states = states.clone();
            check_moves_block(body, &mut body_env, &mut body_states, ctx, fn_ret, None)?;
            merge_moved_from_child(states, &body_states);
        }
        Stmt::Loop { body } => {
            let mut body_env = env.clone();
            let mut body_states = states.clone();
            check_moves_block(body, &mut body_env, &mut body_states, ctx, fn_ret, None)?;
            merge_moved_from_child(states, &body_states);
        }
        Stmt::For {
            var,
            start,
            end,
            body,
        } => {
            let start_ty = check_moves_expr(start, env, states, ctx, None, ExprMoveMode::Consume)?;
            check_moves_expr(
                end,
                env,
                states,
                ctx,
                Some(&start_ty),
                ExprMoveMode::Consume,
            )?;
            let mut body_env = env.clone();
            let mut body_states = states.clone();
            body_env.insert(var.clone(), start_ty);
            body_states.insert(var.clone(), LocalMoveState::Available);
            check_moves_block(body, &mut body_env, &mut body_states, ctx, fn_ret, None)?;
            merge_moved_from_child(states, &body_states);
        }
        Stmt::Quant { body } => {
            let mut body_env = enter_quant_scope(env);
            let mut body_states = states.clone();
            check_moves_block(body, &mut body_env, &mut body_states, ctx, fn_ret, None)?;
            merge_moved_from_child(states, &body_states);
        }
        Stmt::Gpu { body } => {
            let mut body_env = env.clone();
            let mut body_states = states.clone();
            check_moves_block(body, &mut body_env, &mut body_states, ctx, fn_ret, None)?;
            merge_moved_from_child(states, &body_states);
        }
    }
    Ok(())
}

fn is_copy_for_moves(t: &Ty, ctx: &MoveCtx<'_>) -> bool {
    match t {
        Ty::Unit
        | Ty::Bool
        | Ty::I8
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
        | Ty::F16
        | Ty::F32
        | Ty::F64
        | Ty::String
        | Ty::Qubit
        | Ty::Result
        | Ty::Ptr(_)
        | Ty::Fn(_, _) => true,
        Ty::Array(elem, _) | Ty::AnonVector(elem, _) => is_copy_for_moves(elem, ctx),
        Ty::HeapVector(_) | Ty::List(_) | Ty::Matrix(_, _) => true,
        Ty::Struct(name) if name == COMPLEX_TYPE => true,
        Ty::Struct(name) if ctx.vectors.contains_key(name) => {
            let vector = ctx.vectors.get(name).expect("checked vector existence");
            has_declared_ability(&vector.abilities, Ability::Copy)
                || is_copy_for_moves(&vector.ty, ctx)
        }
        Ty::Struct(name) => ctx
            .structs
            .get(name)
            .is_some_and(|s| has_declared_ability(&s.abilities, Ability::Copy)),
        Ty::Enum(name) => ctx
            .enums
            .get(name)
            .is_some_and(|e| has_declared_ability(&e.abilities, Ability::Copy)),
        Ty::Vector(name, elem) => {
            ctx.vectors
                .get(name)
                .is_some_and(|v| has_declared_ability(&v.abilities, Ability::Copy))
                || is_copy_for_moves(elem, ctx)
        }
    }
}

fn ensure_local_available(name: &str, states: &MoveStates) -> Result<(), String> {
    if matches!(states.get(name), Some(LocalMoveState::Moved)) {
        return Err(format!("use of moved local `{name}`"));
    }
    Ok(())
}

fn consume_ident_if_needed(
    name: &str,
    ty: &Ty,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
    mode: ExprMoveMode,
) -> Result<(), String> {
    if !states.contains_key(name) {
        return Ok(());
    }
    ensure_local_available(name, states)?;
    if matches!(mode, ExprMoveMode::Consume) && !is_copy_for_moves(ty, ctx) {
        states.insert(name.to_string(), LocalMoveState::Moved);
    }
    Ok(())
}

fn check_moves_args_with_params(
    args: &[Expr],
    params: &[Ty],
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    for (arg, param) in args.iter().zip(params) {
        check_moves_expr(arg, env, states, ctx, Some(param), ExprMoveMode::Consume)?;
    }
    Ok(())
}

fn check_moves_args_fallback(
    args: &[Expr],
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    for arg in args {
        check_moves_expr(arg, env, states, ctx, None, ExprMoveMode::Consume)?;
    }
    Ok(())
}

fn check_moves_call(
    name: &str,
    args: &[Expr],
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    if let Some(local_ty) = env.get(name).cloned() {
        if matches!(local_ty, Ty::Fn(_, _)) {
            ensure_local_available(name, states)?;
            if let Ty::Fn(params, _) = local_ty {
                return check_moves_args_with_params(args, &params, env, states, ctx);
            }
        }
    }

    if name == PRINTLN || name == LEN {
        for arg in args {
            check_moves_expr(arg, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        }
        return Ok(());
    }

    if let Some(def) = ctx.structs.get(name) {
        if def.is_tuple {
            let params = def
                .fields
                .iter()
                .map(|(_, ty)| ty.clone())
                .collect::<Vec<_>>();
            return check_moves_args_with_params(args, &params, env, states, ctx);
        }
    }

    if let Some((enum_name, variant)) = split_variant_path(name) {
        if let Some(edef) = ctx.enums.get(enum_name) {
            if let Some(EnumVariantFields::Tuple(params)) = enum_variant(edef, variant) {
                return check_moves_args_with_params(args, params, env, states, ctx);
            }
        }
    }

    if let Some(sig) = ctx.fns.get(name) {
        return check_moves_args_with_params(args, &sig.params, env, states, ctx);
    }

    check_moves_args_fallback(args, env, states, ctx)
}

fn check_moves_generic_call(
    name: &str,
    args: &[Expr],
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    if name == LIST_WITH_CAPACITY && args.len() == 1 {
        check_moves_expr(
            &args[0],
            env,
            states,
            ctx,
            Some(&Ty::I32),
            ExprMoveMode::Consume,
        )?;
        return Ok(());
    }
    check_moves_args_fallback(args, env, states, ctx)
}

fn check_moves_method_call(
    receiver: &Expr,
    name: &str,
    args: &[Expr],
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    let recv_ty = infer_expr(
        receiver,
        env,
        ctx.structs,
        ctx.enums,
        ctx.vectors,
        ctx.fns,
        None,
    )?;

    if name == CLONE_METHOD {
        check_moves_expr(receiver, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        return check_moves_args_fallback(args, env, states, ctx);
    }

    if name == TO_MATRIX || name == TO_ARRAY || name == TO_VEC || name == "det" {
        check_moves_expr(receiver, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        return check_moves_args_fallback(args, env, states, ctx);
    }

    if matches!(recv_ty, Ty::List(_)) {
        check_moves_expr(receiver, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        if (name == LIST_LEN || name == LIST_CAPACITY) && args.is_empty() {
            return Ok(());
        }
        if name == LIST_PUSH && args.len() == 1 {
            let Ty::List(elem_ty) = recv_ty else {
                unreachable!("guarded above")
            };
            check_moves_expr(
                &args[0],
                env,
                states,
                ctx,
                Some(elem_ty.as_ref()),
                ExprMoveMode::Consume,
            )?;
            return Ok(());
        }
        if name == LIST_GET && args.len() == 1 {
            check_moves_expr(
                &args[0],
                env,
                states,
                ctx,
                Some(&Ty::I32),
                ExprMoveMode::Consume,
            )?;
            return Ok(());
        }
        return check_moves_args_fallback(args, env, states, ctx);
    }

    let owner_ty = method_receiver_owner_ty(&recv_ty);
    let symbol = method_symbol(owner_ty, name);
    if let Some(sig) = ctx.fns.get(&symbol) {
        let receiver_mode = match sig.params.first() {
            Some(Ty::Ptr(_)) => ExprMoveMode::ReadOnly,
            Some(_) if matches!(recv_ty, Ty::Ptr(_)) => ExprMoveMode::ReadOnly,
            Some(_) => ExprMoveMode::Consume,
            None => ExprMoveMode::ReadOnly,
        };
        check_moves_expr(receiver, env, states, ctx, None, receiver_mode)?;
        return check_moves_args_with_params(args, &sig.params[1..], env, states, ctx);
    }

    check_moves_expr(receiver, env, states, ctx, None, ExprMoveMode::Consume)?;
    check_moves_args_fallback(args, env, states, ctx)
}

fn check_moves_struct_literal_fields(
    fields: &[(String, Expr)],
    def_fields: &[(String, Ty)],
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    for (field_name, field_ty) in def_fields {
        if let Some((_, expr)) = fields.iter().find(|(name, _)| name == field_name) {
            check_moves_expr(
                expr,
                env,
                states,
                ctx,
                Some(field_ty),
                ExprMoveMode::Consume,
            )?;
        }
    }
    Ok(())
}

fn check_moves_literal_elems(
    elems: &[Expr],
    elem_hint: Option<Ty>,
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    let inferred_first;
    let hint = if let Some(hint) = elem_hint.as_ref() {
        Some(hint)
    } else if let Some(first) = elems.first() {
        inferred_first = Some(infer_expr(
            first,
            env,
            ctx.structs,
            ctx.enums,
            ctx.vectors,
            ctx.fns,
            None,
        )?);
        inferred_first.as_ref()
    } else {
        None
    };
    for elem in elems {
        check_moves_expr(elem, env, states, ctx, hint, ExprMoveMode::Consume)?;
    }
    Ok(())
}

fn check_moves_match_arm_bindings(
    pattern: &MatchPattern,
    edef: &EnumDef,
    arm_env: &mut HashMap<String, Ty>,
    arm_states: &mut MoveStates,
) {
    match pattern {
        MatchPattern::Unit { .. } => {}
        MatchPattern::Tuple {
            variant, bindings, ..
        } => {
            if let Some(EnumVariantFields::Tuple(fields)) = enum_variant(edef, variant) {
                for (binding, ty) in bindings.iter().zip(fields) {
                    arm_env.insert(binding.clone(), ty.clone());
                    arm_states.insert(binding.clone(), LocalMoveState::Available);
                }
            }
        }
        MatchPattern::Struct {
            variant, bindings, ..
        } => {
            if let Some(EnumVariantFields::Struct(fields)) = enum_variant(edef, variant) {
                for binding in bindings {
                    if let Some((_, ty)) = fields.iter().find(|(name, _)| name == binding) {
                        arm_env.insert(binding.clone(), ty.clone());
                        arm_states.insert(binding.clone(), LocalMoveState::Available);
                    }
                }
            }
        }
    }
}

fn check_moves_closure(
    params: &[(String, Option<Ty>)],
    body: &Block,
    closure_ty: &Ty,
    ctx: &MoveCtx<'_>,
) -> Result<(), String> {
    let Ty::Fn(param_tys, ret_ty) = closure_ty else {
        return Ok(());
    };
    let mut closure_env = HashMap::new();
    let mut closure_states = MoveStates::new();
    for ((name, _), ty) in params.iter().zip(param_tys) {
        closure_env.insert(name.clone(), ty.clone());
        closure_states.insert(name.clone(), LocalMoveState::Available);
    }
    check_moves_block(
        body,
        &mut closure_env,
        &mut closure_states,
        ctx,
        Some(ret_ty.as_ref()),
        Some(ret_ty.as_ref()),
    )
}

fn check_moves_expr(
    e: &Expr,
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
    hint: Option<&Ty>,
    mode: ExprMoveMode,
) -> Result<Ty, String> {
    let inferred = infer_expr(e, env, ctx.structs, ctx.enums, ctx.vectors, ctx.fns, hint)?;
    match e {
        Expr::Int(_) | Expr::Float(_) | Expr::Bool(_) | Expr::String(_) => {}
        Expr::Ident(name) => consume_ident_if_needed(name, &inferred, states, ctx, mode)?,
        Expr::Neg(inner) | Expr::Not(inner) | Expr::BitNot(inner) => {
            check_moves_expr(inner, env, states, ctx, None, ExprMoveMode::Consume)?;
        }
        Expr::Add(l, r)
        | Expr::Sub(l, r)
        | Expr::Mul(l, r)
        | Expr::VecDot(l, r)
        | Expr::Div(l, r)
        | Expr::Rem(l, r)
        | Expr::BitAnd(l, r)
        | Expr::BitOr(l, r)
        | Expr::BitXor(l, r)
        | Expr::Shl(l, r)
        | Expr::Shr(l, r)
        | Expr::Eq(l, r)
        | Expr::Ne(l, r)
        | Expr::Lt(l, r)
        | Expr::Le(l, r)
        | Expr::Gt(l, r)
        | Expr::Ge(l, r) => {
            check_moves_expr(l, env, states, ctx, None, ExprMoveMode::Consume)?;
            check_moves_expr(r, env, states, ctx, None, ExprMoveMode::Consume)?;
        }
        Expr::Call { name, args } => check_moves_call(name, args, env, states, ctx)?,
        Expr::GenericCall { name, args, .. } => {
            check_moves_generic_call(name, args, env, states, ctx)?
        }
        Expr::MethodCall {
            receiver,
            name,
            args,
        } => check_moves_method_call(receiver, name, args, env, states, ctx)?,
        Expr::CallExpr { callee, args } => {
            let callee_ty =
                check_moves_expr(callee, env, states, ctx, None, ExprMoveMode::Consume)?;
            if let Ty::Fn(params, _) = callee_ty {
                check_moves_args_with_params(args, &params, env, states, ctx)?;
            } else {
                check_moves_args_fallback(args, env, states, ctx)?;
            }
        }
        Expr::Closure { params, body, .. } => check_moves_closure(params, body, &inferred, ctx)?,
        Expr::StructLit { name, fields } => {
            if let Some(def) = ctx.structs.get(name) {
                check_moves_struct_literal_fields(fields, &def.fields, env, states, ctx)?;
            } else if let Some((enum_name, variant)) = split_variant_path(name) {
                if let Some(edef) = ctx.enums.get(enum_name) {
                    if let Some(EnumVariantFields::Struct(def_fields)) = enum_variant(edef, variant)
                    {
                        check_moves_struct_literal_fields(fields, def_fields, env, states, ctx)?;
                    }
                }
            }
        }
        Expr::VectorLit { name, fields } => {
            if let Some(def) = ctx.vectors.get(name) {
                let def_fields = def
                    .fields
                    .iter()
                    .map(|name| (name.clone(), def.ty.clone()))
                    .collect::<Vec<_>>();
                check_moves_struct_literal_fields(fields, &def_fields, env, states, ctx)?;
            }
        }
        Expr::AnonVectorLit(elems) => {
            let elem_hint = match hint {
                Some(Ty::AnonVector(elem, _)) | Some(Ty::HeapVector(elem)) => {
                    Some(elem.as_ref().clone())
                }
                _ => None,
            };
            check_moves_literal_elems(elems, elem_hint, env, states, ctx)?;
        }
        Expr::ArrayLit(elems) => {
            let elem_hint = match hint {
                Some(Ty::Array(elem, _)) => Some(elem.as_ref().clone()),
                _ => None,
            };
            check_moves_literal_elems(elems, elem_hint, env, states, ctx)?;
        }
        Expr::EnumVariant { .. } => {}
        Expr::EnumTuple {
            enum_name,
            variant,
            args,
        } => {
            if let Some(edef) = ctx.enums.get(enum_name) {
                if let Some(EnumVariantFields::Tuple(params)) = enum_variant(edef, variant) {
                    check_moves_args_with_params(args, params, env, states, ctx)?;
                }
            }
        }
        Expr::EnumStruct {
            enum_name,
            variant,
            fields,
        } => {
            if let Some(edef) = ctx.enums.get(enum_name) {
                if let Some(EnumVariantFields::Struct(def_fields)) = enum_variant(edef, variant) {
                    check_moves_struct_literal_fields(fields, def_fields, env, states, ctx)?;
                }
            }
        }
        Expr::Match { scrutinee, arms } => {
            let scrutinee_ty =
                check_moves_expr(scrutinee, env, states, ctx, None, ExprMoveMode::Consume)?;
            let Ty::Enum(enum_name) = scrutinee_ty else {
                return Ok(inferred);
            };
            let Some(edef) = ctx.enums.get(&enum_name) else {
                return Ok(inferred);
            };
            let mut merged_states = states.clone();
            for (pattern, arm_expr) in arms {
                let mut arm_env = env.clone();
                let mut arm_states = states.clone();
                check_moves_match_arm_bindings(pattern, edef, &mut arm_env, &mut arm_states);
                check_moves_expr(
                    arm_expr,
                    &mut arm_env,
                    &mut arm_states,
                    ctx,
                    hint,
                    ExprMoveMode::Consume,
                )?;
                merge_moved_from_child(&mut merged_states, &arm_states);
            }
            *states = merged_states;
        }
        Expr::Quant { body } => {
            let mut body_env = enter_quant_scope(env);
            let mut body_states = states.clone();
            check_moves_block(body, &mut body_env, &mut body_states, ctx, None, hint)?;
            merge_moved_from_child(states, &body_states);
        }
        Expr::Gpu { body } => {
            let mut body_env = env.clone();
            let mut body_states = states.clone();
            check_moves_block(body, &mut body_env, &mut body_states, ctx, None, hint)?;
            merge_moved_from_child(states, &body_states);
        }
        Expr::Field(obj, field_name) => {
            check_moves_expr(obj, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
            if matches!(mode, ExprMoveMode::Consume) && !is_copy_for_moves(&inferred, ctx) {
                return Err(format!(
                    "cannot move field `{field_name}` out of a non-copy value; partial moves are not supported yet"
                ));
            }
        }
        Expr::Index(arr, idx) => {
            check_moves_expr(arr, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
            check_moves_expr(idx, env, states, ctx, Some(&Ty::I32), ExprMoveMode::Consume)?;
            if matches!(mode, ExprMoveMode::Consume) && !is_copy_for_moves(&inferred, ctx) {
                return Err(
                    "cannot move out of an indexed value; indexed moves are not supported yet"
                        .into(),
                );
            }
        }
        Expr::AddrOf(inner) => check_moves_lvalue(inner, env, states, ctx, false)?,
        Expr::Deref(inner) => {
            check_moves_expr(inner, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        }
    }
    Ok(inferred)
}

fn check_moves_lvalue(
    target: &Expr,
    env: &mut HashMap<String, Ty>,
    states: &mut MoveStates,
    ctx: &MoveCtx<'_>,
    allow_reinit_ident: bool,
) -> Result<(), String> {
    match target {
        Expr::Ident(name) => {
            if !allow_reinit_ident && states.contains_key(name) {
                ensure_local_available(name, states)?;
            }
        }
        Expr::Deref(inner) => {
            check_moves_expr(inner, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        }
        Expr::Index(base, idx) => {
            check_moves_lvalue(base, env, states, ctx, false)?;
            check_moves_expr(idx, env, states, ctx, Some(&Ty::I32), ExprMoveMode::Consume)?;
        }
        _ => {
            check_moves_expr(target, env, states, ctx, None, ExprMoveMode::ReadOnly)?;
        }
    }
    Ok(())
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
        (Ty::Fn(xp, xr), Ty::Fn(yp, yr)) => {
            xp.len() == yp.len()
                && xp.iter().zip(yp).all(|(x, y)| types_equal(x, y))
                && types_equal(xr, yr)
        }
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

fn digest_ty() -> Ty {
    Ty::Array(Box::new(Ty::U8), 32)
}

fn expect_digest_arg(
    name: &str,
    args: &[Expr],
    idx: usize,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<(), String> {
    expect_arg_ty(
        name,
        args,
        idx,
        &digest_ty(),
        env,
        structs,
        enums,
        vectors,
        fns,
    )?;
    Ok(())
}

fn expect_byte_array_arg(
    name: &str,
    args: &[Expr],
    idx: usize,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<usize, String> {
    let got = infer_expr(&args[idx], env, structs, enums, vectors, fns, None)?;
    match got {
        Ty::Array(elem, n) if types_equal(&elem, &Ty::U8) => Ok(n),
        _ => Err(format!(
            "`{name}` argument {} type mismatch: expected [u8; N], got {got:?}",
            idx + 1
        )),
    }
}

fn expect_digest_array_arg(
    name: &str,
    args: &[Expr],
    idx: usize,
    allow_empty: bool,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<usize, String> {
    let got = infer_expr(&args[idx], env, structs, enums, vectors, fns, None)?;
    match got {
        Ty::Array(elem, n) if types_equal(&elem, &digest_ty()) => {
            if !allow_empty && n == 0 {
                return Err(format!("`{name}` expects at least one digest"));
            }
            Ok(n)
        }
        _ => Err(format!(
            "`{name}` argument {} type mismatch: expected [[u8; 32]; N], got {got:?}",
            idx + 1
        )),
    }
}

fn expect_leaf_data_array_arg(
    name: &str,
    args: &[Expr],
    idx: usize,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<(usize, usize), String> {
    let got = infer_expr(&args[idx], env, structs, enums, vectors, fns, None)?;
    match got {
        Ty::Array(ref row_ty, leaves) => match row_ty.as_ref() {
            Ty::Array(elem_ty, leaf_len) if types_equal(elem_ty, &Ty::U8) => {
                if leaves == 0 {
                    return Err(format!("`{name}` expects at least one leaf"));
                }
                Ok((leaves, *leaf_len))
            }
            _ => Err(format!(
                "`{name}` argument {} type mismatch: expected [[u8; M]; N], got {got:?}",
                idx + 1
            )),
        },
        _ => Err(format!(
            "`{name}` argument {} type mismatch: expected [[u8; M]; N], got {got:?}",
            idx + 1
        )),
    }
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

fn infer_integer_bin(
    l: &Expr,
    r: &Expr,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
    hint: Option<&Ty>,
    op: &str,
) -> Result<Ty, String> {
    let integer_hint = hint.filter(|ty| is_integer_ty(ty));
    let tl = infer_expr(l, env, structs, enums, vectors, fns, integer_hint)?;
    let tr = infer_expr(r, env, structs, enums, vectors, fns, Some(&tl))?;
    if !types_equal(&tl, &tr) {
        return Err(format!("`{op}` operands differ: {tl:?} vs {tr:?}"));
    }
    if !is_integer_ty(&tl) {
        return Err(format!("cannot use `{op}` on non-integer type {tl:?}"));
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

fn fn_value_ty(sig: &FnSig) -> Ty {
    Ty::Fn(
        sig.params.clone(),
        Box::new(sig.ret.clone().unwrap_or(Ty::Unit)),
    )
}

fn infer_fn_value_call(
    callee_ty: &Ty,
    args: &[Expr],
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
) -> Result<Ty, String> {
    let Ty::Fn(params, ret) = callee_ty else {
        return Err(format!("call requires a function value, got {callee_ty:?}"));
    };
    if args.len() != params.len() {
        return Err(format!(
            "function value call: expected {} args, got {}",
            params.len(),
            args.len()
        ));
    }
    for (a, pt) in args.iter().zip(params) {
        let at = infer_expr(a, env, structs, enums, vectors, fns, Some(pt))?;
        if !types_equal(&at, pt) {
            return Err(format!(
                "function value call: arg type mismatch: expected {pt:?}, got {at:?}"
            ));
        }
    }
    Ok(ret.as_ref().clone())
}

fn infer_closure_expr(
    params: &[(String, Option<Ty>)],
    explicit_ret: Option<&Ty>,
    body: &Block,
    env: &HashMap<String, Ty>,
    structs: &HashMap<String, StructDef>,
    enums: &HashMap<String, EnumDef>,
    vectors: &HashMap<String, VectorDef>,
    fns: &HashMap<String, FnSig>,
    hint: Option<&Ty>,
) -> Result<Ty, String> {
    let expected = match hint {
        Some(Ty::Fn(params, ret)) => Some((params.as_slice(), ret.as_ref())),
        Some(other) => return Err(format!("closure cannot satisfy {other:?}")),
        None => None,
    };
    if let Some((expected_params, _)) = expected {
        if expected_params.len() != params.len() {
            return Err(format!(
                "closure parameter count mismatch: expected {}, got {}",
                expected_params.len(),
                params.len()
            ));
        }
    }

    let mut closure_params = Vec::with_capacity(params.len());
    let mut closure_env = HashMap::new();
    for (idx, (name, ann)) in params.iter().enumerate() {
        let ty = match (ann, expected) {
            (Some(t), _) => normalize_ty(t, structs, enums, vectors)?,
            (None, Some((expected_params, _))) => expected_params[idx].clone(),
            (None, None) => {
                return Err(format!(
                    "closure parameter `{name}` needs a type annotation or contextual `fn(...) -> ...` type"
                ));
            }
        };
        if closure_env.insert(name.clone(), ty.clone()).is_some() {
            return Err(format!("duplicate closure parameter `{name}`"));
        }
        closure_params.push(ty);
    }

    let ret_ty = match (explicit_ret, expected) {
        (Some(t), _) => normalize_ty(t, structs, enums, vectors)?,
        (None, Some((_, ret))) => ret.clone(),
        (None, None) => {
            return Err(
                "closure needs an explicit `-> T` return type or contextual `fn(...) -> T` type"
                    .into(),
            );
        }
    };

    if contains_quantum_ty(&Ty::Fn(closure_params.clone(), Box::new(ret_ty.clone()))) {
        return Err("closures cannot use quantum types".into());
    }
    if is_in_quant_scope(env) {
        return Err("closures are not supported inside `quant` blocks yet".into());
    }

    for st in &body.stmts {
        check_stmt(
            st,
            &mut closure_env,
            structs,
            enums,
            vectors,
            fns,
            Some(&ret_ty),
            0,
            false,
        )?;
    }
    if let Some(tail) = &body.tail {
        let tail_ty = infer_expr(
            tail,
            &closure_env,
            structs,
            enums,
            vectors,
            fns,
            Some(&ret_ty),
        )?;
        if !types_equal(&tail_ty, &ret_ty) {
            return Err(format!(
                "closure return type mismatch: expected {ret_ty:?}, got {tail_ty:?}"
            ));
        }
    } else if !types_equal(&ret_ty, &Ty::Unit) {
        return Err(format!(
            "closure returning {ret_ty:?} must end with an expression"
        ));
    }

    Ok(Ty::Fn(closure_params, Box::new(ret_ty)))
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
        Expr::Ident(name) => {
            if let Some(t) = env.get(name) {
                return Ok(t.clone());
            }
            if name == PI {
                return Ok(Ty::F64);
            }
            if let Some(sig) = fns.get(name) {
                if sig.is_quantum {
                    return Err(format!(
                        "quantum function `{name}` cannot be used as a function value"
                    ));
                }
                return Ok(fn_value_ty(sig));
            }
            if let Some((enum_name, variant)) = split_variant_path(name) {
                if let Some(edef) = enums.get(enum_name) {
                    let Some(fields) = enum_variant(edef, variant) else {
                        return Err(format!("enum `{enum_name}` has no variant `{variant}`"));
                    };
                    if matches!(fields, EnumVariantFields::Unit) {
                        return Ok(Ty::Enum(enum_name.to_string()));
                    }
                    return Err(format!(
                        "enum variant `{enum_name}::{variant}` requires payload"
                    ));
                }
            }
            Err(format!("unknown variable `{name}`"))
        }
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
        Expr::Not(inner) => {
            let t = infer_expr(inner, env, structs, enums, vectors, fns, Some(&Ty::Bool))?;
            if t != Ty::Bool {
                return Err(format!("cannot use `!` on non-bool type {t:?}"));
            }
            Ok(Ty::Bool)
        }
        Expr::BitNot(inner) => {
            let integer_hint = hint.filter(|ty| is_integer_ty(ty));
            let t = infer_expr(inner, env, structs, enums, vectors, fns, integer_hint)?;
            if !is_integer_ty(&t) {
                return Err(format!("cannot use `~` on non-integer type {t:?}"));
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
        Expr::Rem(l, r) => {
            if matches!(r.as_ref(), Expr::Int(0)) {
                return Err("remainder by zero".into());
            }
            infer_integer_bin(l, r, env, structs, enums, vectors, fns, hint, "%")
        }
        Expr::BitAnd(l, r) => infer_integer_bin(l, r, env, structs, enums, vectors, fns, hint, "&"),
        Expr::BitOr(l, r) => infer_integer_bin(l, r, env, structs, enums, vectors, fns, hint, "|"),
        Expr::BitXor(l, r) => infer_integer_bin(l, r, env, structs, enums, vectors, fns, hint, "^"),
        Expr::Shl(l, r) => infer_integer_bin(l, r, env, structs, enums, vectors, fns, hint, "<<"),
        Expr::Shr(l, r) => infer_integer_bin(l, r, env, structs, enums, vectors, fns, hint, ">>"),
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
        Expr::Closure { params, ret, body } => infer_closure_expr(
            params,
            ret.as_ref(),
            body,
            env,
            structs,
            enums,
            vectors,
            fns,
            hint,
        ),
        Expr::CallExpr { callee, args } => {
            let callee_ty = infer_expr(callee, env, structs, enums, vectors, fns, None)?;
            infer_fn_value_call(&callee_ty, args, env, structs, enums, vectors, fns)
        }
        Expr::Call { name, args } => {
            if let Some(local_ty) = env.get(name) {
                if matches!(local_ty, Ty::Fn(_, _)) {
                    return infer_fn_value_call(local_ty, args, env, structs, enums, vectors, fns);
                }
            }
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
            if name == SHA256 || name == MERKLE_LEAF_HASH {
                if args.len() != 1 {
                    return Err(format!(
                        "`{name}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                expect_byte_array_arg(name, args, 0, env, structs, enums, vectors, fns)?;
                return Ok(digest_ty());
            }
            if name == DIGEST_EQ || name == MERKLE_NODE_HASH {
                if args.len() != 2 {
                    return Err(format!(
                        "`{name}` expects exactly 2 arguments, got {}",
                        args.len()
                    ));
                }
                expect_digest_arg(name, args, 0, env, structs, enums, vectors, fns)?;
                expect_digest_arg(name, args, 1, env, structs, enums, vectors, fns)?;
                return Ok(if name == DIGEST_EQ {
                    Ty::Bool
                } else {
                    digest_ty()
                });
            }
            if name == MERKLE_ROOT {
                if args.len() != 1 {
                    return Err(format!(
                        "`{MERKLE_ROOT}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                expect_digest_array_arg(name, args, 0, false, env, structs, enums, vectors, fns)?;
                return Ok(digest_ty());
            }
            if name == MERKLE_ROOT_FROM_DATA {
                if args.len() != 1 {
                    return Err(format!(
                        "`{MERKLE_ROOT_FROM_DATA}` expects exactly 1 argument, got {}",
                        args.len()
                    ));
                }
                expect_leaf_data_array_arg(name, args, 0, env, structs, enums, vectors, fns)?;
                return Ok(digest_ty());
            }
            if name == MERKLE_VERIFY {
                if args.len() != 4 {
                    return Err(format!(
                        "`{MERKLE_VERIFY}` expects exactly 4 arguments, got {}",
                        args.len()
                    ));
                }
                expect_digest_arg(name, args, 0, env, structs, enums, vectors, fns)?;
                expect_digest_arg(name, args, 1, env, structs, enums, vectors, fns)?;
                expect_arg_ty(name, args, 2, &Ty::I32, env, structs, enums, vectors, fns)?;
                expect_digest_array_arg(name, args, 3, true, env, structs, enums, vectors, fns)?;
                return Ok(Ty::Bool);
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
            if let Some((enum_name, variant)) = split_variant_path(name) {
                if let Some(edef) = enums.get(enum_name) {
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
                    return Ok(Ty::Enum(enum_name.to_string()));
                }
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
            if name == CLONE_METHOD {
                if !args.is_empty() {
                    return Err(format!(
                        "method `{CLONE_METHOD}`: expected 0 args, got {}",
                        args.len()
                    ));
                }
                let recv_ty = infer_expr(receiver, env, structs, enums, vectors, fns, None)?;
                if !supports_clone_method(&recv_ty, structs, enums, vectors) {
                    return Err(format!(
                        "method `{CLONE_METHOD}` requires receiver type {recv_ty:?} to declare `clone`; primitive/runtime clone integration is not available yet"
                    ));
                }
                return Ok(recv_ty);
            }
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
            let Some(def) = structs.get(name) else {
                if let Some((enum_name, variant)) = split_variant_path(name) {
                    if let Some(edef) = enums.get(enum_name) {
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
                        return Ok(Ty::Enum(enum_name.to_string()));
                    }
                }
                return Err(format!("unknown struct `{name}`"));
            };
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
