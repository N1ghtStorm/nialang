use std::collections::{BTreeSet, HashMap};
use std::fmt::Write as _;

use crate::ast::{
    Block, EnumDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt, StructDef, Ty, VectorDef,
    method_symbol,
};
use crate::nia_std::{
    ALLOC, DEALLOC, LEN, MATRIX_CLONE, MATRIX_COLS, MATRIX_DROP, MATRIX_GET, MATRIX_LEN,
    MATRIX_NEW, MATRIX_REFCOUNT, MATRIX_ROWS, MATRIX_SET, OUTER, PRINTLN, REALLOC,
};
use crate::semantics::typecheck::FnSig;

/// Walks all functions and collects UTF-8 string literal payloads for module-level globals.
fn collect_string_literals_expr(e: &Expr, out: &mut BTreeSet<String>) {
    match e {
        Expr::String(s) => {
            out.insert(s.clone());
        }
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Ident(_)
        | Expr::EnumVariant { .. } => {}
        Expr::Neg(inner) | Expr::AddrOf(inner) | Expr::Deref(inner) | Expr::Field(inner, _) => {
            collect_string_literals_expr(inner, out)
        }
        Expr::Add(a, b)
        | Expr::Sub(a, b)
        | Expr::Mul(a, b)
        | Expr::VecDot(a, b)
        | Expr::Div(a, b)
        | Expr::Eq(a, b)
        | Expr::Ne(a, b)
        | Expr::Lt(a, b)
        | Expr::Le(a, b)
        | Expr::Gt(a, b)
        | Expr::Ge(a, b) => {
            collect_string_literals_expr(a, out);
            collect_string_literals_expr(b, out);
        }
        Expr::Call { args, .. } => {
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_string_literals_expr(receiver, out);
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::StructLit { fields, .. } | Expr::VectorLit { fields, .. } => {
            for (_, fe) in fields {
                collect_string_literals_expr(fe, out);
            }
        }
        Expr::AnonVectorLit(elems) => {
            for elem in elems {
                collect_string_literals_expr(elem, out);
            }
        }
        Expr::ArrayLit(elems) => {
            for elem in elems {
                collect_string_literals_expr(elem, out);
            }
        }
        Expr::EnumTuple { args, .. } => {
            for a in args {
                collect_string_literals_expr(a, out);
            }
        }
        Expr::EnumStruct { fields, .. } => {
            for (_, fe) in fields {
                collect_string_literals_expr(fe, out);
            }
        }
        Expr::Match { scrutinee, arms } => {
            collect_string_literals_expr(scrutinee, out);
            for (_, ae) in arms {
                collect_string_literals_expr(ae, out);
            }
        }
        Expr::Quant { body } => collect_string_literals_block(body, out),
        Expr::Gpu { body } => collect_string_literals_block(body, out),
        Expr::Index(a, i) => {
            collect_string_literals_expr(a, out);
            collect_string_literals_expr(i, out);
        }
    }
}

fn collect_string_literals_block(b: &Block, out: &mut BTreeSet<String>) {
    for st in &b.stmts {
        collect_string_literals_stmt(st, out);
    }
    if let Some(e) = &b.tail {
        collect_string_literals_expr(e, out);
    }
}

fn collect_string_literals_stmt(st: &Stmt, out: &mut BTreeSet<String>) {
    match st {
        Stmt::Let { init, .. } => collect_string_literals_expr(init, out),
        Stmt::Expr(e) | Stmt::Return(e) => collect_string_literals_expr(e, out),
        Stmt::Assign { target, value } => {
            collect_string_literals_expr(target, out);
            collect_string_literals_expr(value, out);
        }
        Stmt::If { cond, then_block } => {
            collect_string_literals_expr(cond, out);
            collect_string_literals_block(then_block, out);
        }
        Stmt::While { cond, body } => {
            collect_string_literals_expr(cond, out);
            collect_string_literals_block(body, out);
        }
        Stmt::Loop { body } => collect_string_literals_block(body, out),
        Stmt::For {
            start, end, body, ..
        } => {
            collect_string_literals_expr(start, out);
            collect_string_literals_expr(end, out);
            collect_string_literals_block(body, out);
        }
        Stmt::Quant { body } => collect_string_literals_block(body, out),
        Stmt::Gpu { body } => collect_string_literals_block(body, out),
        Stmt::Break => {}
    }
}

/// Returns `(literal -> @symbol)` and LLVM IR for private string globals (NUL-terminated).
fn build_string_literal_section(fns: &[FnDef]) -> (HashMap<String, String>, String) {
    let mut set = BTreeSet::new();
    for f in fns {
        collect_string_literals_block(&f.body, &mut set);
    }
    let mut map = HashMap::new();
    let mut ir = String::new();
    for (i, s) in set.into_iter().enumerate() {
        let sym = format!("nialang.strlit.{i}");
        map.insert(s.clone(), sym.clone());
        let mut bytes = s.as_bytes().to_vec();
        bytes.push(0);
        let lit = llvm_c_escape(&bytes);
        let _ = writeln!(
            ir,
            "@{} = private unnamed_addr constant [{} x i8] c\"{}\", align 1",
            sym,
            bytes.len(),
            lit
        );
    }
    if !ir.is_empty() {
        ir.push('\n');
    }
    (map, ir)
}

/// Emits complete textual LLVM module from already-validated AST.
///
/// ## Preconditions
/// Input program is expected to be successfully typechecked; codegen uses that
/// invariant and contains `unreachable!` branches for impossible typed states.
///
/// ## Emission order
/// 1. Target header.
/// 2. Std prelude (`printf` declaration and text/format globals).
/// 3. Struct LLVM type declarations.
/// 4. Function definitions.
pub fn emit_module(
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
) -> String {
    let mut out = String::new();
    out.push_str("; generated by nialang\n");
    out.push_str("target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\"\n");
    out.push_str("target triple = \"unknown-unknown-unknown\"\n\n");
    out.push_str(crate::nia_std::llvm_prelude());
    let (str_lit_syms, str_lit_ir) = build_string_literal_section(fns);
    out.push_str(&str_lit_ir);
    out.push_str(&emit_struct_print_constants(structs));
    out.push_str(&emit_vector_print_constants(vectors));
    out.push_str(&emit_enum_print_constants(enums));

    for s in structs {
        out.push_str(&struct_type_decl(s));
        out.push('\n');
    }
    for v in vectors {
        // Vector literals are lowered as `%struct.<name>` aggregates.
        // Emit matching concrete LLVM type declarations for all vectors.
        out.push_str(&vector_type_decl(v, structs));
        out.push('\n');
    }
    for e in enums {
        out.push_str(&enum_type_decl(e, structs));
        out.push('\n');
    }
    if !structs.is_empty() || !enums.is_empty() {
        out.push('\n');
    }

    for f in fns {
        out.push_str(&emit_fn(f, structs, enums, vectors, fn_sigs, &str_lit_syms));
        out.push('\n');
    }
    out
}

/// Escapes arbitrary bytes into LLVM `c"..."` literal form.
///
/// Printable ASCII is emitted directly, while control/non-ASCII bytes are emitted as `\XX`.
fn llvm_c_escape(bytes: &[u8]) -> String {
    let mut s = String::new();
    for &b in bytes {
        match b {
            b'\\' => s.push_str("\\5C"),
            b'"' => s.push_str("\\22"),
            0x20..=0x7e => s.push(char::from(b)),
            _ => {
                let _ = write!(s, "\\{:02X}", b);
            }
        }
    }
    s
}

/// Produces unique global symbol name for named-struct JSON key fragment.
fn struct_field_key_symbol(sname: &str, fname: &str) -> String {
    format!(
        "nialang.std.txt.json.key.{}.{}",
        sanitize(sname),
        sanitize(fname)
    )
}

/// Source-like spelling of a type for `println` (vector axis element type).
fn ty_print_label(t: &Ty) -> String {
    match t {
        Ty::I8 => "i8".into(),
        Ty::U8 => "u8".into(),
        Ty::I16 => "i16".into(),
        Ty::U16 => "u16".into(),
        Ty::I32 => "i32".into(),
        Ty::I64 => "i64".into(),
        Ty::U64 => "u64".into(),
        Ty::I128 => "i128".into(),
        Ty::Isize => "isize".into(),
        Ty::Usize => "usize".into(),
        Ty::U128 => "u128".into(),
        Ty::Bool => "bool".into(),
        Ty::F16 => "f16".into(),
        Ty::F32 => "f32".into(),
        Ty::F64 => "f64".into(),
        Ty::String => "string".into(),
        Ty::Array(inner, n) => format!("[{}; {}]", ty_print_label(inner), n),
        Ty::Struct(n) => n.clone(),
        Ty::Enum(n) => n.clone(),
        Ty::Ptr(inner) => format!("&{}", ty_print_label(inner)),
        Ty::Unit => "()".into(),
        Ty::Vector(n, inner) => format!("{} {}", n, ty_print_label(inner)),
        Ty::AnonVector(inner, n) => format!("{}<{}>", ty_print_label(inner), n),
        Ty::Matrix(inner, _) if matches!(inner.as_ref(), Ty::Unit) => "Matrix".into(),
        Ty::Matrix(inner, _) => format!("Matrix<{}>", ty_print_label(inner)),
    }
}

/// LLVM global holding `println` prefix between `(` and `{`: element type + ASCII space (unique per vector decl).
fn vector_print_ty_prefix_symbol(vname: &str) -> String {
    format!("nialang.std.vector.ty.{}", sanitize(vname))
}

fn enum_variant_prefix_symbol(ename: &str, vname: &str) -> String {
    format!(
        "nialang.std.txt.enumprefix.{}.{}",
        sanitize(ename),
        sanitize(vname)
    )
}

fn enum_struct_field_key_symbol(ename: &str, vname: &str, fname: &str) -> String {
    format!(
        "nialang.std.txt.enumjson.{}.{}.{}",
        sanitize(ename),
        sanitize(vname),
        sanitize(fname)
    )
}

/// Global `Variant: ` strings and JSON keys for enum struct-variant fields (for `println`).
fn emit_enum_print_constants(enums: &[EnumDef]) -> String {
    let mut out = String::new();
    for e in enums {
        for v in &e.variants {
            let prefix = format!("{}: ", v.name);
            let bytes = prefix.as_bytes();
            let sz = bytes.len() + 1;
            let lit = llvm_c_escape(bytes);
            let sym = enum_variant_prefix_symbol(&e.name, &v.name);
            writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                sym, sz, lit
            )
            .unwrap();
            if let EnumVariantFields::Struct(fs) = &v.fields {
                for (fname, _) in fs {
                    let key = format!("\"{}\": ", fname);
                    let bytes = key.as_bytes();
                    let sz = bytes.len() + 1;
                    let lit = llvm_c_escape(bytes);
                    let sym = enum_struct_field_key_symbol(&e.name, &v.name, fname);
                    writeln!(
                        out,
                        "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                        sym, sz, lit
                    )
                    .unwrap();
                }
            }
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Emits global string constants for named-struct JSON key rendering.
fn emit_struct_print_constants(structs: &[StructDef]) -> String {
    let mut out = String::new();
    for s in structs.iter().filter(|s| !s.is_tuple) {
        for (fname, _) in &s.fields {
            let key = format!("\"{}\": ", fname);
            let bytes = key.as_bytes();
            let sz = bytes.len() + 1;
            let lit = llvm_c_escape(bytes);
            let sym = struct_field_key_symbol(&s.name, fname);
            writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                sym, sz, lit
            )
            .unwrap();
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// JSON key fragments for `println` on vector values (same symbol scheme as named structs).
fn emit_vector_print_constants(vectors: &[VectorDef]) -> String {
    let mut out = String::new();
    for v in vectors {
        let ty_prefix = format!("{} ", ty_print_label(&v.ty));
        let bytes = ty_prefix.as_bytes();
        let sz = bytes.len() + 1;
        let lit = llvm_c_escape(bytes);
        let sym = vector_print_ty_prefix_symbol(&v.name);
        writeln!(
            out,
            "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
            sym, sz, lit
        )
        .unwrap();

        for fname in &v.fields {
            let key = format!("\"{fname}\": ");
            let bytes = key.as_bytes();
            let sz = bytes.len() + 1;
            let lit = llvm_c_escape(bytes);
            let sym = struct_field_key_symbol(&v.name, fname);
            writeln!(
                out,
                "@{} = private unnamed_addr constant [{} x i8] c\"{}\\00\", align 1",
                sym, sz, lit
            )
            .unwrap();
        }
    }
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Maps high-level nialang type into LLVM IR textual type.
fn llvm_ty(t: &Ty, _structs: &[StructDef]) -> String {
    match t {
        Ty::I8 => "i8".into(),
        Ty::U8 => "i8".into(),
        Ty::I16 => "i16".into(),
        Ty::U16 => "i16".into(),
        Ty::I32 => "i32".into(),
        Ty::I64 => "i64".into(),
        Ty::U64 => "i64".into(),
        Ty::I128 => "i128".into(),
        Ty::Isize => "i64".into(),
        Ty::Usize => "i64".into(),
        Ty::U128 => "i128".into(),
        Ty::Bool => "i1".into(),
        Ty::F16 => "half".into(),
        Ty::F32 => "float".into(),
        Ty::F64 => "double".into(),
        Ty::String => "ptr".into(),
        Ty::Array(elem, n) => format!("[{} x {}]", n, llvm_ty(elem, _structs)),
        Ty::Struct(n) => format!("%struct.{}", sanitize(n)),
        Ty::Enum(n) => format!("%enum.{}", sanitize(n)),
        Ty::Ptr(_) => "ptr".into(),
        Ty::Unit => "void".into(),
        Ty::Vector(n, _) => format!("%struct.{}", sanitize(n)),
        Ty::AnonVector(elem, n) => format!("[{} x {}]", n, llvm_ty(elem, _structs)),
        Ty::Matrix(_, _) => "ptr".into(),
    }
}

/// Signed integer types use `icmp slt` for `..` range checks; unsigned use `icmp ult`.
fn int_ty_signed(t: &Ty) -> bool {
    matches!(
        t,
        Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize
    )
}

fn is_float_ty(t: &Ty) -> bool {
    matches!(t, Ty::F16 | Ty::F32 | Ty::F64)
}

/// Emits LLVM `%struct.Name = type { ... }` declaration.
fn struct_type_decl(s: &StructDef) -> String {
    let parts: Vec<String> = s.fields.iter().map(|(_, t)| llvm_ty(t, &[])).collect();
    format!(
        "%struct.{} = type {{ {} }}",
        sanitize(&s.name),
        parts.join(", ")
    )
}

fn vector_type_decl(v: &VectorDef, structs: &[StructDef]) -> String {
    let elem = llvm_ty(&v.ty, structs);
    let parts = vec![elem; v.fields.len()];
    format!(
        "%struct.{} = type {{ {} }}",
        sanitize(&v.name),
        parts.join(", ")
    )
}

fn enum_variant_payload_ty(v: &crate::ast::EnumVariantDef, structs: &[StructDef]) -> String {
    match &v.fields {
        EnumVariantFields::Unit => "i8".into(),
        EnumVariantFields::Tuple(ts) => {
            if ts.len() == 1 {
                llvm_ty(&ts[0], structs)
            } else {
                let inner = ts
                    .iter()
                    .map(|t| llvm_ty(t, structs))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("{{ {inner} }}")
            }
        }
        EnumVariantFields::Struct(fs) => {
            let inner = fs
                .iter()
                .map(|(_, t)| llvm_ty(t, structs))
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {inner} }}")
        }
    }
}

fn enum_type_decl(e: &EnumDef, structs: &[StructDef]) -> String {
    let mut parts = vec!["i32".to_string()];
    for v in &e.variants {
        parts.push(enum_variant_payload_ty(v, structs));
    }
    format!(
        "%enum.{} = type {{ {} }}",
        sanitize(&e.name),
        parts.join(", ")
    )
}

/// Sanitizes user-defined names for safe LLVM symbol usage.
fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// `root[i][j]...` as a chain ending in a root lvalue expression.
fn collect_array_index_chain(mut e: &Expr) -> Option<(&Expr, Vec<&Expr>)> {
    let mut idxs = Vec::new();
    loop {
        match e {
            Expr::Index(a, i) => {
                idxs.push(i.as_ref());
                e = a.as_ref();
            }
            Expr::Ident(_) | Expr::Deref(_) => {
                idxs.reverse();
                return Some((e, idxs));
            }
            _ => return None,
        }
    }
}

fn method_receiver_owner_ty(t: &Ty) -> &Ty {
    match t {
        Ty::Ptr(inner) => inner.as_ref(),
        _ => t,
    }
}

fn matrix_elem_size(t: &Ty) -> usize {
    match t {
        Ty::I8 | Ty::U8 => 1,
        Ty::I16 | Ty::U16 | Ty::F16 => 2,
        Ty::I32 | Ty::F32 => 4,
        Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize | Ty::F64 => 8,
        Ty::I128 | Ty::U128 => 16,
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

fn matrix_binop_label(op: &str) -> &'static str {
    match op {
        "+" => "add",
        "-" => "sub",
        "*" => "mul",
        _ => unreachable!("typechecked matrix operator"),
    }
}

fn matrix_int_binop_instruction(op: &str) -> &'static str {
    match op {
        "+" => "add",
        "-" => "sub",
        "*" => "mul",
        _ => unreachable!("typechecked matrix operator"),
    }
}

fn matrix_int_div_instruction(elem_ty: &Ty) -> &'static str {
    match elem_ty {
        Ty::I8 | Ty::I16 | Ty::I32 | Ty::I64 | Ty::I128 | Ty::Isize => "sdiv",
        Ty::U8 | Ty::U16 | Ty::U64 | Ty::Usize | Ty::U128 => "udiv",
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

fn matrix_float_binop_instruction(op: &str) -> &'static str {
    match op {
        "+" => "fadd",
        "-" => "fsub",
        "*" => "fmul",
        "/" => "fdiv",
        _ => unreachable!("typechecked matrix operator"),
    }
}

fn matrix_zero_value(t: &Ty) -> &'static str {
    match t {
        Ty::I8
        | Ty::U8
        | Ty::I16
        | Ty::U16
        | Ty::I32
        | Ty::I64
        | Ty::U64
        | Ty::Isize
        | Ty::Usize
        | Ty::I128
        | Ty::U128 => "0",
        Ty::F16 | Ty::F32 | Ty::F64 => "0.0",
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

fn matrix_one_value(t: &Ty) -> &'static str {
    match t {
        Ty::I8
        | Ty::U8
        | Ty::I16
        | Ty::U16
        | Ty::I32
        | Ty::I64
        | Ty::U64
        | Ty::Isize
        | Ty::Usize
        | Ty::I128
        | Ty::U128 => "1",
        Ty::F16 | Ty::F32 | Ty::F64 => "1.0",
        _ => unreachable!("typechecked numeric matrix element"),
    }
}

struct Gen<'a> {
    structs: &'a [StructDef],
    enums: &'a [EnumDef],
    vectors: &'a [VectorDef],
    fn_sigs: &'a HashMap<String, FnSig>,
    str_lit_syms: &'a HashMap<String, String>,
    tmp: u32,
    lbl: u32,
    out: String,
    terminated: bool,
    /// Labels of `loop.exit` for nested `loop`; `break` branches to the top.
    loop_exit_stack: Vec<String>,
}

impl<'a> Gen<'a> {
    /// Constructs function-level codegen state.
    fn new(
        structs: &'a [StructDef],
        enums: &'a [EnumDef],
        vectors: &'a [VectorDef],
        fn_sigs: &'a HashMap<String, FnSig>,
        str_lit_syms: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            structs,
            enums,
            vectors,
            fn_sigs,
            str_lit_syms,
            tmp: 0,
            lbl: 0,
            out: String::new(),
            terminated: false,
            loop_exit_stack: Vec::new(),
        }
    }

    /// Allocates fresh SSA temporary id.
    fn fresh(&mut self) -> String {
        let n = self.tmp;
        self.tmp += 1;
        format!("t{n}")
    }

    /// Allocates fresh basic-block label id.
    fn fresh_label(&mut self, prefix: &str) -> String {
        let n = self.lbl;
        self.lbl += 1;
        format!("{prefix}.{n}")
    }

    /// Emits pointer to first byte of global string constant.
    fn fmt_ptr(&mut self, sym: &str, size: u32) -> String {
        let tmp = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds [{} x i8], ptr @{}, i64 0, i64 0",
            tmp, size, sym
        )
        .unwrap();
        format!("%{tmp}")
    }

    /// Emits plain `printf` call for constant text fragment.
    fn emit_printf_text(&mut self, sym: &str, size: u32) {
        let p = self.fmt_ptr(sym, size);
        writeln!(self.out, "  call i32 (ptr, ...) @printf(ptr {})", p).unwrap();
    }

    /// Convenience wrapper for dynamic known-at-runtime `usize` sizes.
    fn emit_printf_text_dynamic(&mut self, sym: &str, size: usize) {
        self.emit_printf_text(sym, size as u32);
    }

    /// Looks up struct definition by source-level name.
    fn struct_def(&self, name: &str) -> Option<&StructDef> {
        self.structs.iter().find(|s| s.name == name)
    }

    fn enum_tag(&self, enum_name: &str, variant: &str) -> Option<i32> {
        let e = self.enums.iter().find(|e| e.name == enum_name)?;
        e.variants
            .iter()
            .position(|v| v.name == variant)
            .map(|i| i as i32)
    }

    fn enum_variant_index(&self, enum_name: &str, variant: &str) -> Option<usize> {
        let e = self.enums.iter().find(|e| e.name == enum_name)?;
        e.variants.iter().position(|v| v.name == variant)
    }

    fn enum_variant_def(
        &self,
        enum_name: &str,
        variant: &str,
    ) -> Option<&crate::ast::EnumVariantDef> {
        let e = self.enums.iter().find(|e| e.name == enum_name)?;
        e.variants.iter().find(|v| v.name == variant)
    }

    fn emit_sizeof_i64(&mut self, ty: &Ty) -> String {
        let ty_ll = llvm_ty(ty, self.structs);
        let gep = self.fresh();
        let sz = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr {}, ptr null, i64 1",
            gep, ty_ll
        )
        .unwrap();
        writeln!(self.out, "  %{} = ptrtoint ptr %{} to i64", sz, gep).unwrap();
        format!("%{sz}")
    }

    fn matrix_field_ptr(&mut self, matrix: &str, field: u32) -> String {
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {{ i64, ptr, i64, i64 }}, ptr {}, i32 0, i32 {}",
            out, matrix, field
        )
        .unwrap();
        format!("%{out}")
    }

    fn matrix_load_i64_field(&mut self, matrix: &str, field: u32) -> String {
        let ptr = self.matrix_field_ptr(matrix, field);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn matrix_load_data_ptr(&mut self, matrix: &str) -> String {
        let ptr = self.matrix_field_ptr(matrix, 1);
        let out = self.fresh();
        writeln!(self.out, "  %{} = load ptr, ptr {}", out, ptr).unwrap();
        format!("%{out}")
    }

    fn matrix_cell_ptr(&mut self, matrix: &str, row: &str, col: &str, elem_ty: &Ty) -> String {
        let cols = self.matrix_load_i64_field(matrix, 3);
        let row64 = self.fresh();
        let col64 = self.fresh();
        let row_offset = self.fresh();
        let index = self.fresh();
        let data = self.matrix_load_data_ptr(matrix);
        let cell = self.fresh();
        writeln!(self.out, "  %{} = sext i32 {} to i64", row64, row).unwrap();
        writeln!(self.out, "  %{} = sext i32 {} to i64", col64, col).unwrap();
        writeln!(self.out, "  %{} = mul i64 %{}, {}", row_offset, row64, cols).unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            index, row_offset, col64
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            data,
            index
        )
        .unwrap();
        format!("%{cell}")
    }

    fn matrix_data_cell_ptr(
        &mut self,
        data: &str,
        n: &str,
        row: &str,
        col: &str,
        elem_ty: &Ty,
    ) -> String {
        let row_offset = self.fresh();
        let index = self.fresh();
        let cell = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", row_offset, row, n).unwrap();
        writeln!(self.out, "  %{} = add i64 %{}, {}", index, row_offset, col).unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            data,
            index
        )
        .unwrap();
        format!("%{cell}")
    }

    /// Emits primitive scalar printing (`int`, `bool`, pointer) with optional newline.
    ///
    /// This function centralizes ABI-sensitive formatting:
    /// - sign/zero extension for small integers,
    /// - `%lld`/`%llu` for 64-bit lanes,
    /// - split hi/lo printing for 128-bit values,
    /// - `ptrtoint` for pointer hex output.
    fn emit_print_primitive(&mut self, ty: &Ty, v: &str, newline: bool) {
        match ty {
            Ty::I8 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i32", 4)
                } else {
                    ("nialang.std.fmt.i32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = sext i8 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::U8 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = zext i8 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::I16 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i32", 4)
                } else {
                    ("nialang.std.fmt.i32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = sext i16 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::U16 => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = zext i16 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::I32 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i32", 4)
                } else {
                    ("nialang.std.fmt.i32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::I64 | Ty::Isize => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i64", 6)
                } else {
                    ("nialang.std.fmt.i64.nn", 5)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::U64 | Ty::Usize => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u64", 6)
                } else {
                    ("nialang.std.fmt.u64.nn", 5)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 {})",
                    p, v
                )
                .unwrap();
            }
            Ty::Bool => {
                let t = self.fresh();
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.u32", 4)
                } else {
                    ("nialang.std.fmt.u32.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(self.out, "  %{} = zext i1 {} to i32", t, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i32 %{})",
                    p, t
                )
                .unwrap();
            }
            Ty::String => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.str", 4)
                } else {
                    ("nialang.std.fmt.str.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, ptr {})",
                    p, v
                )
                .unwrap();
            }
            Ty::I128 | Ty::U128 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.i128hex", 18)
                } else {
                    ("nialang.std.fmt.i128hex.nn", 17)
                };
                let p = self.fmt_ptr(sym, sz);
                let hi128 = self.fresh();
                let hi64 = self.fresh();
                let lo64 = self.fresh();
                writeln!(self.out, "  %{} = lshr i128 {}, 64", hi128, v).unwrap();
                writeln!(self.out, "  %{} = trunc i128 %{} to i64", hi64, hi128).unwrap();
                writeln!(self.out, "  %{} = trunc i128 {} to i64", lo64, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 %{}, i64 %{})",
                    p, hi64, lo64
                )
                .unwrap();
            }
            Ty::Ptr(_) => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.ptrhex", 8)
                } else {
                    ("nialang.std.fmt.ptrhex.nn", 7)
                };
                let p = self.fmt_ptr(sym, sz);
                let addr = self.fresh();
                writeln!(self.out, "  %{} = ptrtoint ptr {} to i64", addr, v).unwrap();
                writeln!(
                    self.out,
                    "  call i32 (ptr, ...) @printf(ptr {}, i64 %{})",
                    p, addr
                )
                .unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let (sym, sz) = if newline {
                    ("nialang.std.fmt.f64", 4)
                } else {
                    ("nialang.std.fmt.f64.nn", 3)
                };
                let p = self.fmt_ptr(sym, sz);
                match ty {
                    Ty::F64 => {
                        writeln!(
                            self.out,
                            "  call i32 (ptr, ...) @printf(ptr {}, double {})",
                            p, v
                        )
                        .unwrap();
                    }
                    Ty::F32 => {
                        let e = self.fresh();
                        writeln!(self.out, "  %{} = fpext float {} to double", e, v).unwrap();
                        writeln!(
                            self.out,
                            "  call i32 (ptr, ...) @printf(ptr {}, double %{})",
                            p, e
                        )
                        .unwrap();
                    }
                    Ty::F16 => {
                        let e = self.fresh();
                        writeln!(self.out, "  %{} = fpext half {} to double", e, v).unwrap();
                        writeln!(
                            self.out,
                            "  call i32 (ptr, ...) @printf(ptr {}, double %{})",
                            p, e
                        )
                        .unwrap();
                    }
                    _ => unreachable!(),
                }
            }
            Ty::Array(_, _)
            | Ty::Struct(_)
            | Ty::Enum(_)
            | Ty::Unit
            | Ty::Vector(_, _)
            | Ty::AnonVector(_, _)
            | Ty::Matrix(_, _) => unreachable!("typechecked"),
        }
    }

    /// Emits array printing in `[a, b, c]` style with optional trailing newline.
    ///
    /// Elements are extracted using `extractvalue` and recursively rendered
    /// via `emit_print_value`, so nested printable composites are supported.
    fn emit_print_array(&mut self, elem_ty: &Ty, n: usize, arr_v: &str, newline: bool) {
        self.emit_printf_text("nialang.std.txt.arr_open", 2);
        for i in 0..n {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let ev = self.fresh();
            let llvm_arr = format!("[{} x {}]", n, llvm_ty(elem_ty, self.structs));
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ev, llvm_arr, arr_v, i
            )
            .unwrap();
            self.emit_print_value(elem_ty, &format!("%{ev}"), false);
        }
        if newline {
            self.emit_printf_text("nialang.std.txt.arr_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.arr_close", 2);
        }
    }

    /// Emits struct printing in two source-level forms:
    /// - named structs -> JSON-like object form (`{"x": 1, "y": 2}`)
    /// - tuple structs -> tuple form (`(1, 2)`)
    ///
    /// Field values are pulled from aggregate SSA value via `extractvalue` and then
    /// recursively rendered according to each field type.
    fn emit_print_struct(&mut self, sname: &str, struct_v: &str, newline: bool) {
        let sdef = self.struct_def(sname).expect("typechecked struct");
        let is_tuple = sdef.is_tuple;
        let fields = sdef.fields.clone();
        if is_tuple {
            self.emit_printf_text("nialang.std.txt.tuple_open", 2);
            for (i, (_, fty)) in fields.iter().enumerate() {
                if i > 0 {
                    self.emit_printf_text("nialang.std.txt.arr_sep", 3);
                }
                let fv = self.fresh();
                let llvm_st = format!("%struct.{}", sanitize(sname));
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    fv, llvm_st, struct_v, i
                )
                .unwrap();
                self.emit_print_value(fty, &format!("%{fv}"), false);
            }
            if newline {
                self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
            } else {
                self.emit_printf_text("nialang.std.txt.tuple_close", 2);
            }
            return;
        }

        self.emit_printf_text("nialang.std.txt.obj_open", 2);
        for (i, (fname, fty)) in fields.iter().enumerate() {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let key_sym = struct_field_key_symbol(sname, fname);
            let key_sz = fname.as_bytes().len() + 5;
            self.emit_printf_text_dynamic(&key_sym, key_sz);
            let fv = self.fresh();
            let llvm_st = format!("%struct.{}", sanitize(sname));
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                fv, llvm_st, struct_v, i
            )
            .unwrap();
            self.emit_print_value(fty, &format!("%{fv}"), false);
        }
        if newline {
            self.emit_printf_text("nialang.std.txt.obj_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.obj_close", 2);
        }
    }

    /// Vector values use `%struct.<name>` aggregates; print as `(Name {"X": …})`.
    fn emit_print_vector(&mut self, vname: &str, vec_v: &str, newline: bool) {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector");
        let fty = vdef.ty.clone();
        let llvm_st = format!("%struct.{}", sanitize(vname));
        self.emit_printf_text("nialang.std.txt.tuple_open", 2);
        let ty_sym = vector_print_ty_prefix_symbol(vname);
        let ty_sz = format!("{} ", ty_print_label(&vdef.ty)).as_bytes().len() + 1;
        self.emit_printf_text_dynamic(&ty_sym, ty_sz);
        self.emit_printf_text("nialang.std.txt.obj_open", 2);
        for (i, fname) in vdef.fields.iter().enumerate() {
            if i > 0 {
                self.emit_printf_text("nialang.std.txt.arr_sep", 3);
            }
            let key_sym = struct_field_key_symbol(vname, fname);
            let key_sz = fname.as_bytes().len() + 5;
            self.emit_printf_text_dynamic(&key_sym, key_sz);
            let fv = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                fv, llvm_st, vec_v, i
            )
            .unwrap();
            self.emit_print_value(&fty, &format!("%{fv}"), false);
        }
        self.emit_printf_text("nialang.std.txt.obj_close", 2);
        if newline {
            self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.tuple_close", 2);
        }
    }

    /// Prints enum as `Variant: <payload>` (unit: `Variant: ()` with newline when requested).
    fn emit_print_enum(&mut self, ename: &str, ev: &str, newline: bool) {
        let edef = self
            .enums
            .iter()
            .find(|e| e.name == ename)
            .expect("typechecked");
        let enum_ll = format!("%enum.{}", sanitize(ename));
        let tag = self.fresh();
        writeln!(self.out, "  %{} = extractvalue {} {}, 0", tag, enum_ll, ev).unwrap();
        let default_lbl = self.fresh_label("println.enum.default");
        let cont_lbl = self.fresh_label("println.enum.cont");
        let arm_lbls: Vec<String> = edef
            .variants
            .iter()
            .map(|_| self.fresh_label("println.enum.arm"))
            .collect();
        writeln!(self.out, "  switch i32 %{}, label %{} [", tag, default_lbl).unwrap();
        for (i, _) in edef.variants.iter().enumerate() {
            writeln!(self.out, "    i32 {}, label %{}", i as i32, arm_lbls[i]).unwrap();
        }
        writeln!(self.out, "  ]").unwrap();
        writeln!(self.out, "{}:", default_lbl).unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        for (i, vdef) in edef.variants.iter().enumerate() {
            writeln!(self.out, "{}:", arm_lbls[i]).unwrap();
            let sym = enum_variant_prefix_symbol(ename, &vdef.name);
            let prefix = format!("{}: ", vdef.name);
            let prefix_len = prefix.as_bytes().len() + 1;
            self.emit_printf_text_dynamic(&sym, prefix_len);
            let payload_idx = i + 1;
            match &vdef.fields {
                EnumVariantFields::Unit => {
                    self.emit_printf_text("nialang.std.txt.tuple_open", 2);
                    if newline {
                        self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
                    } else {
                        self.emit_printf_text("nialang.std.txt.tuple_close", 2);
                    }
                }
                EnumVariantFields::Tuple(ts) if ts.len() == 1 => {
                    let pl = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        pl, enum_ll, ev, payload_idx
                    )
                    .unwrap();
                    self.emit_print_value(&ts[0], &format!("%{pl}"), newline);
                }
                EnumVariantFields::Tuple(ts) => {
                    let pl = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        pl, enum_ll, ev, payload_idx
                    )
                    .unwrap();
                    let payload_ll = {
                        let inner = ts
                            .iter()
                            .map(|t| llvm_ty(t, self.structs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ {inner} }}")
                    };
                    self.emit_printf_text("nialang.std.txt.tuple_open", 2);
                    for j in 0..ts.len() {
                        if j > 0 {
                            self.emit_printf_text("nialang.std.txt.arr_sep", 3);
                        }
                        let fv = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = extractvalue {} %{}, {}",
                            fv, payload_ll, pl, j
                        )
                        .unwrap();
                        self.emit_print_value(&ts[j], &format!("%{fv}"), false);
                    }
                    if newline {
                        self.emit_printf_text("nialang.std.txt.tuple_close_ln", 3);
                    } else {
                        self.emit_printf_text("nialang.std.txt.tuple_close", 2);
                    }
                }
                EnumVariantFields::Struct(fs) => {
                    let pl = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = extractvalue {} {}, {}",
                        pl, enum_ll, ev, payload_idx
                    )
                    .unwrap();
                    let payload_ll = {
                        let inner = fs
                            .iter()
                            .map(|(_, t)| llvm_ty(t, self.structs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ {inner} }}")
                    };
                    self.emit_printf_text("nialang.std.txt.obj_open", 2);
                    for (j, (fname, fty)) in fs.iter().enumerate() {
                        if j > 0 {
                            self.emit_printf_text("nialang.std.txt.arr_sep", 3);
                        }
                        let ksym = enum_struct_field_key_symbol(ename, &vdef.name, fname);
                        let key = format!("\"{}\": ", fname);
                        let key_sz = key.as_bytes().len() + 1;
                        self.emit_printf_text_dynamic(&ksym, key_sz);
                        let fv = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = extractvalue {} %{}, {}",
                            fv, payload_ll, pl, j
                        )
                        .unwrap();
                        self.emit_print_value(fty, &format!("%{fv}"), false);
                    }
                    if newline {
                        self.emit_printf_text("nialang.std.txt.obj_close_ln", 3);
                    } else {
                        self.emit_printf_text("nialang.std.txt.obj_close", 2);
                    }
                }
            }
            writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        }
        writeln!(self.out, "{}:", cont_lbl).unwrap();
    }

    fn emit_print_matrix_summary(&mut self, matrix: &str, newline: bool) {
        let refs = self.matrix_load_i64_field(matrix, 0);
        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 3);
        let (sym, size) = if newline {
            ("nialang.std.fmt.matrix", 41)
        } else {
            ("nialang.std.fmt.matrix.nn", 40)
        };
        let p = self.fmt_ptr(sym, size);
        writeln!(
            self.out,
            "  call i32 (ptr, ...) @printf(ptr {}, i64 {}, i64 {}, i64 {})",
            p, rows, cols, refs
        )
        .unwrap();
    }

    fn emit_print_matrix(&mut self, elem_ty: &Ty, matrix: &str, newline: bool) {
        if matches!(elem_ty, Ty::Unit) {
            self.emit_print_matrix_summary(matrix, newline);
            return;
        }

        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 3);
        let data = self.matrix_load_data_ptr(matrix);
        let row_addr = self.fresh();
        let col_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", row_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", col_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", row_addr).unwrap();

        let row_cond = self.fresh_label("println.matrix.row.cond");
        let row_body = self.fresh_label("println.matrix.row.body");
        let row_sep = self.fresh_label("println.matrix.row.sep");
        let row_item = self.fresh_label("println.matrix.row.item");
        let row_latch = self.fresh_label("println.matrix.row.latch");
        let row_done = self.fresh_label("println.matrix.row.done");
        let col_cond = self.fresh_label("println.matrix.col.cond");
        let col_body = self.fresh_label("println.matrix.col.body");
        let col_sep = self.fresh_label("println.matrix.col.sep");
        let col_item = self.fresh_label("println.matrix.col.item");
        let col_latch = self.fresh_label("println.matrix.col.latch");
        let col_done = self.fresh_label("println.matrix.col.done");

        self.emit_printf_text("nialang.std.txt.arr_open", 2);
        writeln!(self.out, "  br label %{}", row_cond).unwrap();

        writeln!(self.out, "{}:", row_cond).unwrap();
        let row = self.fresh();
        let has_row = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", row, row_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_row, row, rows).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_row, row_body, row_done
        )
        .unwrap();

        writeln!(self.out, "{}:", row_body).unwrap();
        let first_row = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 0", first_row, row).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            first_row, row_item, row_sep
        )
        .unwrap();

        writeln!(self.out, "{}:", row_sep).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_sep", 3);
        writeln!(self.out, "  br label %{}", row_item).unwrap();

        writeln!(self.out, "{}:", row_item).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_open", 2);
        writeln!(self.out, "  store i64 0, ptr %{}", col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond).unwrap();

        writeln!(self.out, "{}:", col_cond).unwrap();
        let col = self.fresh();
        let has_col = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", col, col_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_col, col, cols).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_col, col_body, col_done
        )
        .unwrap();

        writeln!(self.out, "{}:", col_body).unwrap();
        let first_col = self.fresh();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 0", first_col, col).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            first_col, col_item, col_sep
        )
        .unwrap();

        writeln!(self.out, "{}:", col_sep).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_sep", 3);
        writeln!(self.out, "  br label %{}", col_item).unwrap();

        writeln!(self.out, "{}:", col_item).unwrap();
        let row_offset = self.fresh();
        let index = self.fresh();
        let cell_ptr = self.fresh();
        let cell = self.fresh();
        writeln!(self.out, "  %{} = mul i64 %{}, {}", row_offset, row, cols).unwrap();
        writeln!(self.out, "  %{} = add i64 %{}, %{}", index, row_offset, col).unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell_ptr,
            llvm_ty(elem_ty, self.structs),
            data,
            index
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            cell,
            llvm_ty(elem_ty, self.structs),
            cell_ptr
        )
        .unwrap();
        self.emit_print_value(elem_ty, &format!("%{cell}"), false);
        writeln!(self.out, "  br label %{}", col_latch).unwrap();

        writeln!(self.out, "{}:", col_latch).unwrap();
        let col_next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", col_next, col).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", col_next, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond).unwrap();

        writeln!(self.out, "{}:", col_done).unwrap();
        self.emit_printf_text("nialang.std.txt.arr_close", 2);
        writeln!(self.out, "  br label %{}", row_latch).unwrap();

        writeln!(self.out, "{}:", row_latch).unwrap();
        let row_next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", row_next, row).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", row_next, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond).unwrap();

        writeln!(self.out, "{}:", row_done).unwrap();
        if newline {
            self.emit_printf_text("nialang.std.txt.arr_close_ln", 3);
        } else {
            self.emit_printf_text("nialang.std.txt.arr_close", 2);
        }
    }

    /// Recursive dispatcher for printable value lowering.
    ///
    /// Keeps all `println` formatting behavior in one place and ensures nested
    /// composites (array/struct of array/struct/primitive/pointer) print consistently.
    fn emit_print_value(&mut self, ty: &Ty, v: &str, newline: bool) {
        match ty {
            Ty::Array(elem, n) => {
                self.emit_print_array(elem, *n, v, newline);
            }
            Ty::Struct(sname) => {
                if self.struct_def(sname).is_some() {
                    self.emit_print_struct(sname, v, newline);
                } else {
                    self.emit_print_vector(sname, v, newline);
                }
            }
            Ty::Vector(vname, _) => {
                self.emit_print_vector(vname, v, newline);
            }
            Ty::AnonVector(elem_ty, n) => {
                self.emit_print_array(elem_ty, *n, v, newline);
            }
            Ty::Enum(ename) => {
                self.emit_print_enum(ename, v, newline);
            }
            Ty::Matrix(elem_ty, _) => {
                self.emit_print_matrix(elem_ty, v, newline);
            }
            _ => self.emit_print_primitive(ty, v, newline),
        }
    }

    fn emit_matrix_new(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (src_ty, src_val) = self.emit_expr(&args[0], locals, None);
        let Ty::Array(row_ty, rows) = src_ty else {
            unreachable!("typechecked matrix source")
        };
        let Ty::Array(cell_ty, cols) = row_ty.as_ref() else {
            unreachable!("typechecked matrix source")
        };
        let len = rows * cols;
        let bytes = len * matrix_elem_size(cell_ty);
        let data = self.fresh();
        let matrix = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 32)", matrix).unwrap();

        let rc_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 0);
        let data_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 1);
        let rows_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 2);
        let cols_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 3);
        writeln!(self.out, "  store i64 1, ptr {}", rc_ptr).unwrap();
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", rows, rows_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", cols, cols_ptr).unwrap();

        let src_ll = llvm_ty(
            &Ty::Array(Box::new(Ty::Array(cell_ty.clone(), *cols)), rows),
            self.structs,
        );
        let row_ll = llvm_ty(&Ty::Array(cell_ty.clone(), *cols), self.structs);
        for row in 0..rows {
            let row_val = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                row_val, src_ll, src_val, row
            )
            .unwrap();
            for col in 0..*cols {
                let raw_cell = self.fresh();
                let flat = row * *cols + col;
                let cell_ptr = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} %{}, {}",
                    raw_cell, row_ll, row_val, col
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr %{}, i64 {}",
                    cell_ptr,
                    llvm_ty(cell_ty, self.structs),
                    data,
                    flat
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store {} %{}, ptr %{}",
                    llvm_ty(cell_ty, self.structs),
                    raw_cell,
                    cell_ptr
                )
                .unwrap();
            }
        }

        (
            Ty::Matrix(cell_ty.clone(), Some((rows, *cols))),
            format!("%{matrix}"),
        )
    }

    fn emit_matrix_clone(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (matrix_ty, matrix) = self.emit_expr(&args[0], locals, None);
        let rc_ptr = self.matrix_field_ptr(&matrix, 0);
        let old = self.fresh();
        let new = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr {}", old, rc_ptr).unwrap();
        writeln!(self.out, "  %{} = add i64 %{}, 1", new, old).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr {}", new, rc_ptr).unwrap();
        (matrix_ty, matrix)
    }

    fn emit_matrix_drop(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (_, matrix) = self.emit_expr(&args[0], locals, None);
        let rc_ptr = self.matrix_field_ptr(&matrix, 0);
        let old = self.fresh();
        let new = self.fresh();
        let is_zero = self.fresh();
        let free_lbl = self.fresh_label("matrix.drop.free");
        let cont_lbl = self.fresh_label("matrix.drop.cont");
        writeln!(self.out, "  %{} = load i64, ptr {}", old, rc_ptr).unwrap();
        writeln!(self.out, "  %{} = sub i64 %{}, 1", new, old).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr {}", new, rc_ptr).unwrap();
        writeln!(self.out, "  %{} = icmp eq i64 %{}, 0", is_zero, new).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            is_zero, free_lbl, cont_lbl
        )
        .unwrap();
        writeln!(self.out, "{}:", free_lbl).unwrap();
        let data = self.matrix_load_data_ptr(&matrix);
        writeln!(self.out, "  call void @free(ptr {})", data).unwrap();
        writeln!(self.out, "  call void @free(ptr {})", matrix).unwrap();
        writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
        writeln!(self.out, "{}:", cont_lbl).unwrap();
        (Ty::Unit, String::new())
    }

    fn emit_matrix_get(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (matrix_ty, matrix) = self.emit_expr(&args[0], locals, None);
        let Ty::Matrix(elem_ty, _) = matrix_ty else {
            unreachable!("typechecked matrix_get")
        };
        let (_, row) = self.emit_expr(&args[1], locals, Some(&Ty::I32));
        let (_, col) = self.emit_expr(&args[2], locals, Some(&Ty::I32));
        let cell = self.matrix_cell_ptr(&matrix, &row, &col, &elem_ty);
        let out = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            out,
            llvm_ty(&elem_ty, self.structs),
            cell
        )
        .unwrap();
        ((*elem_ty).clone(), format!("%{out}"))
    }

    fn emit_matrix_set(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (matrix_ty, matrix) = self.emit_expr(&args[0], locals, None);
        let Ty::Matrix(elem_ty, _) = matrix_ty else {
            unreachable!("typechecked matrix_set")
        };
        let (_, row) = self.emit_expr(&args[1], locals, Some(&Ty::I32));
        let (_, col) = self.emit_expr(&args[2], locals, Some(&Ty::I32));
        let (value_ty, value) = self.emit_expr(&args[3], locals, Some(&elem_ty));
        debug_assert!(types_match(&value_ty, &elem_ty));
        let cell = self.matrix_cell_ptr(&matrix, &row, &col, &elem_ty);
        writeln!(
            self.out,
            "  store {} {}, ptr {}",
            llvm_ty(&elem_ty, self.structs),
            value,
            cell
        )
        .unwrap();
        (Ty::Unit, String::new())
    }

    fn emit_matrix_info(
        &mut self,
        name: &str,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (_, matrix) = self.emit_expr(&args[0], locals, None);
        let value64 = if name == MATRIX_ROWS {
            self.matrix_load_i64_field(&matrix, 2)
        } else if name == MATRIX_COLS {
            self.matrix_load_i64_field(&matrix, 3)
        } else if name == MATRIX_REFCOUNT {
            self.matrix_load_i64_field(&matrix, 0)
        } else {
            let rows = self.matrix_load_i64_field(&matrix, 2);
            let cols = self.matrix_load_i64_field(&matrix, 3);
            let len = self.fresh();
            writeln!(self.out, "  %{} = mul i64 {}, {}", len, rows, cols).unwrap();
            format!("%{len}")
        };
        let out = self.fresh();
        writeln!(self.out, "  %{} = trunc i64 {} to i32", out, value64).unwrap();
        (Ty::I32, format!("%{out}"))
    }

    fn emit_matrix_elem_binop(
        &mut self,
        elem_ty: &Ty,
        left: &str,
        right: &str,
        op: &str,
    ) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                let llvm_op = matrix_int_binop_instruction(op);
                writeln!(self.out, "  %{} = {} i8 {}, {}", out, llvm_op, left, right).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                let llvm_op = matrix_int_binop_instruction(op);
                writeln!(self.out, "  %{} = {} i16 {}, {}", out, llvm_op, left, right).unwrap();
            }
            Ty::I32 => {
                if op == "/" {
                    let llvm_op = matrix_int_div_instruction(elem_ty);
                    writeln!(self.out, "  %{} = {} i32 {}, {}", out, llvm_op, left, right).unwrap();
                } else {
                    let llvm_op = matrix_int_binop_instruction(op);
                    writeln!(
                        self.out,
                        "  %{} = {} nsw i32 {}, {}",
                        out, llvm_op, left, right
                    )
                    .unwrap();
                }
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                if op == "/" {
                    let llvm_op = matrix_int_div_instruction(elem_ty);
                    writeln!(self.out, "  %{} = {} i64 {}, {}", out, llvm_op, left, right).unwrap();
                } else {
                    let llvm_op = matrix_int_binop_instruction(op);
                    writeln!(self.out, "  %{} = {} i64 {}, {}", out, llvm_op, left, right).unwrap();
                }
            }
            Ty::I128 | Ty::U128 => {
                let llvm_op = matrix_int_binop_instruction(op);
                writeln!(
                    self.out,
                    "  %{} = {} i128 {}, {}",
                    out, llvm_op, left, right
                )
                .unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                let llvm_op = matrix_float_binop_instruction(op);
                writeln!(
                    self.out,
                    "  %{} = {} {} {}, {}",
                    out, llvm_op, ll, left, right
                )
                .unwrap();
            }
            _ => unreachable!("typechecked numeric matrix element"),
        }
        format!("%{out}")
    }

    fn emit_matrix_elem_div(&mut self, elem_ty: &Ty, left: &str, right: &str) -> String {
        self.emit_matrix_elem_binop(elem_ty, left, right, "/")
    }

    fn emit_matrix_elem_neg(&mut self, elem_ty: &Ty, value: &str) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                writeln!(self.out, "  %{} = sub i8 0, {}", out, value).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                writeln!(self.out, "  %{} = sub i16 0, {}", out, value).unwrap();
            }
            Ty::I32 => {
                writeln!(self.out, "  %{} = sub nsw i32 0, {}", out, value).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(self.out, "  %{} = sub i64 0, {}", out, value).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                writeln!(self.out, "  %{} = sub i128 0, {}", out, value).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(self.out, "  %{} = fneg {} {}", out, ll, value).unwrap();
            }
            _ => unreachable!("typechecked numeric matrix element"),
        }
        format!("%{out}")
    }

    /// Returns an `i1` SSA value: true when `value` is numerically zero.
    fn emit_matrix_elem_is_zero(&mut self, elem_ty: &Ty, value: &str) -> String {
        let out = self.fresh();
        if is_float_ty(elem_ty) {
            let ll = llvm_ty(elem_ty, self.structs);
            writeln!(
                self.out,
                "  %{} = fcmp oeq {} {}, {}",
                out,
                ll,
                value,
                matrix_zero_value(elem_ty)
            )
            .unwrap();
        } else {
            let ll = llvm_ty(elem_ty, self.structs);
            writeln!(
                self.out,
                "  %{} = icmp eq {} {}, {}",
                out,
                ll,
                value,
                matrix_zero_value(elem_ty)
            )
            .unwrap();
        }
        format!("%{out}")
    }

    fn emit_matrix_binop(
        &mut self,
        left: &str,
        right: &str,
        elem_ty: &Ty,
        shape: Option<(usize, usize)>,
        op: &str,
    ) -> (Ty, String) {
        let label_op = matrix_binop_label(op);
        let left_rows = self.matrix_load_i64_field(left, 2);
        let left_cols = self.matrix_load_i64_field(left, 3);
        let right_rows = self.matrix_load_i64_field(right, 2);
        let right_cols = self.matrix_load_i64_field(right, 3);
        let rows_match = self.fresh();
        let cols_match = self.fresh();
        let shape_match = self.fresh();
        let ok_lbl = self.fresh_label(&format!("matrix.{label_op}.shape.ok"));
        let abort_lbl = self.fresh_label(&format!("matrix.{label_op}.shape.abort"));
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            rows_match, left_rows, right_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            cols_match, left_cols, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = and i1 %{}, %{}",
            shape_match, rows_match, cols_match
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            shape_match, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        let len = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 {}, {}",
            len, left_rows, left_cols
        )
        .unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        let matrix = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 32)", matrix).unwrap();

        let rc_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 0);
        let data_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 1);
        let rows_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 2);
        let cols_ptr = self.matrix_field_ptr(&format!("%{matrix}"), 3);
        writeln!(self.out, "  store i64 1, ptr {}", rc_ptr).unwrap();
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", left_rows, rows_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", left_cols, cols_ptr).unwrap();

        let left_data = self.matrix_load_data_ptr(left);
        let right_data = self.matrix_load_data_ptr(right);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label(&format!("matrix.{label_op}.cond"));
        let body_lbl = self.fresh_label(&format!("matrix.{label_op}.body"));
        let latch_lbl = self.fresh_label(&format!("matrix.{label_op}.latch"));
        let done_lbl = self.fresh_label(&format!("matrix.{label_op}.done"));
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            has_item, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let ll = llvm_ty(elem_ty, self.structs);
        let left_cell_ptr = self.fresh();
        let right_cell_ptr = self.fresh();
        let out_cell_ptr = self.fresh();
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            left_cell_ptr, ll, left_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            right_cell_ptr, ll, right_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell_ptr, ll, data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            left_cell, ll, left_cell_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            right_cell, ll, right_cell_ptr
        )
        .unwrap();
        let value = self.emit_matrix_elem_binop(
            elem_ty,
            &format!("%{left_cell}"),
            &format!("%{right_cell}"),
            op,
        );
        writeln!(self.out, "  store {} {}, ptr %{}", ll, value, out_cell_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (
            Ty::Matrix(Box::new(elem_ty.clone()), shape),
            format!("%{matrix}"),
        )
    }

    fn emit_matrix_scalar_mul(
        &mut self,
        matrix: &str,
        scalar: &str,
        elem_ty: &Ty,
        shape: Option<(usize, usize)>,
    ) -> (Ty, String) {
        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 3);
        let len = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", len, rows, cols).unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        let out_matrix = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 32)", out_matrix).unwrap();

        let rc_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 0);
        let data_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 1);
        let rows_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 2);
        let cols_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 3);
        writeln!(self.out, "  store i64 1, ptr {}", rc_ptr).unwrap();
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", rows, rows_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", cols, cols_ptr).unwrap();

        let matrix_data = self.matrix_load_data_ptr(matrix);
        let idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", idx_addr).unwrap();

        let cond_lbl = self.fresh_label("matrix.scalar.mul.cond");
        let body_lbl = self.fresh_label("matrix.scalar.mul.body");
        let latch_lbl = self.fresh_label("matrix.scalar.mul.latch");
        let done_lbl = self.fresh_label("matrix.scalar.mul.done");
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", cond_lbl).unwrap();
        let idx = self.fresh();
        let has_item = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", idx, idx_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            has_item, idx, len
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_item, body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", body_lbl).unwrap();
        let ll = llvm_ty(elem_ty, self.structs);
        let cell_ptr = self.fresh();
        let out_cell_ptr = self.fresh();
        let cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            cell_ptr, ll, matrix_data, idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell_ptr, ll, data, idx
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", cell, ll, cell_ptr).unwrap();
        let value = self.emit_matrix_elem_binop(elem_ty, &format!("%{cell}"), scalar, "*");
        writeln!(self.out, "  store {} {}, ptr %{}", ll, value, out_cell_ptr).unwrap();
        writeln!(self.out, "  br label %{}", latch_lbl).unwrap();

        writeln!(self.out, "{}:", latch_lbl).unwrap();
        let next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next, idx).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next, idx_addr).unwrap();
        writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (
            Ty::Matrix(Box::new(elem_ty.clone()), shape),
            format!("%{out_matrix}"),
        )
    }

    fn emit_matrix_matmul(
        &mut self,
        left: &str,
        right: &str,
        elem_ty: &Ty,
        shape: Option<(usize, usize)>,
    ) -> (Ty, String) {
        let left_rows = self.matrix_load_i64_field(left, 2);
        let left_cols = self.matrix_load_i64_field(left, 3);
        let right_rows = self.matrix_load_i64_field(right, 2);
        let right_cols = self.matrix_load_i64_field(right, 3);
        let shape_match = self.fresh();
        let ok_lbl = self.fresh_label("matrix.matmul.shape.ok");
        let abort_lbl = self.fresh_label("matrix.matmul.shape.abort");
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            shape_match, left_cols, right_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            shape_match, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        let len = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 {}, {}",
            len, left_rows, right_cols
        )
        .unwrap();
        let bytes = if matrix_elem_size(elem_ty) == 1 {
            format!("%{len}")
        } else {
            let bytes = self.fresh();
            writeln!(
                self.out,
                "  %{} = mul i64 %{}, {}",
                bytes,
                len,
                matrix_elem_size(elem_ty)
            )
            .unwrap();
            format!("%{bytes}")
        };
        let data = self.fresh();
        let out_matrix = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 32)", out_matrix).unwrap();

        let rc_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 0);
        let data_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 1);
        let rows_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 2);
        let cols_ptr = self.matrix_field_ptr(&format!("%{out_matrix}"), 3);
        writeln!(self.out, "  store i64 1, ptr {}", rc_ptr).unwrap();
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", left_rows, rows_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", right_cols, cols_ptr).unwrap();

        let ll = llvm_ty(elem_ty, self.structs);
        let left_data = self.matrix_load_data_ptr(left);
        let right_data = self.matrix_load_data_ptr(right);
        let row_addr = self.fresh();
        let col_addr = self.fresh();
        let inner_addr = self.fresh();
        let acc_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", row_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", col_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", inner_addr).unwrap();
        writeln!(self.out, "  %{} = alloca {}", acc_addr, ll).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", row_addr).unwrap();

        let row_cond_lbl = self.fresh_label("matrix.matmul.row.cond");
        let row_body_lbl = self.fresh_label("matrix.matmul.row.body");
        let row_latch_lbl = self.fresh_label("matrix.matmul.row.latch");
        let done_lbl = self.fresh_label("matrix.matmul.done");
        let col_cond_lbl = self.fresh_label("matrix.matmul.col.cond");
        let col_body_lbl = self.fresh_label("matrix.matmul.col.body");
        let col_latch_lbl = self.fresh_label("matrix.matmul.col.latch");
        let col_done_lbl = self.fresh_label("matrix.matmul.col.done");
        let inner_cond_lbl = self.fresh_label("matrix.matmul.inner.cond");
        let inner_body_lbl = self.fresh_label("matrix.matmul.inner.body");
        let inner_latch_lbl = self.fresh_label("matrix.matmul.inner.latch");
        let inner_done_lbl = self.fresh_label("matrix.matmul.inner.done");
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", row_cond_lbl).unwrap();
        let row = self.fresh();
        let has_row = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", row, row_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            has_row, row, left_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_row, row_body_lbl, done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", row_body_lbl).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_cond_lbl).unwrap();
        let col = self.fresh();
        let has_col = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", col, col_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            has_col, col, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_col, col_body_lbl, col_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", col_body_lbl).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll,
            matrix_zero_value(elem_ty),
            acc_addr
        )
        .unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", inner_addr).unwrap();
        writeln!(self.out, "  br label %{}", inner_cond_lbl).unwrap();

        writeln!(self.out, "{}:", inner_cond_lbl).unwrap();
        let inner = self.fresh();
        let has_inner = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", inner, inner_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            has_inner, inner, left_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_inner, inner_body_lbl, inner_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", inner_body_lbl).unwrap();
        let left_row_offset = self.fresh();
        let left_idx = self.fresh();
        let right_row_offset = self.fresh();
        let right_idx = self.fresh();
        let left_cell_ptr = self.fresh();
        let right_cell_ptr = self.fresh();
        let left_cell = self.fresh();
        let right_cell = self.fresh();
        let acc = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            left_row_offset, row, left_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            left_idx, left_row_offset, inner
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            right_row_offset, inner, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            right_idx, right_row_offset, col
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            left_cell_ptr, ll, left_data, left_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            right_cell_ptr, ll, right_data, right_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            left_cell, ll, left_cell_ptr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            right_cell, ll, right_cell_ptr
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", acc, ll, acc_addr).unwrap();
        let product = self.emit_matrix_elem_binop(
            elem_ty,
            &format!("%{left_cell}"),
            &format!("%{right_cell}"),
            "*",
        );
        let next_acc = self.emit_matrix_elem_binop(elem_ty, &format!("%{acc}"), &product, "+");
        writeln!(self.out, "  store {} {}, ptr %{}", ll, next_acc, acc_addr).unwrap();
        writeln!(self.out, "  br label %{}", inner_latch_lbl).unwrap();

        writeln!(self.out, "{}:", inner_latch_lbl).unwrap();
        let next_inner = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_inner, inner).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_inner, inner_addr).unwrap();
        writeln!(self.out, "  br label %{}", inner_cond_lbl).unwrap();

        writeln!(self.out, "{}:", inner_done_lbl).unwrap();
        let out_row_offset = self.fresh();
        let out_idx = self.fresh();
        let out_cell_ptr = self.fresh();
        let out_value = self.fresh();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            out_row_offset, row, right_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = add i64 %{}, %{}",
            out_idx, out_row_offset, col
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            out_cell_ptr, ll, data, out_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            out_value, ll, acc_addr
        )
        .unwrap();
        writeln!(
            self.out,
            "  store {} %{}, ptr %{}",
            ll, out_value, out_cell_ptr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", col_latch_lbl).unwrap();

        writeln!(self.out, "{}:", col_latch_lbl).unwrap();
        let next_col = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_col, col).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_col, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_done_lbl).unwrap();
        writeln!(self.out, "  br label %{}", row_latch_lbl).unwrap();

        writeln!(self.out, "{}:", row_latch_lbl).unwrap();
        let next_row = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_row, row).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_row, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", done_lbl).unwrap();
        (
            Ty::Matrix(Box::new(elem_ty.clone()), shape),
            format!("%{out_matrix}"),
        )
    }

    fn emit_matrix_det_lu(&mut self, elem_ty: &Ty, matrix: &str, n: &str) -> (Ty, String) {
        let ll = llvm_ty(elem_ty, self.structs);
        let src_data = self.matrix_load_data_ptr(matrix);
        let cell_count = self.fresh();
        let bytes = self.fresh();
        let work = self.fresh();
        writeln!(self.out, "  %{} = mul i64 {}, {}", cell_count, n, n).unwrap();
        writeln!(
            self.out,
            "  %{} = mul i64 %{}, {}",
            bytes,
            cell_count,
            matrix_elem_size(elem_ty)
        )
        .unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 %{})", work, bytes).unwrap();

        let copy_idx_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca i64", copy_idx_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", copy_idx_addr).unwrap();
        let copy_cond_lbl = self.fresh_label("matrix.det.lu.copy.cond");
        let copy_body_lbl = self.fresh_label("matrix.det.lu.copy.body");
        let copy_done_lbl = self.fresh_label("matrix.det.lu.copy.done");
        writeln!(self.out, "  br label %{}", copy_cond_lbl).unwrap();

        writeln!(self.out, "{}:", copy_cond_lbl).unwrap();
        let copy_idx = self.fresh();
        let copy_has = self.fresh();
        writeln!(
            self.out,
            "  %{} = load i64, ptr %{}",
            copy_idx, copy_idx_addr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, %{}",
            copy_has, copy_idx, cell_count
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            copy_has, copy_body_lbl, copy_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", copy_body_lbl).unwrap();
        let src_ptr = self.fresh();
        let dst_ptr = self.fresh();
        let copy_val = self.fresh();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
            src_ptr, ll, src_data, copy_idx
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = getelementptr inbounds {}, ptr %{}, i64 %{}",
            dst_ptr, ll, work, copy_idx
        )
        .unwrap();
        writeln!(self.out, "  %{} = load {}, ptr %{}", copy_val, ll, src_ptr).unwrap();
        writeln!(self.out, "  store {} %{}, ptr %{}", ll, copy_val, dst_ptr).unwrap();
        let copy_next = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", copy_next, copy_idx).unwrap();
        writeln!(
            self.out,
            "  store i64 %{}, ptr %{}",
            copy_next, copy_idx_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", copy_cond_lbl).unwrap();

        writeln!(self.out, "{}:", copy_done_lbl).unwrap();
        let det_addr = self.fresh();
        let sign_addr = self.fresh();
        let k_addr = self.fresh();
        let pivot_addr = self.fresh();
        let row_addr = self.fresh();
        let col_addr = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", det_addr, ll).unwrap();
        writeln!(self.out, "  %{} = alloca i1", sign_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", k_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", pivot_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", row_addr).unwrap();
        writeln!(self.out, "  %{} = alloca i64", col_addr).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll,
            matrix_one_value(elem_ty),
            det_addr
        )
        .unwrap();
        writeln!(self.out, "  store i1 1, ptr %{}", sign_addr).unwrap();
        writeln!(self.out, "  store i64 0, ptr %{}", k_addr).unwrap();

        let k_cond_lbl = self.fresh_label("matrix.det.lu.k.cond");
        let k_body_lbl = self.fresh_label("matrix.det.lu.k.body");
        let find_cond_lbl = self.fresh_label("matrix.det.lu.find.cond");
        let find_check_lbl = self.fresh_label("matrix.det.lu.find.check");
        let find_next_lbl = self.fresh_label("matrix.det.lu.find.next");
        let pivot_found_lbl = self.fresh_label("matrix.det.lu.pivot.found");
        let zero_lbl = self.fresh_label("matrix.det.lu.zero");
        let swap_cond_lbl = self.fresh_label("matrix.det.lu.swap.cond");
        let swap_body_lbl = self.fresh_label("matrix.det.lu.swap.body");
        let swap_done_lbl = self.fresh_label("matrix.det.lu.swap.done");
        let no_swap_lbl = self.fresh_label("matrix.det.lu.swap.skip");
        let after_swap_lbl = self.fresh_label("matrix.det.lu.after.swap");
        let row_cond_lbl = self.fresh_label("matrix.det.lu.row.cond");
        let row_body_lbl = self.fresh_label("matrix.det.lu.row.body");
        let col_cond_lbl = self.fresh_label("matrix.det.lu.col.cond");
        let col_body_lbl = self.fresh_label("matrix.det.lu.col.body");
        let col_done_lbl = self.fresh_label("matrix.det.lu.col.done");
        let row_latch_lbl = self.fresh_label("matrix.det.lu.row.latch");
        let k_latch_lbl = self.fresh_label("matrix.det.lu.k.latch");
        let product_done_lbl = self.fresh_label("matrix.det.lu.product.done");
        let sign_neg_lbl = self.fresh_label("matrix.det.lu.sign.neg");
        let cleanup_lbl = self.fresh_label("matrix.det.lu.cleanup");
        writeln!(self.out, "  br label %{}", k_cond_lbl).unwrap();

        writeln!(self.out, "{}:", k_cond_lbl).unwrap();
        let k = self.fresh();
        let has_k = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", k, k_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_k, k, n).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_k, k_body_lbl, product_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", k_body_lbl).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", k, pivot_addr).unwrap();
        writeln!(self.out, "  br label %{}", find_cond_lbl).unwrap();

        writeln!(self.out, "{}:", find_cond_lbl).unwrap();
        let pivot = self.fresh();
        let pivot_has = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", pivot, pivot_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            pivot_has, pivot, n
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            pivot_has, find_check_lbl, zero_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", find_check_lbl).unwrap();
        let pivot_cell_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{pivot}"),
            &format!("%{k}"),
            elem_ty,
        );
        let pivot_cell = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            pivot_cell, ll, pivot_cell_ptr
        )
        .unwrap();
        let pivot_is_zero = self.emit_matrix_elem_is_zero(elem_ty, &format!("%{pivot_cell}"));
        writeln!(
            self.out,
            "  br i1 {}, label %{}, label %{}",
            pivot_is_zero, find_next_lbl, pivot_found_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", find_next_lbl).unwrap();
        let next_pivot = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_pivot, pivot).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_pivot, pivot_addr).unwrap();
        writeln!(self.out, "  br label %{}", find_cond_lbl).unwrap();

        writeln!(self.out, "{}:", zero_lbl).unwrap();
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll,
            matrix_zero_value(elem_ty),
            det_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", cleanup_lbl).unwrap();

        writeln!(self.out, "{}:", pivot_found_lbl).unwrap();
        let pivot_final = self.fresh();
        let needs_swap = self.fresh();
        writeln!(
            self.out,
            "  %{} = load i64, ptr %{}",
            pivot_final, pivot_addr
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp ne i64 %{}, %{}",
            needs_swap, pivot_final, k
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            needs_swap, swap_cond_lbl, no_swap_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", swap_cond_lbl).unwrap();
        let swap_col = self.fresh();
        let swap_has_col = self.fresh();
        writeln!(self.out, "  store i64 0, ptr %{}", col_addr).unwrap();
        writeln!(self.out, "  br label %{}", swap_body_lbl).unwrap();

        writeln!(self.out, "{}:", swap_body_lbl).unwrap();
        writeln!(self.out, "  %{} = load i64, ptr %{}", swap_col, col_addr).unwrap();
        writeln!(
            self.out,
            "  %{} = icmp slt i64 %{}, {}",
            swap_has_col, swap_col, n
        )
        .unwrap();
        let swap_do_lbl = self.fresh_label("matrix.det.lu.swap.do");
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            swap_has_col, swap_do_lbl, swap_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", swap_do_lbl).unwrap();
        let a_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{k}"),
            &format!("%{swap_col}"),
            elem_ty,
        );
        let b_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{pivot_final}"),
            &format!("%{swap_col}"),
            elem_ty,
        );
        let a_val = self.fresh();
        let b_val = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr {}", a_val, ll, a_ptr).unwrap();
        writeln!(self.out, "  %{} = load {}, ptr {}", b_val, ll, b_ptr).unwrap();
        writeln!(self.out, "  store {} %{}, ptr {}", ll, b_val, a_ptr).unwrap();
        writeln!(self.out, "  store {} %{}, ptr {}", ll, a_val, b_ptr).unwrap();
        let swap_next_col = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", swap_next_col, swap_col).unwrap();
        writeln!(
            self.out,
            "  store i64 %{}, ptr %{}",
            swap_next_col, col_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", swap_body_lbl).unwrap();

        writeln!(self.out, "{}:", swap_done_lbl).unwrap();
        let sign = self.fresh();
        let next_sign = self.fresh();
        writeln!(self.out, "  %{} = load i1, ptr %{}", sign, sign_addr).unwrap();
        writeln!(self.out, "  %{} = xor i1 %{}, true", next_sign, sign).unwrap();
        writeln!(self.out, "  store i1 %{}, ptr %{}", next_sign, sign_addr).unwrap();
        writeln!(self.out, "  br label %{}", after_swap_lbl).unwrap();

        writeln!(self.out, "{}:", no_swap_lbl).unwrap();
        writeln!(self.out, "  br label %{}", after_swap_lbl).unwrap();

        writeln!(self.out, "{}:", after_swap_lbl).unwrap();
        let pivot_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{k}"),
            &format!("%{k}"),
            elem_ty,
        );
        let pivot_val = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            pivot_val, ll, pivot_ptr
        )
        .unwrap();
        let det_current = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            det_current, ll, det_addr
        )
        .unwrap();
        let det_next = self.emit_matrix_elem_binop(
            elem_ty,
            &format!("%{det_current}"),
            &format!("%{pivot_val}"),
            "*",
        );
        writeln!(self.out, "  store {} {}, ptr %{}", ll, det_next, det_addr).unwrap();
        let first_row = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", first_row, k).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", first_row, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", row_cond_lbl).unwrap();
        let row = self.fresh();
        let has_row = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", row, row_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_row, row, n).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_row, row_body_lbl, k_latch_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", row_body_lbl).unwrap();
        let below_pivot_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{row}"),
            &format!("%{k}"),
            elem_ty,
        );
        let below_pivot = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr {}",
            below_pivot, ll, below_pivot_ptr
        )
        .unwrap();
        let factor = self.emit_matrix_elem_div(
            elem_ty,
            &format!("%{below_pivot}"),
            &format!("%{pivot_val}"),
        );
        writeln!(self.out, "  store i64 %{}, ptr %{}", first_row, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_cond_lbl).unwrap();
        let col = self.fresh();
        let has_col = self.fresh();
        writeln!(self.out, "  %{} = load i64, ptr %{}", col, col_addr).unwrap();
        writeln!(self.out, "  %{} = icmp slt i64 %{}, {}", has_col, col, n).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            has_col, col_body_lbl, col_done_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", col_body_lbl).unwrap();
        let aij_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{row}"),
            &format!("%{col}"),
            elem_ty,
        );
        let akj_ptr = self.matrix_data_cell_ptr(
            &format!("%{work}"),
            n,
            &format!("%{k}"),
            &format!("%{col}"),
            elem_ty,
        );
        let aij = self.fresh();
        let akj = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr {}", aij, ll, aij_ptr).unwrap();
        writeln!(self.out, "  %{} = load {}, ptr {}", akj, ll, akj_ptr).unwrap();
        let scaled = self.emit_matrix_elem_binop(elem_ty, &factor, &format!("%{akj}"), "*");
        let updated = self.emit_matrix_elem_binop(elem_ty, &format!("%{aij}"), &scaled, "-");
        writeln!(self.out, "  store {} {}, ptr {}", ll, updated, aij_ptr).unwrap();
        let next_col = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_col, col).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_col, col_addr).unwrap();
        writeln!(self.out, "  br label %{}", col_cond_lbl).unwrap();

        writeln!(self.out, "{}:", col_done_lbl).unwrap();
        writeln!(self.out, "  br label %{}", row_latch_lbl).unwrap();

        writeln!(self.out, "{}:", row_latch_lbl).unwrap();
        let next_row = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_row, row).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_row, row_addr).unwrap();
        writeln!(self.out, "  br label %{}", row_cond_lbl).unwrap();

        writeln!(self.out, "{}:", k_latch_lbl).unwrap();
        let next_k = self.fresh();
        writeln!(self.out, "  %{} = add i64 %{}, 1", next_k, k).unwrap();
        writeln!(self.out, "  store i64 %{}, ptr %{}", next_k, k_addr).unwrap();
        writeln!(self.out, "  br label %{}", k_cond_lbl).unwrap();

        writeln!(self.out, "{}:", product_done_lbl).unwrap();
        let final_sign = self.fresh();
        writeln!(self.out, "  %{} = load i1, ptr %{}", final_sign, sign_addr).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            final_sign, cleanup_lbl, sign_neg_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", sign_neg_lbl).unwrap();
        let positive_det = self.fresh();
        writeln!(
            self.out,
            "  %{} = load {}, ptr %{}",
            positive_det, ll, det_addr
        )
        .unwrap();
        let negative_det = self.emit_matrix_elem_neg(elem_ty, &format!("%{positive_det}"));
        writeln!(
            self.out,
            "  store {} {}, ptr %{}",
            ll, negative_det, det_addr
        )
        .unwrap();
        writeln!(self.out, "  br label %{}", cleanup_lbl).unwrap();

        writeln!(self.out, "{}:", cleanup_lbl).unwrap();
        writeln!(self.out, "  call void @free(ptr %{})", work).unwrap();
        let result = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", result, ll, det_addr).unwrap();
        (elem_ty.clone(), format!("%{result}"))
    }

    fn emit_matrix_det_value(&mut self, elem_ty: Ty, matrix: &str) -> (Ty, String) {
        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 3);
        let square = self.fresh();
        let ok_lbl = self.fresh_label("matrix.det.shape.ok");
        let abort_lbl = self.fresh_label("matrix.det.shape.abort");
        writeln!(self.out, "  %{} = icmp eq i64 {}, {}", square, rows, cols).unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            square, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        self.emit_matrix_det_lu(&elem_ty, matrix, &rows)
    }

    /// Resolves numeric field index for named/tuple field access codegen.
    fn struct_idx(&self, sname: &str, field: &str) -> Option<u32> {
        let s = self.structs.iter().find(|s| s.name == sname)?;
        s.fields
            .iter()
            .position(|(n, _)| n == field)
            .map(|i| i as u32)
    }

    fn vector_idx(&self, vname: &str, field: &str) -> Option<u32> {
        let v = self.vectors.iter().find(|v| v.name == vname)?;
        v.fields.iter().position(|n| n == field).map(|i| i as u32)
    }

    /// If `t` is a declared `vector Name ...`, returns `Name`.
    fn as_nia_vector_name(&self, t: &Ty) -> Option<&str> {
        match t {
            Ty::Struct(s) => self
                .vectors
                .iter()
                .find(|v| v.name == *s)
                .map(|v| v.name.as_str()),
            Ty::Vector(n, _) => self
                .vectors
                .iter()
                .find(|v| v.name == *n)
                .map(|v| v.name.as_str()),
            _ => None,
        }
    }

    fn vector_value_meta(&self, t: &Ty) -> Option<(Ty, usize, String)> {
        match t {
            Ty::Struct(name) | Ty::Vector(name, _) => {
                let v = self.vectors.iter().find(|v| v.name == *name)?;
                Some((
                    v.ty.clone(),
                    v.fields.len(),
                    format!("%struct.{}", sanitize(&v.name)),
                ))
            }
            Ty::AnonVector(elem, n) => Some((elem.as_ref().clone(), *n, llvm_ty(t, self.structs))),
            _ => None,
        }
    }

    /// Single-axis `+` / `-` for vector components (matches scalar `Expr::Add` / `Sub`).
    fn emit_scalar_vec_binop(&mut self, elem_ty: &Ty, a: &str, b: &str, is_add: bool) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i8 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i16 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I32 => {
                let op = if is_add { "add nsw" } else { "sub nsw" };
                writeln!(self.out, "  %{} = {} i32 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i64 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                let op = if is_add { "add" } else { "sub" };
                writeln!(self.out, "  %{} = {} i128 %{}, %{}", out, op, a, b).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let op = if is_add { "fadd" } else { "fsub" };
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(self.out, "  %{} = {} {} %{}, %{}", out, op, ll, a, b).unwrap();
            }
            _ => unreachable!("typechecked vector axis"),
        }
        out
    }

    /// Per-axis `*` between two vector components (matches scalar `Expr::Mul` on integers/floats).
    fn emit_scalar_vec_mul_pair(&mut self, elem_ty: &Ty, a: &str, b: &str) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                writeln!(self.out, "  %{} = mul i8 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                writeln!(self.out, "  %{} = mul i16 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I32 => {
                writeln!(self.out, "  %{} = mul nsw i32 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(self.out, "  %{} = mul i64 %{}, %{}", out, a, b).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                writeln!(self.out, "  %{} = mul i128 %{}, %{}", out, a, b).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(self.out, "  %{} = fmul {} %{}, %{}", out, ll, a, b).unwrap();
            }
            _ => unreachable!("typechecked vector axis"),
        }
        out
    }

    fn emit_scalar_vec_mul(&mut self, elem_ty: &Ty, comp: &str, scalar_ssa: &str) -> String {
        let out = self.fresh();
        match elem_ty {
            Ty::I8 | Ty::U8 => {
                writeln!(self.out, "  %{} = mul i8 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I16 | Ty::U16 => {
                writeln!(self.out, "  %{} = mul i16 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I32 => {
                writeln!(
                    self.out,
                    "  %{} = mul nsw i32 %{}, {}",
                    out, comp, scalar_ssa
                )
                .unwrap();
            }
            Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                writeln!(self.out, "  %{} = mul i64 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::I128 | Ty::U128 => {
                writeln!(self.out, "  %{} = mul i128 %{}, {}", out, comp, scalar_ssa).unwrap();
            }
            Ty::F16 | Ty::F32 | Ty::F64 => {
                let ll = llvm_ty(elem_ty, self.structs);
                writeln!(
                    self.out,
                    "  %{} = fmul {} %{}, {}",
                    out, ll, comp, scalar_ssa
                )
                .unwrap();
            }
            _ => unreachable!("typechecked vector axis"),
        }
        out
    }

    fn vector_axis_ty_cloned(&self, vname: &str) -> Ty {
        self.vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("vector decl")
            .ty
            .clone()
    }

    fn emit_nia_vector_scalar_mul(
        &mut self,
        vname: &str,
        vec_val: &str,
        scalar_ssa: &str,
    ) -> String {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector scalar mul");
        let llvm_st = format!("%struct.{}", sanitize(vname));
        let elem_ty = &vdef.ty;
        let mut agg = "poison".to_string();
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vec_val, i
            )
            .unwrap();
            let ci = self.emit_scalar_vec_mul(elem_ty, &ai, scalar_ssa);
            let ci_v = format!("%{}", ci);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_st,
                agg,
                llvm_ty(elem_ty, self.structs),
                ci_v,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", slot, llvm_st).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, slot).unwrap();
        let loadt = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, slot).unwrap();
        format!("%{loadt}")
    }

    /// Component-wise vector sum/difference (`vector` decl aggregates).
    fn emit_nia_vector_binop(&mut self, vname: &str, vl: &str, vr: &str, is_add: bool) -> String {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector binop");
        let llvm_st = format!("%struct.{}", sanitize(vname));
        let elem_ty = &vdef.ty;
        let mut agg = "poison".to_string();
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vl, i
            )
            .unwrap();
            let bi = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                bi, llvm_st, vr, i
            )
            .unwrap();
            let ci = self.emit_scalar_vec_binop(elem_ty, &ai, &bi, is_add);
            let ci_v = format!("%{}", ci);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_st,
                agg,
                llvm_ty(elem_ty, self.structs),
                ci_v,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", slot, llvm_st).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, slot).unwrap();
        let loadt = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, slot).unwrap();
        format!("%{loadt}")
    }

    /// Component-wise vector product (`vector` decl aggregates).
    fn emit_nia_vector_component_mul(&mut self, vname: &str, vl: &str, vr: &str) -> String {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector component mul");
        let llvm_st = format!("%struct.{}", sanitize(vname));
        let elem_ty = &vdef.ty;
        let mut agg = "poison".to_string();
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vl, i
            )
            .unwrap();
            let bi = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                bi, llvm_st, vr, i
            )
            .unwrap();
            let ci = self.emit_scalar_vec_mul_pair(elem_ty, &ai, &bi);
            let ci_v = format!("%{}", ci);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_st,
                agg,
                llvm_ty(elem_ty, self.structs),
                ci_v,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        let slot = self.fresh();
        writeln!(self.out, "  %{} = alloca {}", slot, llvm_st).unwrap();
        writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, slot).unwrap();
        let loadt = self.fresh();
        writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, slot).unwrap();
        format!("%{loadt}")
    }

    /// Dot product of two `vector` values (sum of per-axis products).
    fn emit_nia_vector_dot(&mut self, vname: &str, vl: &str, vr: &str) -> (Ty, String) {
        let vdef = self
            .vectors
            .iter()
            .find(|v| v.name == vname)
            .expect("typechecked vector dot");
        let elem_ty = vdef.ty.clone();
        let llvm_st = format!("%struct.{}", sanitize(vname));
        assert!(
            !vdef.fields.is_empty(),
            "vector type must have at least one axis"
        );
        let mut acc: Option<String> = None;
        for i in 0..vdef.fields.len() {
            let ai = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                ai, llvm_st, vl, i
            )
            .unwrap();
            let bi = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                bi, llvm_st, vr, i
            )
            .unwrap();
            let pi = self.emit_scalar_vec_mul_pair(&elem_ty, &ai, &bi);
            acc = Some(match acc {
                None => pi,
                Some(prev) => self.emit_scalar_vec_binop(&elem_ty, &prev, &pi, true),
            });
        }
        let id = acc.expect("non-empty vector");
        (elem_ty, format!("%{id}"))
    }

    fn emit_anon_vector_lit(
        &mut self,
        elems: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        let mut emitted: Vec<String> = Vec::new();
        let (elem_ty, n) = match hint {
            Some(Ty::AnonVector(elem_ty, n)) => (elem_ty.as_ref().clone(), *n),
            _ => {
                let first = elems
                    .first()
                    .expect("typechecked non-empty anonymous vector");
                let (first_ty, first_value) = self.emit_expr(first, locals, None);
                emitted.push(first_value);
                (first_ty, elems.len())
            }
        };
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let mut agg = "poison".to_string();
        for (i, elem) in elems.iter().enumerate() {
            let value = if i < emitted.len() {
                emitted[i].clone()
            } else {
                let (_, value) = self.emit_expr(elem, locals, Some(&elem_ty));
                value
            };
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                tmp,
                llvm_vec,
                agg,
                llvm_ty(&elem_ty, self.structs),
                value,
                i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        (vec_ty, agg)
    }

    fn emit_anon_vector_binop(
        &mut self,
        elem_ty: &Ty,
        n: usize,
        vl: &str,
        vr: &str,
        is_add: bool,
    ) -> String {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for i in 0..n {
            let left = self.fresh();
            let right = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left, llvm_vec, vl, i
            )
            .unwrap();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                right, llvm_vec, vr, i
            )
            .unwrap();
            let value = self.emit_scalar_vec_binop(elem_ty, &left, &right, is_add);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} %{}, {}",
                tmp, llvm_vec, agg, elem_ll, value, i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        agg
    }

    fn emit_anon_vector_component_mul(
        &mut self,
        elem_ty: &Ty,
        n: usize,
        vl: &str,
        vr: &str,
    ) -> String {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for i in 0..n {
            let left = self.fresh();
            let right = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left, llvm_vec, vl, i
            )
            .unwrap();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                right, llvm_vec, vr, i
            )
            .unwrap();
            let value = self.emit_scalar_vec_mul_pair(elem_ty, &left, &right);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} %{}, {}",
                tmp, llvm_vec, agg, elem_ll, value, i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        agg
    }

    fn emit_anon_vector_scalar_mul(
        &mut self,
        elem_ty: &Ty,
        n: usize,
        vec_val: &str,
        scalar: &str,
    ) -> String {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for i in 0..n {
            let cell = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                cell, llvm_vec, vec_val, i
            )
            .unwrap();
            let value = self.emit_scalar_vec_mul(elem_ty, &cell, scalar);
            let tmp = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} %{}, {}",
                tmp, llvm_vec, agg, elem_ll, value, i
            )
            .unwrap();
            agg = format!("%{tmp}");
        }
        agg
    }

    fn emit_anon_vector_dot(&mut self, elem_ty: &Ty, n: usize, vl: &str, vr: &str) -> (Ty, String) {
        let vec_ty = Ty::AnonVector(Box::new(elem_ty.clone()), n);
        let llvm_vec = llvm_ty(&vec_ty, self.structs);
        let mut acc: Option<String> = None;
        for i in 0..n {
            let left = self.fresh();
            let right = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left, llvm_vec, vl, i
            )
            .unwrap();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                right, llvm_vec, vr, i
            )
            .unwrap();
            let product = self.emit_scalar_vec_mul_pair(elem_ty, &left, &right);
            acc = Some(match acc {
                None => product,
                Some(prev) => self.emit_scalar_vec_binop(elem_ty, &prev, &product, true),
            });
        }
        let id = acc.expect("typechecked non-empty anonymous vector");
        (elem_ty.clone(), format!("%{id}"))
    }

    fn emit_matrix_vector_shape_check(
        &mut self,
        matrix: &str,
        expected_rows: usize,
        expected_cols: usize,
        label: &str,
    ) -> (String, String, String) {
        let rows = self.matrix_load_i64_field(matrix, 2);
        let cols = self.matrix_load_i64_field(matrix, 3);
        let rows_ok = self.fresh();
        let cols_ok = self.fresh();
        let shape_ok = self.fresh();
        let ok_lbl = self.fresh_label(&format!("{label}.shape.ok"));
        let abort_lbl = self.fresh_label(&format!("{label}.shape.abort"));
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            rows_ok, rows, expected_rows
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = icmp eq i64 {}, {}",
            cols_ok, cols, expected_cols
        )
        .unwrap();
        writeln!(
            self.out,
            "  %{} = and i1 %{}, %{}",
            shape_ok, rows_ok, cols_ok
        )
        .unwrap();
        writeln!(
            self.out,
            "  br i1 %{}, label %{}, label %{}",
            shape_ok, ok_lbl, abort_lbl
        )
        .unwrap();

        writeln!(self.out, "{}:", abort_lbl).unwrap();
        writeln!(self.out, "  call void @abort()").unwrap();
        writeln!(self.out, "  unreachable").unwrap();

        writeln!(self.out, "{}:", ok_lbl).unwrap();
        (rows, cols, self.matrix_load_data_ptr(matrix))
    }

    fn emit_matrix_vector_product(
        &mut self,
        matrix: &str,
        vec_ty: &Ty,
        vec_val: &str,
        out_ty: Ty,
        elem_ty: &Ty,
    ) -> (Ty, String) {
        let (_, in_len, vec_ll) = self
            .vector_value_meta(vec_ty)
            .expect("typechecked Matrix @ vector right operand");
        let (_, out_len, out_ll) = self
            .vector_value_meta(&out_ty)
            .expect("typechecked Matrix @ vector result");
        let (_, cols, data) =
            self.emit_matrix_vector_shape_check(matrix, out_len, in_len, "matrix.vector.matmul");
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for row in 0..out_len {
            let mut acc = matrix_zero_value(elem_ty).to_string();
            for col_idx in 0..in_len {
                let row_offset = self.fresh();
                let flat_idx = self.fresh();
                let matrix_cell_ptr = self.fresh();
                let matrix_cell = self.fresh();
                let vector_cell = self.fresh();
                writeln!(self.out, "  %{} = mul i64 {}, {}", row_offset, row, cols).unwrap();
                writeln!(
                    self.out,
                    "  %{} = add i64 %{}, {}",
                    flat_idx, row_offset, col_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
                    matrix_cell_ptr, elem_ll, data, flat_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr %{}",
                    matrix_cell, elem_ll, matrix_cell_ptr
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    vector_cell, vec_ll, vec_val, col_idx
                )
                .unwrap();
                let product = self.emit_matrix_elem_binop(
                    elem_ty,
                    &format!("%{matrix_cell}"),
                    &format!("%{vector_cell}"),
                    "*",
                );
                acc = self.emit_matrix_elem_binop(elem_ty, &acc, &product, "+");
            }
            let next = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next, out_ll, agg, elem_ll, acc, row
            )
            .unwrap();
            agg = format!("%{next}");
        }
        (out_ty, agg)
    }

    fn emit_vector_matrix_product(
        &mut self,
        vec_ty: &Ty,
        vec_val: &str,
        matrix: &str,
        out_ty: Ty,
        elem_ty: &Ty,
    ) -> (Ty, String) {
        let (_, in_len, vec_ll) = self
            .vector_value_meta(vec_ty)
            .expect("typechecked vector @ Matrix left operand");
        let (_, out_len, out_ll) = self
            .vector_value_meta(&out_ty)
            .expect("typechecked vector @ Matrix result");
        let (_, cols, data) =
            self.emit_matrix_vector_shape_check(matrix, in_len, out_len, "vector.matrix.matmul");
        let elem_ll = llvm_ty(elem_ty, self.structs);
        let mut agg = "poison".to_string();
        for col_idx in 0..out_len {
            let mut acc = matrix_zero_value(elem_ty).to_string();
            for row in 0..in_len {
                let row_offset = self.fresh();
                let flat_idx = self.fresh();
                let matrix_cell_ptr = self.fresh();
                let matrix_cell = self.fresh();
                let vector_cell = self.fresh();
                writeln!(self.out, "  %{} = mul i64 {}, {}", row_offset, row, cols).unwrap();
                writeln!(
                    self.out,
                    "  %{} = add i64 %{}, {}",
                    flat_idx, row_offset, col_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr {}, i64 %{}",
                    matrix_cell_ptr, elem_ll, data, flat_idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr %{}",
                    matrix_cell, elem_ll, matrix_cell_ptr
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    vector_cell, vec_ll, vec_val, row
                )
                .unwrap();
                let product = self.emit_matrix_elem_binop(
                    elem_ty,
                    &format!("%{vector_cell}"),
                    &format!("%{matrix_cell}"),
                    "*",
                );
                acc = self.emit_matrix_elem_binop(elem_ty, &acc, &product, "+");
            }
            let next = self.fresh();
            writeln!(
                self.out,
                "  %{} = insertvalue {} {}, {} {}, {}",
                next, out_ll, agg, elem_ll, acc, col_idx
            )
            .unwrap();
            agg = format!("%{next}");
        }
        (out_ty, agg)
    }

    fn emit_outer(
        &mut self,
        args: &[Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (left_ty, left_val) = self.emit_expr(&args[0], locals, None);
        let (right_ty, right_val) = self.emit_expr(&args[1], locals, None);
        let (elem_ty, rows, left_ll) = self
            .vector_value_meta(&left_ty)
            .expect("typechecked outer left vector");
        let (right_elem_ty, cols, right_ll) = self
            .vector_value_meta(&right_ty)
            .expect("typechecked outer right vector");
        debug_assert!(types_match(&elem_ty, &right_elem_ty));

        let len = rows * cols;
        let bytes = len * matrix_elem_size(&elem_ty);
        let data = self.fresh();
        let matrix = self.fresh();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", data, bytes).unwrap();
        writeln!(self.out, "  %{} = call ptr @malloc(i64 32)", matrix).unwrap();

        let matrix_ref = format!("%{matrix}");
        let rc_ptr = self.matrix_field_ptr(&matrix_ref, 0);
        let data_ptr = self.matrix_field_ptr(&matrix_ref, 1);
        let rows_ptr = self.matrix_field_ptr(&matrix_ref, 2);
        let cols_ptr = self.matrix_field_ptr(&matrix_ref, 3);
        writeln!(self.out, "  store i64 1, ptr {}", rc_ptr).unwrap();
        writeln!(self.out, "  store ptr %{}, ptr {}", data, data_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", rows, rows_ptr).unwrap();
        writeln!(self.out, "  store i64 {}, ptr {}", cols, cols_ptr).unwrap();

        let ll = llvm_ty(&elem_ty, self.structs);
        for i in 0..rows {
            let left_cell = self.fresh();
            writeln!(
                self.out,
                "  %{} = extractvalue {} {}, {}",
                left_cell, left_ll, left_val, i
            )
            .unwrap();
            for j in 0..cols {
                let right_cell = self.fresh();
                let out_cell_ptr = self.fresh();
                let idx = i * cols + j;
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    right_cell, right_ll, right_val, j
                )
                .unwrap();
                let product = self.emit_scalar_vec_mul_pair(&elem_ty, &left_cell, &right_cell);
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds {}, ptr %{}, i64 {}",
                    out_cell_ptr, ll, data, idx
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  store {} %{}, ptr %{}",
                    ll, product, out_cell_ptr
                )
                .unwrap();
            }
        }

        (
            Ty::Matrix(Box::new(elem_ty), Some((rows, cols))),
            format!("%{matrix}"),
        )
    }

    /// Emits one full LLVM function definition.
    ///
    /// ## Strategy
    /// - materialize params into stack slots for uniform local-variable handling,
    /// - lower statements in order, supporting early termination on `return`,
    /// - emit implicit final return from block tail for non-void functions.
    ///
    /// The function body is emitted in SSA-with-stack-slots style (simple and explicit).
    fn emit_fn(mut self, f: &FnDef) -> String {
        self.out.clear();
        let sig = self.fn_sigs.get(&f.name).expect("typechecked sig");
        let ret_ll = match &sig.ret {
            None => "void".into(),
            Some(t) => llvm_ty(t, self.structs),
        };
        let mut params = Vec::new();
        for ((pname, _), pty) in f.params.iter().zip(&sig.params) {
            let ll = llvm_ty(pty, self.structs);
            let ps = sanitize(pname);
            params.push(format!("{ll} %{ps}"));
        }
        writeln!(
            self.out,
            "define {} @{}({}) {{",
            ret_ll,
            sanitize(&f.name),
            params.join(", ")
        )
        .unwrap();
        writeln!(self.out, "entry:").unwrap();

        // Allocate and initialize stack slots for all parameters to unify load/store path
        // with local variables.
        let mut local_ptr: HashMap<String, (Ty, String)> = HashMap::new();
        for ((pname, _), pty) in f.params.iter().zip(&sig.params) {
            let ps = sanitize(pname);
            let ptr = format!("%{ps}.addr");
            writeln!(
                self.out,
                "  {} = alloca {}",
                ptr,
                llvm_ty(pty, self.structs)
            )
            .unwrap();
            writeln!(
                self.out,
                "  store {} %{ps}, ptr {}",
                llvm_ty(pty, self.structs),
                ptr
            )
            .unwrap();
            local_ptr.insert(pname.clone(), (pty.clone(), ptr));
        }

        for st in &f.body.stmts {
            self.emit_stmt(st, &mut local_ptr, sig.ret.as_ref());
            if self.terminated {
                break;
            }
        }

        if self.terminated {
            writeln!(self.out, "}}").unwrap();
            return self.out;
        }

        if let Some(ret_ty) = &sig.ret {
            let tail = f.body.tail.as_ref().unwrap();
            let (t, v) = self.emit_expr(tail, &local_ptr, Some(ret_ty));
            debug_assert!(types_match(&t, ret_ty));
            writeln!(self.out, "  ret {} {}", llvm_ty(ret_ty, self.structs), v).unwrap();
        } else {
            writeln!(self.out, "  ret void").unwrap();
        }

        writeln!(self.out, "}}").unwrap();
        self.out
    }

    /// Emits one statement into current function body.
    ///
    /// `locals` maps source names to `(type, stack_ptr)` and is cloned for `if` branch
    /// lowering to keep branch-local mutations isolated.
    fn emit_stmt(
        &mut self,
        st: &Stmt,
        locals: &mut HashMap<String, (Ty, String)>,
        fn_ret: Option<&Ty>,
    ) {
        match st {
            Stmt::Let { name, ty, init } => {
                let (t, v) = self.emit_expr(init, locals, ty.as_ref());
                let ptr = format!("%{}.addr", sanitize(name));
                writeln!(self.out, "  {} = alloca {}", ptr, llvm_ty(&t, self.structs)).unwrap();
                writeln!(
                    self.out,
                    "  store {} {}, ptr {}",
                    llvm_ty(&t, self.structs),
                    v,
                    ptr
                )
                .unwrap();
                locals.insert(name.clone(), (t, ptr));
            }
            Stmt::Expr(e) => {
                self.emit_expr(e, locals, None);
            }
            Stmt::Assign { target, value } => {
                let (tt, ptr_v) = self.emit_assign_ptr(target, locals);
                let (vt, vv) = self.emit_expr(value, locals, Some(&tt));
                debug_assert!(types_match(&tt, &vt));
                writeln!(
                    self.out,
                    "  store {} {}, ptr {}",
                    llvm_ty(&tt, self.structs),
                    vv,
                    ptr_v
                )
                .unwrap();
            }
            Stmt::Return(e) => {
                let Some(ret_ty) = fn_ret else {
                    unreachable!("typechecked")
                };
                let (t, v) = self.emit_expr(e, locals, Some(ret_ty));
                debug_assert!(types_match(&t, ret_ty));
                writeln!(self.out, "  ret {} {}", llvm_ty(ret_ty, self.structs), v).unwrap();
                self.terminated = true;
            }
            Stmt::Break => {
                let Some(exit) = self.loop_exit_stack.last() else {
                    panic!("internal: `break` should be rejected by typecheck outside `loop`");
                };
                writeln!(self.out, "  br label %{}", exit).unwrap();
                self.terminated = true;
            }
            Stmt::If { cond, then_block } => {
                let (ct, cv) = self.emit_expr(cond, locals, Some(&Ty::Bool));
                debug_assert!(matches!(ct, Ty::Bool));
                let then_lbl = self.fresh_label("if.then");
                let cont_lbl = self.fresh_label("if.cont");
                writeln!(
                    self.out,
                    "  br i1 {}, label %{}, label %{}",
                    cv, then_lbl, cont_lbl
                )
                .unwrap();
                writeln!(self.out, "{}:", then_lbl).unwrap();
                let mut then_locals = locals.clone();
                self.terminated = false;
                for st in &then_block.stmts {
                    self.emit_stmt(st, &mut then_locals, fn_ret);
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &then_block.tail {
                        self.emit_expr(tail, &then_locals, None);
                    }
                    writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
                }
                self.terminated = false;
                writeln!(self.out, "{}:", cont_lbl).unwrap();
            }
            Stmt::While { cond, body } => {
                if self.terminated {
                    return;
                }
                let cond_lbl = self.fresh_label("while.cond");
                let body_lbl = self.fresh_label("while.body");
                let exit_lbl = self.fresh_label("while.exit");
                writeln!(self.out, "  br label %{}", cond_lbl).unwrap();

                writeln!(self.out, "{}:", cond_lbl).unwrap();
                let (ct, cv) = self.emit_expr(cond, locals, Some(&Ty::Bool));
                debug_assert!(matches!(ct, Ty::Bool));
                writeln!(
                    self.out,
                    "  br i1 {}, label %{}, label %{}",
                    cv, body_lbl, exit_lbl
                )
                .unwrap();

                writeln!(self.out, "{}:", body_lbl).unwrap();
                let mut body_locals = locals.clone();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, fn_ret);
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                    }
                    writeln!(self.out, "  br label %{}", cond_lbl).unwrap();
                }
                self.terminated = false;
                writeln!(self.out, "{}:", exit_lbl).unwrap();
            }
            Stmt::Loop { body } => {
                if self.terminated {
                    return;
                }
                let iter_lbl = self.fresh_label("loop.iter");
                let exit_lbl = self.fresh_label("loop.exit");
                self.loop_exit_stack.push(exit_lbl.clone());

                writeln!(self.out, "  br label %{}", iter_lbl).unwrap();
                writeln!(self.out, "{}:", iter_lbl).unwrap();
                let mut body_locals = locals.clone();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, fn_ret);
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                    }
                    writeln!(self.out, "  br label %{}", iter_lbl).unwrap();
                }
                self.loop_exit_stack.pop();
                self.terminated = false;
                writeln!(self.out, "{}:", exit_lbl).unwrap();
            }
            Stmt::For {
                var,
                start,
                end,
                body,
            } => {
                if self.terminated {
                    return;
                }
                let pre = self.fresh_label("for.pre");
                let header = self.fresh_label("for.header");
                let body_lbl = self.fresh_label("for.body");
                let latch = self.fresh_label("for.latch");
                let exit = self.fresh_label("for.exit");

                writeln!(self.out, "  br label %{}", pre).unwrap();
                writeln!(self.out, "{}:", pre).unwrap();
                let (t_ev, v_start) = self.emit_expr(start, locals, None);
                let (_, v_end) = self.emit_expr(end, locals, Some(&t_ev));
                let ll = llvm_ty(&t_ev, self.structs);
                let slot = self.fresh();
                let var_ptr = format!("%{}.{}", sanitize(var), slot);
                writeln!(self.out, "  {} = alloca {}", var_ptr, ll).unwrap();
                writeln!(self.out, "  br label %{}", header).unwrap();

                writeln!(self.out, "{}:", header).unwrap();
                let iv = self.fresh();
                let iv_next = self.fresh();
                let cmp = self.fresh();
                let icmp_op = if int_ty_signed(&t_ev) { "slt" } else { "ult" };
                writeln!(
                    self.out,
                    "  %{} = phi {} [ {}, %{} ], [ %{}, %{} ]",
                    iv, ll, v_start, pre, iv_next, latch
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  %{} = icmp {} {} %{}, {}",
                    cmp, icmp_op, ll, iv, v_end
                )
                .unwrap();
                writeln!(
                    self.out,
                    "  br i1 %{}, label %{}, label %{}",
                    cmp, body_lbl, exit
                )
                .unwrap();

                writeln!(self.out, "{}:", body_lbl).unwrap();
                writeln!(self.out, "  store {} %{}, ptr {}", ll, iv, var_ptr).unwrap();
                let mut body_locals = locals.clone();
                body_locals.insert(var.clone(), (t_ev.clone(), var_ptr));
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, fn_ret);
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                    }
                    writeln!(self.out, "  br label %{}", latch).unwrap();
                }
                let body_term = self.terminated;
                self.terminated = false;
                if body_term {
                    panic!("internal: `return` inside `for` should be rejected by typecheck");
                }

                writeln!(self.out, "{}:", latch).unwrap();
                match &t_ev {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = add i8 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = add i16 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = add nsw i32 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = add i64 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::I128 => {
                        writeln!(self.out, "  %{} = add i128 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::U128 => {
                        writeln!(self.out, "  %{} = add i128 %{}, 1", iv_next, iv).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        unreachable!("typechecked for range")
                    }
                    Ty::Array(_, _)
                    | Ty::Struct(_)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::String
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::Matrix(_, _) => unreachable!("typechecked for range"),
                }
                writeln!(self.out, "  br label %{}", header).unwrap();

                writeln!(self.out, "{}:", exit).unwrap();
            }
            Stmt::Quant { body } => {
                if self.terminated {
                    return;
                }
                let mut body_locals = locals.clone();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, fn_ret);
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                    }
                }
            }
            Stmt::Gpu { body } => {
                if self.terminated {
                    return;
                }
                let mut body_locals = locals.clone();
                self.terminated = false;
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, fn_ret);
                    if self.terminated {
                        break;
                    }
                }
                if !self.terminated {
                    if let Some(tail) = &body.tail {
                        self.emit_expr(tail, &body_locals, None);
                    }
                }
            }
        }
    }

    /// Emits expression and returns `(type, value)` where value is either:
    /// - immediate literal text, or
    /// - SSA reference like `%t7`.
    ///
    /// Caller decides whether returned value should be stored, returned, printed, etc.
    fn emit_expr(
        &mut self,
        e: &Expr,
        locals: &HashMap<String, (Ty, String)>,
        hint: Option<&Ty>,
    ) -> (Ty, String) {
        match e {
            Expr::Int(n) => match hint {
                Some(Ty::F16 | Ty::F32 | Ty::F64) => {
                    let tmp = self.fresh();
                    let t = hint.unwrap().clone();
                    writeln!(
                        self.out,
                        "  %{} = sitofp i32 {} to {}",
                        tmp,
                        n,
                        llvm_ty(&t, self.structs)
                    )
                    .unwrap();
                    (t, format!("%{tmp}"))
                }
                Some(Ty::I8) => (Ty::I8, format!("{n}")),
                Some(Ty::U8) => (Ty::U8, format!("{n}")),
                Some(Ty::I16) => (Ty::I16, format!("{n}")),
                Some(Ty::U16) => (Ty::U16, format!("{n}")),
                Some(Ty::I32) | None => (Ty::I32, format!("{n}")),
                Some(Ty::I64) => (Ty::I64, format!("{n}")),
                Some(Ty::U64) => (Ty::U64, format!("{n}")),
                Some(Ty::I128) => (Ty::I128, format!("{n}")),
                Some(Ty::Isize) => (Ty::Isize, format!("{n}")),
                Some(Ty::Usize) => (Ty::Usize, format!("{n}")),
                Some(Ty::U128) => (Ty::U128, format!("{n}")),
                Some(Ty::Struct(_))
                | Some(Ty::Vector(_, _))
                | Some(Ty::AnonVector(_, _))
                | Some(Ty::Enum(_))
                | Some(Ty::Unit)
                | Some(Ty::Ptr(_))
                | Some(Ty::Bool)
                | Some(Ty::String)
                | Some(Ty::Array(_, _))
                | Some(Ty::Matrix(_, _)) => {
                    unreachable!("typechecked")
                }
            },
            Expr::Float(v) => {
                let lit = format!("{:.17e}", v);
                let target = hint.cloned().unwrap_or(Ty::F64);
                let dbl = self.fresh();
                writeln!(self.out, "  %{} = fadd double {}, 0.0", dbl, lit).unwrap();
                match target {
                    Ty::F64 => (Ty::F64, format!("%{dbl}")),
                    Ty::F32 => {
                        let tmp = self.fresh();
                        writeln!(self.out, "  %{} = fptrunc double %{} to float", tmp, dbl)
                            .unwrap();
                        (Ty::F32, format!("%{tmp}"))
                    }
                    Ty::F16 => {
                        let tmp = self.fresh();
                        writeln!(self.out, "  %{} = fptrunc double %{} to half", tmp, dbl).unwrap();
                        (Ty::F16, format!("%{tmp}"))
                    }
                    _ => unreachable!("typechecked float literal"),
                }
            }
            Expr::Bool(b) => (Ty::Bool, if *b { "1".into() } else { "0".into() }),
            Expr::String(s) => {
                let sym = self
                    .str_lit_syms
                    .get(s)
                    .unwrap_or_else(|| panic!("missing string literal global for {s:?}"));
                let nbytes = s.len() + 1;
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = getelementptr inbounds [{} x i8], ptr @{}, i64 0, i64 0",
                    tmp, nbytes, sym
                )
                .unwrap();
                (Ty::String, format!("%{tmp}"))
            }
            Expr::Ident(name) => {
                let (ty, ptr) = locals.get(name).expect("checked var");
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr {}",
                    tmp,
                    llvm_ty(ty, self.structs),
                    ptr
                )
                .unwrap();
                (ty.clone(), format!("%{tmp}"))
            }
            Expr::Neg(inner) => {
                let (t, v) = self.emit_expr(inner, locals, None);
                let tmp = self.fresh();
                match t {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = sub i8 0, {}", tmp, v).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = sub i16 0, {}", tmp, v).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = sub nsw i32 0, {}", tmp, v).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = sub i64 0, {}", tmp, v).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = sub i128 0, {}", tmp, v).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&t, self.structs);
                        writeln!(self.out, "  %{} = fneg {} {}", tmp, ll, v).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::String
                    | Ty::Matrix(_, _) => {
                        unreachable!("typechecked neg")
                    }
                }
                (t, format!("%{tmp}"))
            }
            Expr::Add(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                if let Some(vname) = self.as_nia_vector_name(&tl).map(|s| s.to_string()) {
                    let out_v = self.emit_nia_vector_binop(&vname, &vl, &vr, true);
                    return (tl, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    let out_v = self.emit_anon_vector_binop(elem_ty, *n, &vl, &vr, true);
                    return (tl, out_v);
                }
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "+");
                }
                let tmp = self.fresh();
                match tl {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = add i8 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = add i16 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = add nsw i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = add i64 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = add i128 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fadd {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::String
                    | Ty::Matrix(_, _) => {
                        unreachable!("add on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::Sub(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                if let Some(vname) = self.as_nia_vector_name(&tl).map(|s| s.to_string()) {
                    let out_v = self.emit_nia_vector_binop(&vname, &vl, &vr, false);
                    return (tl, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    let out_v = self.emit_anon_vector_binop(elem_ty, *n, &vl, &vr, false);
                    return (tl, out_v);
                }
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "-");
                }
                let tmp = self.fresh();
                match tl {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = sub i8 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = sub i16 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = sub nsw i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = sub i64 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = sub i128 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fsub {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::String
                    | Ty::Matrix(_, _) => {
                        unreachable!("sub on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::Mul(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                if let Some(vname) = self.as_nia_vector_name(&tl).map(|s| s.to_string()) {
                    let et = self.vector_axis_ty_cloned(&vname);
                    let (tr, vr) = self.emit_expr(r, locals, Some(&et));
                    if self.as_nia_vector_name(&tr).is_some() && types_match(&tl, &tr) {
                        let out_v = self.emit_nia_vector_component_mul(&vname, &vl, &vr);
                        return (tl, out_v);
                    }
                    let out_v = self.emit_nia_vector_scalar_mul(&vname, &vl, &vr);
                    return (tl, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    let (tr, vr) = match r.as_ref() {
                        Expr::AnonVectorLit(_) => self.emit_expr(r, locals, Some(&tl)),
                        _ => self.emit_expr(r, locals, Some(elem_ty)),
                    };
                    if matches!(tr, Ty::AnonVector(_, _)) {
                        debug_assert!(types_match(&tl, &tr));
                        let out_v = self.emit_anon_vector_component_mul(elem_ty, *n, &vl, &vr);
                        return (tl, out_v);
                    }
                    debug_assert!(types_match(&tr, elem_ty));
                    let out_v = self.emit_anon_vector_scalar_mul(elem_ty, *n, &vl, &vr);
                    return (tl, out_v);
                }
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    let (tr, vr) = self.emit_expr(r, locals, None);
                    if let Ty::Matrix(_, _) = tr {
                        debug_assert!(types_match(&tl, &tr));
                        return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "*");
                    }
                    debug_assert!(types_match(&tr, elem_ty));
                    return self.emit_matrix_scalar_mul(&vl, &vr, elem_ty, *shape);
                }
                let (tr, vr) = match r.as_ref() {
                    Expr::AnonVectorLit(_) => self.emit_expr(r, locals, None),
                    _ => self.emit_expr(r, locals, Some(&tl)),
                };
                if let Some(vname) = self.as_nia_vector_name(&tr).map(|s| s.to_string()) {
                    let et = self.vector_axis_ty_cloned(&vname);
                    let (tl2, vl2) = self.emit_expr(l, locals, Some(&et));
                    if self.as_nia_vector_name(&tl2).is_some() && types_match(&tl2, &tr) {
                        let out_v = self.emit_nia_vector_component_mul(&vname, &vl2, &vr);
                        return (tr, out_v);
                    }
                    let out_v = self.emit_nia_vector_scalar_mul(&vname, &vr, &vl2);
                    return (tr, out_v);
                }
                if let Ty::AnonVector(elem_ty, n) = &tr {
                    debug_assert!(types_match(&tl, elem_ty));
                    let out_v = self.emit_anon_vector_scalar_mul(elem_ty, *n, &vr, &vl);
                    return (tr, out_v);
                }
                if let Ty::Matrix(elem_ty, shape) = &tr {
                    debug_assert!(types_match(&tl, elem_ty));
                    return self.emit_matrix_scalar_mul(&vr, &vl, elem_ty, *shape);
                }
                assert!(types_match(&tl, &tr));
                if let Ty::Matrix(elem_ty, shape) = &tl {
                    return self.emit_matrix_binop(&vl, &vr, elem_ty, *shape, "*");
                }
                let tmp = self.fresh();
                match tl {
                    Ty::I8 | Ty::U8 => {
                        writeln!(self.out, "  %{} = mul i8 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I16 | Ty::U16 => {
                        writeln!(self.out, "  %{} = mul i16 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I32 => {
                        writeln!(self.out, "  %{} = mul nsw i32 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        writeln!(self.out, "  %{} = mul i64 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::I128 | Ty::U128 => {
                        writeln!(self.out, "  %{} = mul i128 {}, {}", tmp, vl, vr).unwrap();
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fmul {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::String
                    | Ty::Matrix(_, _) => {
                        unreachable!("mul on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::VecDot(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                if let Ty::Matrix(elem_ty, left_shape) = &tl {
                    let right_hint =
                        left_shape.map(|(_, cols)| Ty::AnonVector(elem_ty.clone(), cols));
                    let (tr, vr) = match (r.as_ref(), right_hint.as_ref()) {
                        (Expr::AnonVectorLit(_), Some(hint_ty)) => {
                            self.emit_expr(r, locals, Some(hint_ty))
                        }
                        _ => self.emit_expr(r, locals, None),
                    };
                    if let Ty::Matrix(_, right_shape) = &tr {
                        debug_assert!(types_match(&tl, &tr));
                        let shape = match (left_shape, right_shape) {
                            (Some((rows, _)), Some((_, cols))) => Some((*rows, *cols)),
                            _ => None,
                        };
                        return self.emit_matrix_matmul(&vl, &vr, elem_ty, shape);
                    }
                    let (_, in_len, _) = self
                        .vector_value_meta(&tr)
                        .expect("typechecked Matrix @ vector right operand");
                    let out_ty = if let Some(hint_ty) = hint {
                        if self.vector_value_meta(hint_ty).is_some() {
                            hint_ty.clone()
                        } else {
                            unreachable!("typechecked Matrix @ vector result hint")
                        }
                    } else if let Some((rows, _)) = left_shape {
                        if *rows == in_len && self.as_nia_vector_name(&tr).is_some() {
                            tr.clone()
                        } else {
                            Ty::AnonVector(elem_ty.clone(), *rows)
                        }
                    } else {
                        unreachable!("typechecked Matrix @ vector result length")
                    };
                    return self.emit_matrix_vector_product(&vl, &tr, &vr, out_ty, elem_ty);
                }
                let (tr, vr) = self.emit_expr(r, locals, None);
                if let Ty::Matrix(elem_ty, matrix_shape) = &tr {
                    let (_, in_len, _) = self
                        .vector_value_meta(&tl)
                        .expect("typechecked vector @ Matrix left operand");
                    let out_ty = if let Some(hint_ty) = hint {
                        if self.vector_value_meta(hint_ty).is_some() {
                            hint_ty.clone()
                        } else {
                            unreachable!("typechecked vector @ Matrix result hint")
                        }
                    } else if let Some((_, cols)) = matrix_shape {
                        if *cols == in_len && self.as_nia_vector_name(&tl).is_some() {
                            tl.clone()
                        } else {
                            Ty::AnonVector(elem_ty.clone(), *cols)
                        }
                    } else {
                        unreachable!("typechecked vector @ Matrix result length")
                    };
                    return self.emit_vector_matrix_product(&tl, &vl, &vr, out_ty, elem_ty);
                }
                assert!(types_match(&tl, &tr));
                if let Ty::AnonVector(elem_ty, n) = &tl {
                    return self.emit_anon_vector_dot(elem_ty, *n, &vl, &vr);
                }
                let vname = self
                    .as_nia_vector_name(&tl)
                    .expect("typechecked `@` on vectors")
                    .to_string();
                self.emit_nia_vector_dot(&vname, &vl, &vr)
            }
            Expr::Div(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                let signed = int_ty_signed(&tl);
                match tl {
                    Ty::I8 | Ty::U8 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i8 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i8 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I16 | Ty::U16 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i16 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i16 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I32 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i32 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i32 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I64 | Ty::U64 | Ty::Isize | Ty::Usize => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i64 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i64 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::I128 | Ty::U128 => {
                        if signed {
                            writeln!(self.out, "  %{} = sdiv i128 {}, {}", tmp, vl, vr).unwrap();
                        } else {
                            writeln!(self.out, "  %{} = udiv i128 {}, {}", tmp, vl, vr).unwrap();
                        }
                    }
                    Ty::F16 | Ty::F32 | Ty::F64 => {
                        let ll = llvm_ty(&tl, self.structs);
                        writeln!(self.out, "  %{} = fdiv {} {}, {}", tmp, ll, vl, vr).unwrap();
                    }
                    Ty::Array(_, _)
                    | Ty::Struct(_)
                    | Ty::Vector(_, _)
                    | Ty::AnonVector(_, _)
                    | Ty::Enum(_)
                    | Ty::Unit
                    | Ty::Ptr(_)
                    | Ty::Bool
                    | Ty::String
                    | Ty::Matrix(_, _) => {
                        unreachable!("div on non-numeric")
                    }
                }
                (tl, format!("%{tmp}"))
            }
            Expr::Eq(l, r)
            | Expr::Ne(l, r)
            | Expr::Lt(l, r)
            | Expr::Le(l, r)
            | Expr::Gt(l, r)
            | Expr::Ge(l, r) => {
                let (tl, vl) = self.emit_expr(l, locals, None);
                let (tr, vr) = self.emit_expr(r, locals, Some(&tl));
                assert!(types_match(&tl, &tr));
                let tmp = self.fresh();
                if matches!(tl, Ty::String) {
                    let cmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = call i32 @strcmp(ptr {}, ptr {})",
                        cmp, vl, vr
                    )
                    .unwrap();
                    let pred = match e {
                        Expr::Eq(_, _) => "eq",
                        Expr::Ne(_, _) => "ne",
                        _ => unreachable!("typechecked string ordering"),
                    };
                    writeln!(self.out, "  %{} = icmp {} i32 %{}, 0", tmp, pred, cmp).unwrap();
                } else if is_float_ty(&tl) {
                    let pred = match e {
                        Expr::Eq(_, _) => "oeq",
                        Expr::Ne(_, _) => "one",
                        Expr::Lt(_, _) => "olt",
                        Expr::Le(_, _) => "ole",
                        Expr::Gt(_, _) => "ogt",
                        Expr::Ge(_, _) => "oge",
                        _ => unreachable!(),
                    };
                    writeln!(
                        self.out,
                        "  %{} = fcmp {} {} {}, {}",
                        tmp,
                        pred,
                        llvm_ty(&tl, self.structs),
                        vl,
                        vr
                    )
                    .unwrap();
                } else {
                    let pred = match e {
                        Expr::Eq(_, _) => "eq",
                        Expr::Ne(_, _) => "ne",
                        Expr::Lt(_, _) => {
                            if int_ty_signed(&tl) {
                                "slt"
                            } else {
                                "ult"
                            }
                        }
                        Expr::Le(_, _) => {
                            if int_ty_signed(&tl) {
                                "sle"
                            } else {
                                "ule"
                            }
                        }
                        Expr::Gt(_, _) => {
                            if int_ty_signed(&tl) {
                                "sgt"
                            } else {
                                "ugt"
                            }
                        }
                        Expr::Ge(_, _) => {
                            if int_ty_signed(&tl) {
                                "sge"
                            } else {
                                "uge"
                            }
                        }
                        _ => unreachable!(),
                    };
                    writeln!(
                        self.out,
                        "  %{} = icmp {} {} {}, {}",
                        tmp,
                        pred,
                        llvm_ty(&tl, self.structs),
                        vl,
                        vr
                    )
                    .unwrap();
                }
                (Ty::Bool, format!("%{tmp}"))
            }
            Expr::Call { name, args } => {
                if name == PRINTLN {
                    let (at, av) = self.emit_expr(&args[0], locals, None);
                    self.emit_print_value(&at, &av, true);
                    return (Ty::Unit, String::new());
                }
                if name == LEN {
                    let (at, _) = self.emit_expr(&args[0], locals, None);
                    let Ty::Array(_, n) = at else {
                        unreachable!("typechecked len")
                    };
                    return (Ty::I32, format!("{n}"));
                }
                if name == MATRIX_NEW {
                    return self.emit_matrix_new(args, locals);
                }
                if name == MATRIX_GET {
                    return self.emit_matrix_get(args, locals);
                }
                if name == MATRIX_SET {
                    return self.emit_matrix_set(args, locals);
                }
                if name == MATRIX_ROWS
                    || name == MATRIX_COLS
                    || name == MATRIX_LEN
                    || name == MATRIX_REFCOUNT
                {
                    return self.emit_matrix_info(name, args, locals);
                }
                if name == MATRIX_CLONE {
                    return self.emit_matrix_clone(args, locals);
                }
                if name == MATRIX_DROP {
                    return self.emit_matrix_drop(args, locals);
                }
                if name == OUTER {
                    return self.emit_outer(args, locals);
                }
                if name == ALLOC {
                    let (at, av) = self.emit_expr(&args[0], locals, None);
                    let sz = self.emit_sizeof_i64(&at);
                    let raw = self.fresh();
                    writeln!(self.out, "  %{} = call ptr @malloc(i64 {})", raw, sz).unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr %{}",
                        llvm_ty(&at, self.structs),
                        av,
                        raw
                    )
                    .unwrap();
                    return (Ty::Ptr(Box::new(at)), format!("%{raw}"));
                }
                if name == DEALLOC {
                    let (_, pv) = self.emit_expr(&args[0], locals, None);
                    writeln!(self.out, "  call void @free(ptr {})", pv).unwrap();
                    return (Ty::Unit, String::new());
                }
                if name == REALLOC {
                    let (pt, pv) = self.emit_expr(&args[0], locals, None);
                    let Ty::Ptr(pointee) = pt else {
                        unreachable!("typechecked")
                    };
                    let (vt, vv) = self.emit_expr(&args[1], locals, Some(&pointee));
                    debug_assert!(types_match(&vt, &pointee));
                    let sz = self.emit_sizeof_i64(&pointee);
                    let raw = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = call ptr @realloc(ptr {}, i64 {})",
                        raw, pv, sz
                    )
                    .unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr %{}",
                        llvm_ty(&pointee, self.structs),
                        vv,
                        raw
                    )
                    .unwrap();
                    return (Ty::Ptr(pointee), format!("%{raw}"));
                }
                if let Some(sdef) = self.structs.iter().find(|s| s.name == *name && s.is_tuple) {
                    let llvm_st = format!("%struct.{}", sanitize(name));
                    let mut agg = "poison".to_string();
                    for (i, (_, fty)) in sdef.fields.iter().enumerate() {
                        let (at, av) = self.emit_expr(&args[i], locals, Some(fty));
                        debug_assert!(types_match(&at, fty));
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = insertvalue {} {}, {} {}, {}",
                            tmp,
                            llvm_st,
                            agg,
                            llvm_ty(&at, self.structs),
                            av,
                            i
                        )
                        .unwrap();
                        agg = format!("%{tmp}");
                    }
                    let sname = name.clone();
                    let tmp = self.fresh();
                    writeln!(self.out, "  %{} = alloca {}", tmp, llvm_st).unwrap();
                    writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, tmp).unwrap();
                    let loadt = self.fresh();
                    writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, tmp).unwrap();
                    return (Ty::Struct(sname), format!("%{loadt}"));
                }
                let sig = self.fn_sigs.get(name).expect("unknown fn");
                let mut arg_strs = Vec::new();
                for (a, pt) in args.iter().zip(&sig.params) {
                    let (at, av) = self.emit_expr(a, locals, Some(pt));
                    debug_assert!(types_match(&at, pt));
                    arg_strs.push(format!("{} {}", llvm_ty(pt, self.structs), av));
                }
                match &sig.ret {
                    Some(ret_ty) => {
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = call {} @{}({})",
                            tmp,
                            llvm_ty(ret_ty, self.structs),
                            sanitize(name),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (ret_ty.clone(), format!("%{tmp}"))
                    }
                    None => {
                        writeln!(
                            self.out,
                            "  call void @{}({})",
                            sanitize(name),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (Ty::Unit, String::new())
                    }
                }
            }
            Expr::MethodCall {
                receiver,
                name,
                args,
            } => {
                let (recv_ty, recv_val) = self.emit_expr(receiver, locals, None);
                if name == "det" {
                    if let Ty::Matrix(elem_ty, _) = method_receiver_owner_ty(&recv_ty) {
                        debug_assert!(args.is_empty());
                        let matrix = match &recv_ty {
                            Ty::Matrix(_, _) => recv_val,
                            Ty::Ptr(_) => {
                                let tmp = self.fresh();
                                writeln!(self.out, "  %{} = load ptr, ptr {}", tmp, recv_val)
                                    .unwrap();
                                format!("%{tmp}")
                            }
                            _ => unreachable!("typechecked Matrix.det receiver"),
                        };
                        return self.emit_matrix_det_value(elem_ty.as_ref().clone(), &matrix);
                    }
                }
                let symbol = method_symbol(method_receiver_owner_ty(&recv_ty), name);
                let (params, ret) = {
                    let sig = self.fn_sigs.get(&symbol).expect("typechecked method");
                    (sig.params.clone(), sig.ret.clone())
                };
                debug_assert!(!params.is_empty());
                let self_param = &params[0];
                let (self_arg_ty, self_arg_val) = if types_match(&recv_ty, self_param) {
                    (self_param.clone(), recv_val)
                } else if let Ty::Ptr(pointee) = self_param {
                    debug_assert!(types_match(&recv_ty, pointee));
                    let slot = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = alloca {}",
                        slot,
                        llvm_ty(pointee, self.structs)
                    )
                    .unwrap();
                    writeln!(
                        self.out,
                        "  store {} {}, ptr %{}",
                        llvm_ty(pointee, self.structs),
                        recv_val,
                        slot
                    )
                    .unwrap();
                    (self_param.clone(), format!("%{slot}"))
                } else if let Ty::Ptr(pointee) = &recv_ty {
                    debug_assert!(types_match(pointee, self_param));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr {}",
                        tmp,
                        llvm_ty(self_param, self.structs),
                        recv_val
                    )
                    .unwrap();
                    (self_param.clone(), format!("%{tmp}"))
                } else {
                    unreachable!("typechecked method receiver")
                };
                let mut arg_strs = vec![format!(
                    "{} {}",
                    llvm_ty(&self_arg_ty, self.structs),
                    self_arg_val
                )];
                for (a, pt) in args.iter().zip(params.iter().skip(1)) {
                    let (at, av) = self.emit_expr(a, locals, Some(pt));
                    debug_assert!(types_match(&at, pt));
                    arg_strs.push(format!("{} {}", llvm_ty(pt, self.structs), av));
                }
                match ret {
                    Some(ret_ty) => {
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = call {} @{}({})",
                            tmp,
                            llvm_ty(&ret_ty, self.structs),
                            sanitize(&symbol),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (ret_ty, format!("%{tmp}"))
                    }
                    None => {
                        writeln!(
                            self.out,
                            "  call void @{}({})",
                            sanitize(&symbol),
                            arg_strs.join(", ")
                        )
                        .unwrap();
                        (Ty::Unit, String::new())
                    }
                }
            }
            Expr::StructLit { name, fields } => {
                let sdef = self.structs.iter().find(|s| s.name == *name).unwrap();
                let llvm_st = format!("%struct.{}", sanitize(name));
                let mut agg = "poison".to_string();
                for (i, (fname, _)) in sdef.fields.iter().enumerate() {
                    let (_, fe) = fields.iter().find(|(n, _)| n == fname).unwrap();
                    let fty = sdef.fields[i].1.clone();
                    let (ft, fv) = self.emit_expr(fe, locals, Some(&fty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        llvm_st,
                        agg,
                        llvm_ty(&ft, self.structs),
                        fv,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let sname = name.clone();
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = alloca {}", tmp, llvm_st).unwrap();
                writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, tmp).unwrap();
                let loadt = self.fresh();
                writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, tmp).unwrap();
                (Ty::Struct(sname), format!("%{loadt}"))
            }
            Expr::VectorLit { name, fields } => {
                let vdef = self.vectors.iter().find(|s| s.name == *name).unwrap();
                let llvm_st = format!("%struct.{}", sanitize(name));
                let mut agg = "poison".to_string();
                for (i, fname) in vdef.fields.iter().enumerate() {
                    let (_, fe) = fields.iter().find(|(n, _)| n == fname).unwrap();
                    // !!!!!type of fields is the same as the vector type!!!!!
                    let (ft, fv) = self.emit_expr(fe, locals, Some(&vdef.ty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        llvm_st,
                        agg,
                        llvm_ty(&ft, self.structs),
                        fv,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let sname = name.clone();
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = alloca {}", tmp, llvm_st).unwrap();
                writeln!(self.out, "  store {} {}, ptr %{}", llvm_st, agg, tmp).unwrap();
                let loadt = self.fresh();
                writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_st, tmp).unwrap();
                (Ty::Struct(sname), format!("%{loadt}"))
            }
            Expr::AnonVectorLit(elems) => self.emit_anon_vector_lit(elems, locals, hint),
            Expr::EnumVariant { enum_name, variant } => {
                let tag = self
                    .enum_tag(enum_name, variant)
                    .expect("typechecked enum variant");
                let enum_ll = format!("%enum.{}", sanitize(enum_name));
                let idx = self
                    .enum_variant_index(enum_name, variant)
                    .expect("typechecked enum idx");
                let vdef = self
                    .enum_variant_def(enum_name, variant)
                    .expect("typechecked enum variant")
                    .clone();
                let payload_ll = enum_variant_payload_ty(&vdef, self.structs);
                let with_tag = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} poison, i32 {}, 0",
                    with_tag, enum_ll, tag
                )
                .unwrap();
                let with_payload = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} %{}, {} zeroinitializer, {}",
                    with_payload,
                    enum_ll,
                    with_tag,
                    payload_ll,
                    idx + 1
                )
                .unwrap();
                (Ty::Enum(enum_name.clone()), format!("%{with_payload}"))
            }
            Expr::EnumTuple {
                enum_name,
                variant,
                args,
            } => {
                let tag = self
                    .enum_tag(enum_name, variant)
                    .expect("typechecked enum variant");
                let idx = self
                    .enum_variant_index(enum_name, variant)
                    .expect("typechecked enum idx");
                let vdef = self
                    .enum_variant_def(enum_name, variant)
                    .expect("typechecked enum variant")
                    .clone();
                let EnumVariantFields::Tuple(ts) = vdef.fields else {
                    unreachable!("typechecked tuple variant")
                };
                let enum_ll = format!("%enum.{}", sanitize(enum_name));
                let with_tag = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} poison, i32 {}, 0",
                    with_tag, enum_ll, tag
                )
                .unwrap();
                let payload_val = if ts.len() == 1 {
                    let (_, v) = self.emit_expr(&args[0], locals, Some(&ts[0]));
                    (llvm_ty(&ts[0], self.structs), v)
                } else {
                    let payload_ll = if ts.len() == 1 {
                        llvm_ty(&ts[0], self.structs)
                    } else {
                        let inner = ts
                            .iter()
                            .map(|t| llvm_ty(t, self.structs))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{{ {inner} }}")
                    };
                    let mut agg = "poison".to_string();
                    for (i, (a, t)) in args.iter().zip(ts.iter()).enumerate() {
                        let (_, av) = self.emit_expr(a, locals, Some(t));
                        let tmp = self.fresh();
                        writeln!(
                            self.out,
                            "  %{} = insertvalue {} {}, {} {}, {}",
                            tmp,
                            payload_ll,
                            agg,
                            llvm_ty(t, self.structs),
                            av,
                            i
                        )
                        .unwrap();
                        agg = format!("%{tmp}");
                    }
                    (payload_ll, agg)
                };
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} %{}, {} {}, {}",
                    out,
                    enum_ll,
                    with_tag,
                    payload_val.0,
                    payload_val.1,
                    idx + 1
                )
                .unwrap();
                (Ty::Enum(enum_name.clone()), format!("%{out}"))
            }
            Expr::EnumStruct {
                enum_name,
                variant,
                fields,
            } => {
                let tag = self
                    .enum_tag(enum_name, variant)
                    .expect("typechecked enum variant");
                let idx = self
                    .enum_variant_index(enum_name, variant)
                    .expect("typechecked enum idx");
                let vdef = self
                    .enum_variant_def(enum_name, variant)
                    .expect("typechecked enum variant")
                    .clone();
                let EnumVariantFields::Struct(fs) = vdef.fields else {
                    unreachable!("typechecked struct variant")
                };
                let enum_ll = format!("%enum.{}", sanitize(enum_name));
                let with_tag = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} poison, i32 {}, 0",
                    with_tag, enum_ll, tag
                )
                .unwrap();
                let payload_ll = {
                    let inner = fs
                        .iter()
                        .map(|(_, t)| llvm_ty(t, self.structs))
                        .collect::<Vec<_>>()
                        .join(", ");
                    format!("{{ {inner} }}")
                };
                let mut agg = "poison".to_string();
                for (i, (fname, fty)) in fs.iter().enumerate() {
                    let (_, fe) = fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .expect("typechecked");
                    let (_, fv) = self.emit_expr(fe, locals, Some(fty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        payload_ll,
                        agg,
                        llvm_ty(fty, self.structs),
                        fv,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let out = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = insertvalue {} %{}, {} {}, {}",
                    out,
                    enum_ll,
                    with_tag,
                    payload_ll,
                    agg,
                    idx + 1
                )
                .unwrap();
                (Ty::Enum(enum_name.clone()), format!("%{out}"))
            }
            Expr::Match { scrutinee, arms } => {
                let (st, sv) = self.emit_expr(scrutinee, locals, None);
                let Ty::Enum(enum_name) = st else {
                    unreachable!("typechecked match enum")
                };
                let tag_tmp = self.fresh();
                let enum_ll = format!("%enum.{}", sanitize(&enum_name));
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, 0",
                    tag_tmp, enum_ll, sv
                )
                .unwrap();
                let cont_lbl = self.fresh_label("match.cont");
                let default_lbl = self.fresh_label("match.default");
                let mut arm_labels = Vec::new();
                for _ in arms {
                    arm_labels.push(self.fresh_label("match.arm"));
                }
                writeln!(
                    self.out,
                    "  switch i32 %{}, label %{} [",
                    tag_tmp, default_lbl
                )
                .unwrap();
                for ((pat, _), lbl) in arms.iter().zip(&arm_labels) {
                    let variant = match pat {
                        MatchPattern::Unit { variant, .. } => variant,
                        MatchPattern::Tuple { variant, .. } => variant,
                        MatchPattern::Struct { variant, .. } => variant,
                    };
                    let tag = self
                        .enum_tag(&enum_name, variant)
                        .expect("typechecked enum tag");
                    writeln!(self.out, "    i32 {}, label %{}", tag, lbl).unwrap();
                }
                writeln!(self.out, "  ]").unwrap();
                writeln!(self.out, "{}:", default_lbl).unwrap();
                writeln!(self.out, "  unreachable").unwrap();

                let mut arm_vals: Vec<(String, String)> = Vec::new();
                let mut out_ty: Option<Ty> = None;
                for ((pat, arm_expr), lbl) in arms.iter().zip(&arm_labels) {
                    writeln!(self.out, "{}:", lbl).unwrap();
                    let mut arm_locals = locals.clone();
                    let variant = match pat {
                        MatchPattern::Unit { variant, .. } => variant,
                        MatchPattern::Tuple { variant, .. } => variant,
                        MatchPattern::Struct { variant, .. } => variant,
                    };
                    let vidx = self
                        .enum_variant_index(&enum_name, variant)
                        .expect("typechecked enum idx");
                    let vdef = self
                        .enum_variant_def(&enum_name, variant)
                        .expect("typechecked enum variant")
                        .clone();
                    match (pat, vdef.fields) {
                        (MatchPattern::Unit { .. }, EnumVariantFields::Unit) => {}
                        (MatchPattern::Tuple { bindings, .. }, EnumVariantFields::Tuple(ts)) => {
                            let payload_ll = if ts.len() == 1 {
                                llvm_ty(&ts[0], self.structs)
                            } else {
                                let inner = ts
                                    .iter()
                                    .map(|t| llvm_ty(t, self.structs))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("{{ {inner} }}")
                            };
                            let pl = self.fresh();
                            writeln!(
                                self.out,
                                "  %{} = extractvalue {} {}, {}",
                                pl,
                                enum_ll,
                                sv,
                                vidx + 1
                            )
                            .unwrap();
                            if ts.len() == 1 {
                                let ptr = format!("%{}.addr", sanitize(&bindings[0]));
                                writeln!(
                                    self.out,
                                    "  {} = alloca {}",
                                    ptr,
                                    llvm_ty(&ts[0], self.structs)
                                )
                                .unwrap();
                                writeln!(
                                    self.out,
                                    "  store {} %{}, ptr {}",
                                    llvm_ty(&ts[0], self.structs),
                                    pl,
                                    ptr
                                )
                                .unwrap();
                                arm_locals.insert(bindings[0].clone(), (ts[0].clone(), ptr));
                            } else {
                                for (i, (b, t)) in bindings.iter().zip(ts.iter()).enumerate() {
                                    let ev = self.fresh();
                                    writeln!(
                                        self.out,
                                        "  %{} = extractvalue {} %{}, {}",
                                        ev, payload_ll, pl, i
                                    )
                                    .unwrap();
                                    let ptr = format!("%{}.addr", sanitize(b));
                                    writeln!(
                                        self.out,
                                        "  {} = alloca {}",
                                        ptr,
                                        llvm_ty(t, self.structs)
                                    )
                                    .unwrap();
                                    writeln!(
                                        self.out,
                                        "  store {} %{}, ptr {}",
                                        llvm_ty(t, self.structs),
                                        ev,
                                        ptr
                                    )
                                    .unwrap();
                                    arm_locals.insert(b.clone(), (t.clone(), ptr));
                                }
                            }
                        }
                        (MatchPattern::Struct { bindings, .. }, EnumVariantFields::Struct(fs)) => {
                            let payload_ll = {
                                let inner = fs
                                    .iter()
                                    .map(|(_, t)| llvm_ty(t, self.structs))
                                    .collect::<Vec<_>>()
                                    .join(", ");
                                format!("{{ {inner} }}")
                            };
                            let pl = self.fresh();
                            writeln!(
                                self.out,
                                "  %{} = extractvalue {} {}, {}",
                                pl,
                                enum_ll,
                                sv,
                                vidx + 1
                            )
                            .unwrap();
                            for b in bindings {
                                let (i, (_, t)) = fs
                                    .iter()
                                    .enumerate()
                                    .find(|(_, (n, _))| n == b)
                                    .expect("typechecked field");
                                let ev = self.fresh();
                                writeln!(
                                    self.out,
                                    "  %{} = extractvalue {} %{}, {}",
                                    ev, payload_ll, pl, i
                                )
                                .unwrap();
                                let ptr = format!("%{}.addr", sanitize(b));
                                writeln!(
                                    self.out,
                                    "  {} = alloca {}",
                                    ptr,
                                    llvm_ty(t, self.structs)
                                )
                                .unwrap();
                                writeln!(
                                    self.out,
                                    "  store {} %{}, ptr {}",
                                    llvm_ty(t, self.structs),
                                    ev,
                                    ptr
                                )
                                .unwrap();
                                arm_locals.insert(b.clone(), (t.clone(), ptr));
                            }
                        }
                        _ => unreachable!("typechecked pattern kind"),
                    }
                    let (at, av) = self.emit_expr(arm_expr, &arm_locals, hint);
                    if !matches!(at, Ty::Unit) {
                        arm_vals.push((av.clone(), lbl.clone()));
                    }
                    if out_ty.is_none() {
                        out_ty = Some(at.clone());
                    }
                    writeln!(self.out, "  br label %{}", cont_lbl).unwrap();
                }
                writeln!(self.out, "{}:", cont_lbl).unwrap();
                let out_ty = out_ty.expect("typechecked non-empty match");
                if matches!(out_ty, Ty::Unit) {
                    (Ty::Unit, String::new())
                } else {
                    let phi = self.fresh();
                    let ll = llvm_ty(&out_ty, self.structs);
                    let incoming = arm_vals
                        .iter()
                        .map(|(v, l)| format!("[ {}, %{} ]", v, l))
                        .collect::<Vec<_>>()
                        .join(", ");
                    writeln!(self.out, "  %{} = phi {} {}", phi, ll, incoming).unwrap();
                    (out_ty, format!("%{phi}"))
                }
            }
            Expr::Quant { body } => {
                let mut body_locals = locals.clone();
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, None);
                    debug_assert!(
                        !self.terminated,
                        "typecheck rejects terminating statements in quant expressions"
                    );
                }
                if let Some(tail) = &body.tail {
                    self.emit_expr(tail, &body_locals, hint)
                } else {
                    (Ty::Unit, String::new())
                }
            }
            Expr::Gpu { body } => {
                let mut body_locals = locals.clone();
                for st in &body.stmts {
                    self.emit_stmt(st, &mut body_locals, None);
                    debug_assert!(
                        !self.terminated,
                        "typecheck rejects terminating statements in gpu expressions"
                    );
                }
                if let Some(tail) = &body.tail {
                    self.emit_expr(tail, &body_locals, hint)
                } else {
                    (Ty::Unit, String::new())
                }
            }
            Expr::ArrayLit(elems) => {
                let (elem_ty, n) = match hint {
                    Some(Ty::Array(elem, n)) => (elem.as_ref().clone(), *n),
                    _ => {
                        let first = elems.first().expect("typechecked non-empty array");
                        let (t, _) = self.emit_expr(first, locals, None);
                        (t, elems.len())
                    }
                };
                let llvm_arr = format!("[{} x {}]", n, llvm_ty(&elem_ty, self.structs));
                let mut agg = "poison".to_string();
                for (i, e) in elems.iter().enumerate() {
                    let (et, ev) = self.emit_expr(e, locals, Some(&elem_ty));
                    debug_assert!(types_match(&et, &elem_ty));
                    let tmp = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = insertvalue {} {}, {} {}, {}",
                        tmp,
                        llvm_arr,
                        agg,
                        llvm_ty(&et, self.structs),
                        ev,
                        i
                    )
                    .unwrap();
                    agg = format!("%{tmp}");
                }
                let tmp = self.fresh();
                writeln!(self.out, "  %{} = alloca {}", tmp, llvm_arr).unwrap();
                writeln!(self.out, "  store {} {}, ptr %{}", llvm_arr, agg, tmp).unwrap();
                let loadt = self.fresh();
                writeln!(self.out, "  %{} = load {}, ptr %{}", loadt, llvm_arr, tmp).unwrap();
                (Ty::Array(Box::new(elem_ty), n), format!("%{loadt}"))
            }
            Expr::Field(obj, fname) => {
                let (bt, bv) = self.emit_expr(obj, locals, None);
                let (sname, aggregate_val) = match bt {
                    Ty::Struct(sname) | Ty::Vector(sname, _) => (sname, bv),
                    Ty::Ptr(inner) => match inner.as_ref() {
                        Ty::Struct(sname) | Ty::Vector(sname, _) => {
                            let tmp = self.fresh();
                            writeln!(
                                self.out,
                                "  %{} = load {}, ptr {}",
                                tmp,
                                llvm_ty(inner.as_ref(), self.structs),
                                bv
                            )
                            .unwrap();
                            (sname.clone(), format!("%{tmp}"))
                        }
                        _ => unreachable!("typechecked field base type"),
                    },
                    _ => unreachable!("typechecked field base type"),
                };
                let idx = self
                    .struct_idx(&sname, fname)
                    .or_else(|| self.vector_idx(&sname, fname))
                    .unwrap();
                let llvm_st = format!("%struct.{}", sanitize(&sname));
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = extractvalue {} {}, {}",
                    tmp, llvm_st, aggregate_val, idx
                )
                .unwrap();
                let fty = if let Some(sdef) = self.structs.iter().find(|s| s.name == sname) {
                    sdef.fields
                        .iter()
                        .find(|(n, _)| n == fname)
                        .unwrap()
                        .1
                        .clone()
                } else if let Some(vdef) = self.vectors.iter().find(|v| v.name == sname) {
                    assert!(vdef.fields.iter().any(|n| n == fname));
                    vdef.ty.clone()
                } else {
                    unreachable!("typechecked field base type")
                };
                (fty, format!("%{tmp}"))
            }
            Expr::Index(arr, idx) => {
                let full = Expr::Index(arr.clone(), idx.clone());
                if let Some((root, idxs)) = collect_array_index_chain(&full) {
                    let (elem_ty, gep_ptr) = self.emit_array_gep_chain(root, &idxs, locals);
                    let val = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr {}",
                        val,
                        llvm_ty(&elem_ty, self.structs),
                        gep_ptr
                    )
                    .unwrap();
                    (elem_ty, format!("%{val}"))
                } else {
                    let (at, av) = self.emit_expr(arr, locals, None);
                    let (it, iv) = self.emit_expr(idx, locals, Some(&Ty::I32));
                    debug_assert!(matches!(it, Ty::I32));
                    let Ty::Array(elem_ty, n) = at else {
                        unreachable!("typechecked")
                    };
                    let llvm_arr = format!("[{} x {}]", n, llvm_ty(&elem_ty, self.structs));
                    let ptr = self.fresh();
                    writeln!(self.out, "  %{} = alloca {}", ptr, llvm_arr).unwrap();
                    writeln!(self.out, "  store {} {}, ptr %{}", llvm_arr, av, ptr).unwrap();
                    let idx64 = self.fresh();
                    writeln!(self.out, "  %{} = sext i32 {} to i64", idx64, iv).unwrap();
                    let gep = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = getelementptr inbounds {}, ptr %{}, i64 0, i64 %{}",
                        gep, llvm_arr, ptr, idx64
                    )
                    .unwrap();
                    let val = self.fresh();
                    writeln!(
                        self.out,
                        "  %{} = load {}, ptr %{}",
                        val,
                        llvm_ty(&elem_ty, self.structs),
                        gep
                    )
                    .unwrap();
                    ((*elem_ty).clone(), format!("%{val}"))
                }
            }
            Expr::AddrOf(inner) => {
                let Expr::Ident(name) = inner.as_ref() else {
                    unreachable!("typechecked")
                };
                let (ty, ptr) = locals.get(name).expect("checked var");
                (Ty::Ptr(Box::new(ty.clone())), ptr.clone())
            }
            Expr::Deref(inner) => {
                let (ti, v) = self.emit_expr(inner, locals, None);
                let Ty::Ptr(pointee) = ti else {
                    unreachable!("typechecked")
                };
                let tmp = self.fresh();
                writeln!(
                    self.out,
                    "  %{} = load {}, ptr {}",
                    tmp,
                    llvm_ty(pointee.as_ref(), self.structs),
                    v
                )
                .unwrap();
                ((*pointee).clone(), format!("%{tmp}"))
            }
        }
    }

    /// Follow `root` through `indices` with GEP; returns element type and `ptr` to the slot.
    fn emit_array_gep_chain(
        &mut self,
        root: &Expr,
        indices: &[&Expr],
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        let (mut cur_ty, mut llvm_ptr) = match root {
            Expr::Ident(name) => locals.get(name).expect("indexed assign var").clone(),
            Expr::Deref(inner) => {
                let (pt, pv) = self.emit_expr(inner, locals, None);
                let Ty::Ptr(pointee) = pt else {
                    unreachable!("typechecked indexed deref root")
                };
                ((*pointee).clone(), pv)
            }
            _ => unreachable!("typechecked index chain root"),
        };
        for idx_expr in indices {
            let Ty::Array(elem_ty, n) = &cur_ty else {
                unreachable!("typechecked index chain");
            };
            let (_, iv) = self.emit_expr(idx_expr, locals, Some(&Ty::I32));
            let llvm_arr = format!("[{} x {}]", n, llvm_ty(elem_ty, self.structs));
            let idx64 = self.fresh();
            writeln!(self.out, "  %{} = sext i32 {} to i64", idx64, iv).unwrap();
            let gep = self.fresh();
            writeln!(
                self.out,
                "  %{} = getelementptr inbounds {}, ptr {}, i64 0, i64 %{}",
                gep, llvm_arr, llvm_ptr, idx64
            )
            .unwrap();
            llvm_ptr = format!("%{gep}");
            cur_ty = *elem_ty.clone();
        }
        (cur_ty, llvm_ptr)
    }

    fn emit_assign_ptr(
        &mut self,
        target: &Expr,
        locals: &HashMap<String, (Ty, String)>,
    ) -> (Ty, String) {
        match target {
            Expr::Ident(name) => {
                let (ty, ptr) = locals.get(name).expect("typechecked assign ident");
                (ty.clone(), ptr.clone())
            }
            Expr::Deref(inner) => {
                let (pt, pv) = self.emit_expr(inner, locals, None);
                let Ty::Ptr(pointee) = pt else {
                    unreachable!("typechecked assign deref")
                };
                ((*pointee).clone(), pv)
            }
            Expr::Index(arr, idx) => {
                let full = Expr::Index(arr.clone(), idx.clone());
                if let Some((root, idxs)) = collect_array_index_chain(&full) {
                    self.emit_array_gep_chain(root, &idxs, locals)
                } else {
                    unreachable!("typechecked indexed assign")
                }
            }
            _ => unreachable!("typechecked assign lvalue"),
        }
    }
}

