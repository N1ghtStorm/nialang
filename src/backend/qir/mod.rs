//! QIR (Quantum Intermediate Representation) backend.
//!
//! Current lowering is intentionally small: it recognizes `qubit()` calls inside
//! `quant { ... }`, assigns them static resource ids, lowers `H(q)` to the
//! QIS Hadamard intrinsic, and emits a Base Profile QIR entry point with the
//! matching `required_num_qubits` attribute. Measurements and result recording
//! are follow-up work.

use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::{Block, EnumDef, Expr, FnDef, Stmt, StructDef, VectorDef};
use crate::nia_std::{GATE_H, QUBIT};
use crate::semantics::typecheck::FnSig;

#[derive(Default)]
struct QirPlan {
    qubits: usize,
    ops: Vec<QirOp>,
}

enum QirOp {
    GateH(usize),
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
    collect_block(&main.body, false, &mut plan, &mut HashMap::new())?;
    Ok(render_module(&plan))
}

fn collect_block(
    block: &Block,
    in_quant: bool,
    plan: &mut QirPlan,
    qubits: &mut HashMap<String, usize>,
) -> Result<(), String> {
    for st in &block.stmts {
        collect_stmt(st, in_quant, plan, qubits)?;
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, in_quant, plan, qubits)?;
    }
    Ok(())
}

fn collect_stmt(
    st: &Stmt,
    in_quant: bool,
    plan: &mut QirPlan,
    qubits: &mut HashMap<String, usize>,
) -> Result<(), String> {
    match st {
        Stmt::Quant { body } => {
            let mut body_qubits = qubits.clone();
            collect_block(body, true, plan, &mut body_qubits)
        }
        Stmt::Gpu { body } if !in_quant => collect_block(body, false, plan, qubits),
        Stmt::Let { name, init, .. } if in_quant => collect_quant_let(name, init, plan, qubits),
        Stmt::Expr(e) if in_quant => collect_quant_expr(e, plan, qubits),
        Stmt::Let { init, .. } => collect_expr(init, false, plan, qubits),
        Stmt::Expr(e) => collect_expr(e, false, plan, qubits),
        Stmt::Assign { target, value } if !in_quant => {
            collect_expr(target, false, plan, qubits)?;
            collect_expr(value, false, plan, qubits)
        }
        Stmt::Return(e) if !in_quant => collect_expr(e, false, plan, qubits),
        Stmt::If { cond, then_block } if !in_quant => {
            collect_expr(cond, false, plan, qubits)?;
            let mut then_qubits = qubits.clone();
            collect_block(then_block, false, plan, &mut then_qubits)
        }
        Stmt::While { cond, body } if !in_quant => {
            collect_expr(cond, false, plan, qubits)?;
            let mut body_qubits = qubits.clone();
            collect_block(body, false, plan, &mut body_qubits)
        }
        Stmt::Loop { body } if !in_quant => {
            let mut body_qubits = qubits.clone();
            collect_block(body, false, plan, &mut body_qubits)
        }
        Stmt::For {
            start, end, body, ..
        } if !in_quant => {
            collect_expr(start, false, plan, qubits)?;
            collect_expr(end, false, plan, qubits)?;
            let mut body_qubits = qubits.clone();
            collect_block(body, false, plan, &mut body_qubits)
        }
        Stmt::Assign { .. }
        | Stmt::Return(_)
        | Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::Loop { .. }
        | Stmt::Break
        | Stmt::For { .. }
        | Stmt::Gpu { .. } => Err(
            "QIR lowering currently supports only `let name = qubit();` and `H(name);` inside `quant` blocks"
                .into(),
        ),
    }
}

