//! QIR (Quantum Intermediate Representation) backend.
//!
//! Current lowering is intentionally small: it recognizes `qubit()` calls inside
//! `quant { ... }`, assigns them static resource ids, and emits a Base Profile
//! QIR entry point with the matching `required_num_qubits` attribute. Gates,
//! measurements, and result recording are follow-up work.

use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::{Block, EnumDef, Expr, FnDef, Stmt, StructDef, VectorDef};
use crate::nia_std::QUBIT;
use crate::semantics::typecheck::FnSig;

#[derive(Default)]
struct QirPlan {
    qubits: usize,
}

/// Emits a QIR module for the validated AST.
///
/// The signature mirrors [`crate::backend::codegen::emit_module`] so the driver
/// can dispatch on a `Backend` enum without per-backend argument shaping.
pub fn emit_module(
    _structs: &[StructDef],
    _enums: &[EnumDef],
    _vectors: &[VectorDef],
    fns: &[FnDef],
    _fn_sigs: &HashMap<String, FnSig>,
) -> Result<String, String> {
    let main = fns
        .iter()
        .find(|f| f.name == "main")
        .ok_or_else(|| "QIR backend requires a `main` function".to_string())?;

    let mut plan = QirPlan::default();
    collect_block(&main.body, false, &mut plan)?;
    Ok(render_module(&plan))
}

fn collect_block(block: &Block, in_quant: bool, plan: &mut QirPlan) -> Result<(), String> {
    for st in &block.stmts {
        collect_stmt(st, in_quant, plan)?;
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, in_quant, plan)?;
    }
    Ok(())
}

fn collect_stmt(st: &Stmt, in_quant: bool, plan: &mut QirPlan) -> Result<(), String> {
    match st {
        Stmt::Quant { body } => collect_block(body, true, plan),
        Stmt::Gpu { body } if !in_quant => collect_block(body, false, plan),
        Stmt::Let { init, .. } if in_quant => collect_quant_expr(init, plan),
        Stmt::Expr(e) if in_quant => collect_quant_expr(e, plan),
        Stmt::Let { init, .. } => collect_expr(init, false, plan),
        Stmt::Expr(e) => collect_expr(e, false, plan),
        Stmt::Assign { target, value } if !in_quant => {
            collect_expr(target, false, plan)?;
            collect_expr(value, false, plan)
        }
        Stmt::Return(e) if !in_quant => collect_expr(e, false, plan),
        Stmt::If { cond, then_block } if !in_quant => {
            collect_expr(cond, false, plan)?;
            collect_block(then_block, false, plan)
        }
        Stmt::While { cond, body } if !in_quant => {
            collect_expr(cond, false, plan)?;
            collect_block(body, false, plan)
        }
        Stmt::Loop { body } if !in_quant => collect_block(body, false, plan),
        Stmt::For {
            start, end, body, ..
        } if !in_quant => {
            collect_expr(start, false, plan)?;
            collect_expr(end, false, plan)?;
            collect_block(body, false, plan)
        }
        Stmt::Assign { .. }
        | Stmt::Return(_)
        | Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::Loop { .. }
        | Stmt::Break
        | Stmt::For { .. }
        | Stmt::Gpu { .. } => Err(
            "QIR lowering currently supports only `let name = qubit();` inside `quant` blocks"
                .into(),
        ),
    }
}

fn collect_expr(e: &Expr, in_quant: bool, plan: &mut QirPlan) -> Result<(), String> {
    if in_quant {
        return collect_quant_expr(e, plan);
    }

    match e {
        Expr::Quant { body } => collect_block(body, true, plan),
        Expr::Gpu { body } => collect_block(body, false, plan),
        Expr::Neg(inner) | Expr::AddrOf(inner) | Expr::Deref(inner) => {
            collect_expr(inner, false, plan)
        }
        Expr::Add(l, r)
        | Expr::Sub(l, r)
        | Expr::Mul(l, r)
        | Expr::VecDot(l, r)
        | Expr::Div(l, r)
        | Expr::Eq(l, r)
        | Expr::Ne(l, r)
        | Expr::Lt(l, r)
        | Expr::Le(l, r)
        | Expr::Gt(l, r)
        | Expr::Ge(l, r)
        | Expr::Index(l, r) => {
            collect_expr(l, false, plan)?;
            collect_expr(r, false, plan)
        }
        Expr::Call { args, .. } | Expr::GenericCall { args, .. } => {
            for arg in args {
                collect_expr(arg, false, plan)?;
            }
            Ok(())
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_expr(receiver, false, plan)?;
            for arg in args {
                collect_expr(arg, false, plan)?;
            }
            Ok(())
        }
        Expr::StructLit { fields, .. }
        | Expr::VectorLit { fields, .. }
        | Expr::EnumStruct { fields, .. } => {
            for (_, expr) in fields {
                collect_expr(expr, false, plan)?;
            }
            Ok(())
        }
        Expr::AnonVectorLit(elems)
        | Expr::ArrayLit(elems)
        | Expr::EnumTuple { args: elems, .. } => {
            for elem in elems {
                collect_expr(elem, false, plan)?;
            }
            Ok(())
        }
        Expr::Match { scrutinee, arms } => {
            collect_expr(scrutinee, false, plan)?;
            for (_, arm) in arms {
                collect_expr(arm, false, plan)?;
            }
            Ok(())
        }
        Expr::Field(obj, _) => collect_expr(obj, false, plan),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Ident(_)
        | Expr::EnumVariant { .. } => Ok(()),
    }
}