/// Lightweight type compatibility used by codegen assertions.
fn types_match(a: &Ty, b: &Ty) -> bool {
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
        | (Ty::F16, Ty::F16)
        | (Ty::F32, Ty::F32)
        | (Ty::F64, Ty::F64)
        | (Ty::Bool, Ty::Bool)
        | (Ty::String, Ty::String)
        | (Ty::Unit, Ty::Unit) => true,
        (Ty::Array(ax, an), Ty::Array(bx, bn)) => an == bn && types_match(ax, bx),
        (Ty::Struct(x), Ty::Struct(y)) => x == y,
        (Ty::Vector(xn, xt), Ty::Vector(yn, yt)) => xn == yn && types_match(xt, yt),
        (Ty::AnonVector(xt, xn), Ty::AnonVector(yt, yn)) => xn == yn && types_match(xt, yt),
        (Ty::Struct(x), Ty::Vector(y, _)) | (Ty::Vector(y, _), Ty::Struct(x)) => x == y,
        (Ty::Enum(x), Ty::Enum(y)) => x == y,
        (Ty::Ptr(x), Ty::Ptr(y)) => types_match(x, y),
        (Ty::Matrix(x, _), Ty::Matrix(y, _)) => {
            matches!(x.as_ref(), Ty::Unit) || matches!(y.as_ref(), Ty::Unit) || types_match(x, y)
        }
        _ => false,
    }
}

/// Convenience wrapper creating fresh generator per function.
fn emit_fn(
    f: &FnDef,
    structs: &[StructDef],
    enums: &[EnumDef],
    vectors: &[VectorDef],
    fn_sigs: &HashMap<String, FnSig>,
    str_lit_syms: &HashMap<String, String>,
) -> String {
    Gen::new(structs, enums, vectors, fn_sigs, str_lit_syms).emit_fn(f)
}

#[cfg(test)]
mod tests;