fn collect_expr(
    e: &Expr,
    in_quant: bool,
    plan: &mut QirPlan,
    qubits: &mut HashMap<String, usize>,
) -> Result<(), String> {
    if in_quant {
        return collect_quant_expr(e, plan, qubits);
    }

    match e {
        Expr::Quant { body } => {
            let mut body_qubits = qubits.clone();
            collect_block(body, true, plan, &mut body_qubits)
        }
        Expr::Gpu { body } => collect_block(body, false, plan, qubits),
        Expr::Neg(inner) | Expr::AddrOf(inner) | Expr::Deref(inner) => {
            collect_expr(inner, false, plan, qubits)
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
            collect_expr(l, false, plan, qubits)?;
            collect_expr(r, false, plan, qubits)
        }
        Expr::Call { args, .. } | Expr::GenericCall { args, .. } => {
            for arg in args {
                collect_expr(arg, false, plan, qubits)?;
            }
            Ok(())
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_expr(receiver, false, plan, qubits)?;
            for arg in args {
                collect_expr(arg, false, plan, qubits)?;
            }
            Ok(())
        }
        Expr::StructLit { fields, .. }
        | Expr::VectorLit { fields, .. }
        | Expr::EnumStruct { fields, .. } => {
            for (_, expr) in fields {
                collect_expr(expr, false, plan, qubits)?;
            }
            Ok(())
        }
        Expr::AnonVectorLit(elems)
        | Expr::ArrayLit(elems)
        | Expr::EnumTuple { args: elems, .. } => {
            for elem in elems {
                collect_expr(elem, false, plan, qubits)?;
            }
            Ok(())
        }
        Expr::Match { scrutinee, arms } => {
            collect_expr(scrutinee, false, plan, qubits)?;
            for (_, arm) in arms {
                collect_expr(arm, false, plan, qubits)?;
            }
            Ok(())
        }
        Expr::Field(obj, _) => collect_expr(obj, false, plan, qubits),
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::String(_)
        | Expr::Ident(_)
        | Expr::EnumVariant { .. } => Ok(()),
    }
}

fn collect_quant_let(
    name: &str,
    init: &Expr,
    plan: &mut QirPlan,
    qubits: &mut HashMap<String, usize>,
) -> Result<(), String> {
    match init {
        Expr::Call { name: call, args } if call == QUBIT && args.is_empty() => {
            let id = plan.qubits;
            plan.qubits += 1;
            qubits.insert(name.to_string(), id);
            Ok(())
        }
        _ => collect_quant_expr(init, plan, qubits),
    }
}

fn collect_quant_expr(
    e: &Expr,
    plan: &mut QirPlan,
    qubits: &HashMap<String, usize>,
) -> Result<(), String> {
    match e {
        Expr::Call { name, args } if name == QUBIT && args.is_empty() => {
            plan.qubits += 1;
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_H && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], qubits)?;
            plan.ops.push(QirOp::GateH(id));
            Ok(())
        }
        Expr::Quant { body } => {
            let mut body_qubits = qubits.clone();
            collect_block(body, true, plan, &mut body_qubits)
        }
        _ => Err(
            "QIR lowering currently supports only `let name = qubit();` and `H(name);` inside `quant` blocks"
                .into(),
        ),
    }
}

fn qubit_arg_id(arg: &Expr, qubits: &HashMap<String, usize>) -> Result<usize, String> {
    let Expr::Ident(name) = arg else {
        return Err("QIR lowering currently supports only variable qubit arguments".into());
    };
    qubits
        .get(name)
        .copied()
        .ok_or_else(|| format!("unknown qubit `{name}` in QIR lowering"))
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
    for op in &plan.ops {
        match op {
            QirOp::GateH(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__h__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
        }
    }
    writeln!(out, "  ret void").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "declare void @__quantum__qis__h__body(ptr)").unwrap();
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

fn qir_qubit_value(id: usize) -> String {
    if id == 0 {
        "ptr null".into()
    } else {
        format!("ptr inttoptr (i64 {id} to ptr)")
    }
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
        H(a);
        H(b);
    }
    0
}
"#,
        );
        assert!(ir.contains("define void @main() #0"), "IR:\n{ir}");
        assert!(ir.contains("\"required_num_qubits\"=\"2\""), "IR:\n{ir}");
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
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
        assert!(err.contains("H(name)"), "{err}");
    }
}