fn collect_quant_expr(e: &Expr, plan: &mut QirPlan) -> Result<(), String> {
    match e {
        Expr::Call { name, args } if name == QUBIT && args.is_empty() => {
            plan.qubits += 1;
            Ok(())
        }
        Expr::Quant { body } => collect_block(body, true, plan),
        _ => Err(
            "QIR lowering currently supports only `let name = qubit();` inside `quant` blocks"
                .into(),
        ),
    }
}

fn render_module(plan: &QirPlan) -> String {
    let mut out = String::new();
    writeln!(out, "; generated by nialang (QIR backend)").unwrap();
    writeln!(out, "; lowered quantum resources: {} qubit(s)", plan.qubits).unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "target datalayout = \"e-m:e-p270:32:32-p271:32:32-p272:64:64-i64:64-i128:128-f80:128-n8:16:32:64-S128\""
    )
    .unwrap();
    writeln!(out, "target triple = \"unknown-unknown-unknown\"").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "define void @main() #0 {{").unwrap();
    writeln!(out, "entry:").unwrap();
    for id in 0..plan.qubits {
        writeln!(out, "  ; qubit {id}: ptr inttoptr (i64 {id} to ptr)").unwrap();
    }
    writeln!(out, "  ret void").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "attributes #0 = {{ \"entry_point\" \"output_labeling_schema\" \"qir_profiles\"=\"base_profile\" \"required_num_qubits\"=\"{}\" \"required_num_results\"=\"0\" }}",
        plan.qubits
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(out, "; module flags").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "!llvm.module.flags = !{{!0, !1, !2, !3}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "!0 = !{{i32 1, !\"qir_major_version\", i32 2}}").unwrap();
    writeln!(out, "!1 = !{{i32 7, !\"qir_minor_version\", i32 0}}").unwrap();
    writeln!(
        out,
        "!2 = !{{i32 1, !\"dynamic_qubit_management\", i1 false}}"
    )
    .unwrap();
    writeln!(
        out,
        "!3 = !{{i32 1, !\"dynamic_result_management\", i1 false}}"
    )
    .unwrap();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::{Parser, tokenize};
    use crate::semantics::typecheck::collect_sigs;

    fn emit(src: &str) -> String {
        let (structs, enums, fns, vectors) = Parser::new(tokenize(src)).parse_file().unwrap();
        let (_, _, _, fn_sigs) = collect_sigs(&structs, &enums, &vectors, &fns).unwrap();
        emit_module(&structs, &enums, &vectors, &fns, &fn_sigs).unwrap()
    }

    #[test]
    fn qir_counts_qubit_creations_in_quant_blocks() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let a = qubit();
        let b: qubit = qubit();
    }
    0
}
"#,
        );
        assert!(ir.contains("define void @main() #0"), "IR:\n{ir}");
        assert!(ir.contains("\"required_num_qubits\"=\"2\""), "IR:\n{ir}");
        assert!(ir.contains("qir_major_version"), "IR:\n{ir}");
        assert!(ir.contains("dynamic_qubit_management"), "IR:\n{ir}");
    }

    #[test]
    fn qir_rejects_unsupported_quant_body() {
        let (structs, enums, fns, vectors) = Parser::new(tokenize(
            r#"
fn main() i32 {
    quant {
        println("not lowered yet");
    }
    0
}
"#,
        ))
        .parse_file()
        .unwrap();
        let (_, _, _, fn_sigs) = collect_sigs(&structs, &enums, &vectors, &fns).unwrap();
        let err = emit_module(&structs, &enums, &vectors, &fns, &fn_sigs)
            .expect_err("unsupported quantum body");
        assert!(err.contains("only `let name = qubit();`"), "{err}");
    }
}
