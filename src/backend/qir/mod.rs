//! QIR (Quantum Intermediate Representation) backend.
//!
//! Current lowering is intentionally small: it recognizes `qubit()` calls inside
//! `quant { ... }`, assigns them static resource ids, lowers supported quantum
//! gates to QIS intrinsics, lowers `q_measure(q)` to Z-basis measurement,
//! lowers `q_record(r)` to QIR output recording, and emits a Base Profile QIR
//! entry point with matching resource attributes.

use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::{Block, EnumDef, Expr, FnDef, Stmt, StructDef, Ty, VectorDef};
use crate::nia_std::{
    GATE_CCNOT, GATE_CCZ, GATE_CH, GATE_CNOT, GATE_CR1, GATE_CRX, GATE_CRY, GATE_CRZ, GATE_CS,
    GATE_CSDG, GATE_CSWAP, GATE_CT, GATE_CTDG, GATE_CY, GATE_CZ, GATE_H, GATE_I, GATE_R1, GATE_RX,
    GATE_RY, GATE_RZ, GATE_S, GATE_SDG, GATE_SWAP, GATE_T, GATE_TDG, GATE_X, GATE_Y, GATE_Z,
    MEASURE, PI, QUBIT, RECORD,
};
use crate::semantics::typecheck::FnSig;

#[derive(Default)]
struct QirPlan {
    qubits: usize,
    results: usize,
    ops: Vec<QirOp>,
}

enum QirOp {
    GateI(usize),
    GateH(usize),
    GateX(usize),
    GateY(usize),
    GateZ(usize),
    GateS(usize),
    GateSdg(usize),
    GateT(usize),
    GateTdg(usize),
    GateCnot {
        control: usize,
        target: usize,
    },
    GateCz {
        control: usize,
        target: usize,
    },
    GateSwap {
        left: usize,
        right: usize,
    },
    GateControlled {
        gate: QirControlled,
        control: usize,
        target: usize,
    },
    GateThree {
        gate: QirThree,
        first: usize,
        second: usize,
        third: usize,
    },
    GateRotation {
        gate: QirRotation,
        theta: f64,
        qubit: usize,
    },
    GateControlledRotation {
        gate: QirControlledRotation,
        theta: f64,
        control: usize,
        target: usize,
    },
    Measure {
        qubit: usize,
        result: usize,
    },
    Record(usize),
}

#[derive(Clone, Copy)]
enum QirControlled {
    Ch,
    Cy,
    Cs,
    Csdg,
    Ct,
    Ctdg,
}

#[derive(Clone, Copy)]
enum QirThree {
    Ccnot,
    Ccz,
    Cswap,
}

#[derive(Clone, Copy)]
enum QirRotation {
    Rx,
    Ry,
    Rz,
    R1,
}

#[derive(Clone, Copy)]
enum QirControlledRotation {
    Crx,
    Cry,
    Crz,
    Cr1,
}

#[derive(Clone, Default)]
struct QuantResources {
    qubits: HashMap<String, usize>,
    results: HashMap<String, usize>,
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
    fn_sigs: &HashMap<String, FnSig>,
) -> Result<String, String> {
    let main = fns
        .iter()
        .find(|f| f.name == "main")
        .ok_or_else(|| "QIR backend requires a `main` function".to_string())?;

    let mut plan = QirPlan::default();
    collect_block(
        &main.body,
        false,
        &mut plan,
        &mut QuantResources::default(),
        fns,
        fn_sigs,
        &mut Vec::new(),
    )?;
    Ok(render_module(&plan))
}

fn collect_block(
    block: &Block,
    in_quant: bool,
    plan: &mut QirPlan,
    resources: &mut QuantResources,
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    call_stack: &mut Vec<String>,
) -> Result<(), String> {
    for st in &block.stmts {
        collect_stmt(st, in_quant, plan, resources, fns, fn_sigs, call_stack)?;
    }
    if let Some(tail) = &block.tail {
        collect_expr(tail, in_quant, plan, resources, fns, fn_sigs, call_stack)?;
    }
    Ok(())
}

fn collect_stmt(
    st: &Stmt,
    in_quant: bool,
    plan: &mut QirPlan,
    resources: &mut QuantResources,
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    call_stack: &mut Vec<String>,
) -> Result<(), String> {
    match st {
        Stmt::Quant { body } => {
            let mut body_resources = resources.clone();
            collect_block(body, true, plan, &mut body_resources, fns, fn_sigs, call_stack)
        }
        Stmt::Gpu { body } if !in_quant => {
            collect_block(body, false, plan, resources, fns, fn_sigs, call_stack)
        }
        Stmt::Let { name, init, .. } if in_quant => {
            collect_quant_let(name, init, plan, resources, fns, fn_sigs, call_stack)
        }
        Stmt::Expr(e) if in_quant => collect_quant_expr(e, plan, resources, fns, fn_sigs, call_stack),
        Stmt::Let { init, .. } => {
            collect_expr(init, false, plan, resources, fns, fn_sigs, call_stack)
        }
        Stmt::Expr(e) => collect_expr(e, false, plan, resources, fns, fn_sigs, call_stack),
        Stmt::Assign { target, value } if !in_quant => {
            collect_expr(target, false, plan, resources, fns, fn_sigs, call_stack)?;
            collect_expr(value, false, plan, resources, fns, fn_sigs, call_stack)
        }
        Stmt::Return(e) if !in_quant => {
            collect_expr(e, false, plan, resources, fns, fn_sigs, call_stack)
        }
        Stmt::If { cond, then_block } if !in_quant => {
            collect_expr(cond, false, plan, resources, fns, fn_sigs, call_stack)?;
            let mut then_resources = resources.clone();
            collect_block(
                then_block,
                false,
                plan,
                &mut then_resources,
                fns,
                fn_sigs,
                call_stack,
            )
        }
        Stmt::While { cond, body } if !in_quant => {
            collect_expr(cond, false, plan, resources, fns, fn_sigs, call_stack)?;
            let mut body_resources = resources.clone();
            collect_block(body, false, plan, &mut body_resources, fns, fn_sigs, call_stack)
        }
        Stmt::Loop { body } if !in_quant => {
            let mut body_resources = resources.clone();
            collect_block(body, false, plan, &mut body_resources, fns, fn_sigs, call_stack)
        }
        Stmt::For {
            start, end, body, ..
        } if !in_quant => {
            collect_expr(start, false, plan, resources, fns, fn_sigs, call_stack)?;
            collect_expr(end, false, plan, resources, fns, fn_sigs, call_stack)?;
            let mut body_resources = resources.clone();
            collect_block(body, false, plan, &mut body_resources, fns, fn_sigs, call_stack)
        }
        Stmt::Assign { .. }
        | Stmt::Return(_)
        | Stmt::If { .. }
        | Stmt::While { .. }
        | Stmt::Loop { .. }
        | Stmt::Break
        | Stmt::For { .. }
        | Stmt::Gpu { .. } => Err(
            "QIR lowering currently supports only `let q = qubit();`, one- and two-qubit gates, `let r = q_measure(q);`, and `q_record(r);` inside `quant` blocks"
                .into(),
        ),
    }
}

fn collect_expr(
    e: &Expr,
    in_quant: bool,
    plan: &mut QirPlan,
    resources: &mut QuantResources,
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    call_stack: &mut Vec<String>,
) -> Result<(), String> {
    if in_quant {
        return collect_quant_expr(e, plan, resources, fns, fn_sigs, call_stack);
    }

    match e {
        Expr::Quant { body } => {
            let mut body_resources = resources.clone();
            collect_block(
                body,
                true,
                plan,
                &mut body_resources,
                fns,
                fn_sigs,
                call_stack,
            )
        }
        Expr::Gpu { body } => collect_block(body, false, plan, resources, fns, fn_sigs, call_stack),
        Expr::Neg(inner) | Expr::AddrOf(inner) | Expr::Deref(inner) => {
            collect_expr(inner, false, plan, resources, fns, fn_sigs, call_stack)
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
            collect_expr(l, false, plan, resources, fns, fn_sigs, call_stack)?;
            collect_expr(r, false, plan, resources, fns, fn_sigs, call_stack)
        }
        Expr::Call { args, .. } | Expr::GenericCall { args, .. } => {
            for arg in args {
                collect_expr(arg, false, plan, resources, fns, fn_sigs, call_stack)?;
            }
            Ok(())
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_expr(receiver, false, plan, resources, fns, fn_sigs, call_stack)?;
            for arg in args {
                collect_expr(arg, false, plan, resources, fns, fn_sigs, call_stack)?;
            }
            Ok(())
        }
        Expr::StructLit { fields, .. }
        | Expr::VectorLit { fields, .. }
        | Expr::EnumStruct { fields, .. } => {
            for (_, expr) in fields {
                collect_expr(expr, false, plan, resources, fns, fn_sigs, call_stack)?;
            }
            Ok(())
        }
        Expr::AnonVectorLit(elems)
        | Expr::ArrayLit(elems)
        | Expr::EnumTuple { args: elems, .. } => {
            for elem in elems {
                collect_expr(elem, false, plan, resources, fns, fn_sigs, call_stack)?;
            }
            Ok(())
        }
        Expr::Match { scrutinee, arms } => {
            collect_expr(scrutinee, false, plan, resources, fns, fn_sigs, call_stack)?;
            for (_, arm) in arms {
                collect_expr(arm, false, plan, resources, fns, fn_sigs, call_stack)?;
            }
            Ok(())
        }
        Expr::Field(obj, _) => collect_expr(obj, false, plan, resources, fns, fn_sigs, call_stack),
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
    resources: &mut QuantResources,
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    call_stack: &mut Vec<String>,
) -> Result<(), String> {
    match init {
        Expr::Call { name: call, args } if call == QUBIT && args.is_empty() => {
            let id = plan.qubits;
            plan.qubits += 1;
            resources.qubits.insert(name.to_string(), id);
            Ok(())
        }
        Expr::Call { name: call, args } if call == MEASURE && args.len() == 1 => {
            let qubit = qubit_arg_id(&args[0], resources)?;
            let result = plan.results;
            plan.results += 1;
            resources.results.insert(name.to_string(), result);
            plan.ops.push(QirOp::Measure { qubit, result });
            Ok(())
        }
        Expr::Call { name: call, .. } if fn_sigs.get(call).is_some_and(|sig| sig.is_quantum) => {
            Err(
                "QIR lowering currently does not support binding quantum function return values"
                    .into(),
            )
        }
        _ => collect_quant_expr(init, plan, resources, fns, fn_sigs, call_stack),
    }
}

fn collect_quant_expr(
    e: &Expr,
    plan: &mut QirPlan,
    resources: &mut QuantResources,
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    call_stack: &mut Vec<String>,
) -> Result<(), String> {
    match e {
        Expr::Call { name, args } if name == QUBIT && args.is_empty() => {
            plan.qubits += 1;
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_I && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateI(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_H && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateH(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_X && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateX(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_Y && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateY(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_Z && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateZ(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_S && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateS(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_SDG && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateSdg(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_T && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateT(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_TDG && args.len() == 1 => {
            let id = qubit_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::GateTdg(id));
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_CNOT && args.len() == 2 => {
            let control = qubit_arg_id(&args[0], resources)?;
            let target = qubit_arg_id(&args[1], resources)?;
            plan.ops.push(QirOp::GateCnot { control, target });
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_CZ && args.len() == 2 => {
            let control = qubit_arg_id(&args[0], resources)?;
            let target = qubit_arg_id(&args[1], resources)?;
            plan.ops.push(QirOp::GateCz { control, target });
            Ok(())
        }
        Expr::Call { name, args } if name == GATE_SWAP && args.len() == 2 => {
            let left = qubit_arg_id(&args[0], resources)?;
            let right = qubit_arg_id(&args[1], resources)?;
            plan.ops.push(QirOp::GateSwap { left, right });
            Ok(())
        }
        Expr::Call { name, args }
            if (name == GATE_CH
                || name == GATE_CY
                || name == GATE_CS
                || name == GATE_CSDG
                || name == GATE_CT
                || name == GATE_CTDG)
                && args.len() == 2 =>
        {
            let control = qubit_arg_id(&args[0], resources)?;
            let target = qubit_arg_id(&args[1], resources)?;
            let gate = match name.as_str() {
                GATE_CH => QirControlled::Ch,
                GATE_CY => QirControlled::Cy,
                GATE_CS => QirControlled::Cs,
                GATE_CSDG => QirControlled::Csdg,
                GATE_CT => QirControlled::Ct,
                GATE_CTDG => QirControlled::Ctdg,
                _ => unreachable!(),
            };
            plan.ops.push(QirOp::GateControlled {
                gate,
                control,
                target,
            });
            Ok(())
        }
        Expr::Call { name, args }
            if (name == GATE_CCNOT || name == GATE_CCZ || name == GATE_CSWAP) && args.len() == 3 =>
        {
            let first = qubit_arg_id(&args[0], resources)?;
            let second = qubit_arg_id(&args[1], resources)?;
            let third = qubit_arg_id(&args[2], resources)?;
            let gate = match name.as_str() {
                GATE_CCNOT => QirThree::Ccnot,
                GATE_CCZ => QirThree::Ccz,
                GATE_CSWAP => QirThree::Cswap,
                _ => unreachable!(),
            };
            plan.ops.push(QirOp::GateThree {
                gate,
                first,
                second,
                third,
            });
            Ok(())
        }
        Expr::Call { name, args }
            if (name == GATE_RX || name == GATE_RY || name == GATE_RZ || name == GATE_R1)
                && args.len() == 2 =>
        {
            let theta = const_f64_arg(&args[0])?;
            let qubit = qubit_arg_id(&args[1], resources)?;
            let gate = match name.as_str() {
                GATE_RX => QirRotation::Rx,
                GATE_RY => QirRotation::Ry,
                GATE_RZ => QirRotation::Rz,
                GATE_R1 => QirRotation::R1,
                _ => unreachable!(),
            };
            plan.ops.push(QirOp::GateRotation { gate, theta, qubit });
            Ok(())
        }
        Expr::Call { name, args }
            if (name == GATE_CRX || name == GATE_CRY || name == GATE_CRZ || name == GATE_CR1)
                && args.len() == 3 =>
        {
            let theta = const_f64_arg(&args[0])?;
            let control = qubit_arg_id(&args[1], resources)?;
            let target = qubit_arg_id(&args[2], resources)?;
            let gate = match name.as_str() {
                GATE_CRX => QirControlledRotation::Crx,
                GATE_CRY => QirControlledRotation::Cry,
                GATE_CRZ => QirControlledRotation::Crz,
                GATE_CR1 => QirControlledRotation::Cr1,
                _ => unreachable!(),
            };
            plan.ops.push(QirOp::GateControlledRotation {
                gate,
                theta,
                control,
                target,
            });
            Ok(())
        }
        Expr::Call { name, args } if name == RECORD && args.len() == 1 => {
            let id = result_arg_id(&args[0], resources)?;
            plan.ops.push(QirOp::Record(id));
            Ok(())
        }
        Expr::Call { name, args } if fn_sigs.get(name).is_some_and(|sig| sig.is_quantum) => {
            collect_quant_fn_call(name, args, plan, resources, fns, fn_sigs, call_stack)
        }
        Expr::Quant { body } => {
            let mut body_resources = resources.clone();
            collect_block(body, true, plan, &mut body_resources, fns, fn_sigs, call_stack)
        }
        _ => Err(
            "QIR lowering currently supports only `let q = qubit();`, one- and two-qubit gates, `let r = q_measure(q);`, and `q_record(r);` inside `quant` blocks"
                .into(),
        ),
    }
}

fn collect_quant_fn_call(
    name: &str,
    args: &[Expr],
    plan: &mut QirPlan,
    resources: &mut QuantResources,
    fns: &[FnDef],
    fn_sigs: &HashMap<String, FnSig>,
    call_stack: &mut Vec<String>,
) -> Result<(), String> {
    let sig = fn_sigs
        .get(name)
        .ok_or_else(|| format!("unknown quantum function `{name}` in QIR lowering"))?;
    if sig.ret.is_some() {
        return Err(format!(
            "QIR lowering currently supports only void quantum function calls, got `{name}` with a return type"
        ));
    }
    if call_stack.iter().any(|n| n == name) {
        return Err(format!(
            "recursive quantum function `{name}` cannot be lowered to QIR"
        ));
    }
    let f = fns
        .iter()
        .find(|f| f.name == name)
        .ok_or_else(|| format!("missing quantum function `{name}` in QIR lowering"))?;
    if args.len() != sig.params.len() {
        return Err(format!(
            "quantum function `{name}` argument count mismatch during QIR lowering"
        ));
    }

    let mut body_resources = resources.clone();
    for (((param_name, _), param_ty), arg) in f.params.iter().zip(&sig.params).zip(args) {
        match param_ty {
            Ty::Qubit => {
                let id = qubit_arg_id(arg, resources)?;
                body_resources.qubits.insert(param_name.clone(), id);
            }
            Ty::Result => {
                let id = result_arg_id(arg, resources)?;
                body_resources.results.insert(param_name.clone(), id);
            }
            other => {
                return Err(format!(
                    "QIR lowering currently supports only `qubit` and `result` quantum function parameters, got {other:?}"
                ));
            }
        }
    }

    call_stack.push(name.to_string());
    let result = collect_block(
        &f.body,
        true,
        plan,
        &mut body_resources,
        fns,
        fn_sigs,
        call_stack,
    );
    call_stack.pop();
    result
}

fn qubit_arg_id(arg: &Expr, resources: &QuantResources) -> Result<usize, String> {
    let Expr::Ident(name) = arg else {
        return Err("QIR lowering currently supports only variable qubit arguments".into());
    };
    resources
        .qubits
        .get(name)
        .copied()
        .ok_or_else(|| format!("unknown qubit `{name}` in QIR lowering"))
}

fn result_arg_id(arg: &Expr, resources: &QuantResources) -> Result<usize, String> {
    let Expr::Ident(name) = arg else {
        return Err("QIR lowering currently supports only variable result arguments".into());
    };
    resources
        .results
        .get(name)
        .copied()
        .ok_or_else(|| format!("unknown result `{name}` in QIR lowering"))
}

fn render_module(plan: &QirPlan) -> String {
    let mut out = String::new();
    writeln!(out, "; generated by nialang (QIR backend)").unwrap();
    writeln!(
        out,
        "; lowered quantum resources: {} qubit(s), {} result(s)",
        plan.qubits, plan.results
    )
    .unwrap();
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
            QirOp::GateI(id) => {
                writeln!(out, "  ; I({})", qir_qubit_value(*id)).unwrap();
            }
            QirOp::GateH(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__h__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
            QirOp::GateX(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__x__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
            QirOp::GateY(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__y__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
            QirOp::GateZ(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__z__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
            QirOp::GateS(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__s__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
            QirOp::GateSdg(id) => {
                render_rotation_call(&mut out, QirRotation::Rz, -std::f64::consts::FRAC_PI_2, *id);
            }
            QirOp::GateT(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__t__body({})",
                    qir_qubit_value(*id)
                )
                .unwrap();
            }
            QirOp::GateTdg(id) => {
                render_rotation_call(&mut out, QirRotation::Rz, -std::f64::consts::FRAC_PI_4, *id);
            }
            QirOp::GateCnot { control, target } => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__cnot__body({}, {})",
                    qir_qubit_value(*control),
                    qir_qubit_value(*target)
                )
                .unwrap();
            }
            QirOp::GateCz { control, target } => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__cz__body({}, {})",
                    qir_qubit_value(*control),
                    qir_qubit_value(*target)
                )
                .unwrap();
            }
            QirOp::GateSwap { left, right } => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__swap__body({}, {})",
                    qir_qubit_value(*left),
                    qir_qubit_value(*right)
                )
                .unwrap();
            }
            QirOp::GateControlled {
                gate,
                control,
                target,
            } => {
                render_controlled_gate(&mut out, *gate, *control, *target);
            }
            QirOp::GateThree {
                gate,
                first,
                second,
                third,
            } => {
                render_three_qubit_gate(&mut out, *gate, *first, *second, *third);
            }
            QirOp::GateRotation { gate, theta, qubit } => {
                render_rotation_call(&mut out, *gate, *theta, *qubit);
            }
            QirOp::GateControlledRotation {
                gate,
                theta,
                control,
                target,
            } => {
                render_controlled_rotation(&mut out, *gate, *theta, *control, *target);
            }
            QirOp::Measure { qubit, result } => {
                writeln!(
                    out,
                    "  call void @__quantum__qis__mz__body({}, {})",
                    qir_qubit_value(*qubit),
                    qir_result_value(*result)
                )
                .unwrap();
            }
            QirOp::Record(id) => {
                writeln!(
                    out,
                    "  call void @__quantum__rt__result_record_output({}, ptr null)",
                    qir_result_value(*id)
                )
                .unwrap();
            }
        }
    }
    writeln!(out, "  ret void").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "declare void @__quantum__qis__h__body(ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__x__body(ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__y__body(ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__z__body(ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__s__body(ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__t__body(ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__cnot__body(ptr, ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__cz__body(ptr, ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__swap__body(ptr, ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__rx__body(double, ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__ry__body(double, ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__rz__body(double, ptr)").unwrap();
    writeln!(out, "declare void @__quantum__qis__mz__body(ptr, ptr) #1").unwrap();
    writeln!(
        out,
        "declare void @__quantum__rt__result_record_output(ptr, ptr)"
    )
    .unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "attributes #0 = {{ \"entry_point\" \"output_labeling_schema\" \"qir_profiles\"=\"base_profile\" \"required_num_qubits\"=\"{}\" \"required_num_results\"=\"{}\" }}",
        plan.qubits, plan.results
    )
    .unwrap();
    writeln!(out, "attributes #1 = {{ \"irreversible\" }}").unwrap();
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

fn qir_result_value(id: usize) -> String {
    if id == 0 {
        "ptr null".into()
    } else {
        format!("ptr inttoptr (i64 {id} to ptr)")
    }
}

fn render_one_qubit_call(out: &mut String, intrinsic: &str, qubit: usize) {
    writeln!(
        out,
        "  call void @__quantum__qis__{}__body({})",
        intrinsic,
        qir_qubit_value(qubit)
    )
    .unwrap();
}

fn render_two_qubit_call(out: &mut String, intrinsic: &str, left: usize, right: usize) {
    writeln!(
        out,
        "  call void @__quantum__qis__{}__body({}, {})",
        intrinsic,
        qir_qubit_value(left),
        qir_qubit_value(right)
    )
    .unwrap();
}

fn render_rotation_call(out: &mut String, gate: QirRotation, theta: f64, qubit: usize) {
    writeln!(
        out,
        "  call void @__quantum__qis__{}__body(double {}, {})",
        qir_rotation_intrinsic(gate),
        qir_float_value(theta),
        qir_qubit_value(qubit)
    )
    .unwrap();
}

fn render_sdg(out: &mut String, qubit: usize) {
    render_rotation_call(out, QirRotation::Rz, -std::f64::consts::FRAC_PI_2, qubit);
}

fn render_tdg(out: &mut String, qubit: usize) {
    render_rotation_call(out, QirRotation::Rz, -std::f64::consts::FRAC_PI_4, qubit);
}

fn render_controlled_gate(out: &mut String, gate: QirControlled, control: usize, target: usize) {
    match gate {
        QirControlled::Ch => {
            render_rotation_call(out, QirRotation::Ry, -std::f64::consts::FRAC_PI_4, target);
            render_two_qubit_call(out, "cz", control, target);
            render_rotation_call(out, QirRotation::Ry, std::f64::consts::FRAC_PI_4, target);
        }
        QirControlled::Cy => {
            render_sdg(out, target);
            render_two_qubit_call(out, "cnot", control, target);
            render_one_qubit_call(out, "s", target);
        }
        QirControlled::Cs => {
            render_controlled_phase(out, std::f64::consts::FRAC_PI_2, control, target);
        }
        QirControlled::Csdg => {
            render_controlled_phase(out, -std::f64::consts::FRAC_PI_2, control, target);
        }
        QirControlled::Ct => {
            render_controlled_phase(out, std::f64::consts::FRAC_PI_4, control, target);
        }
        QirControlled::Ctdg => {
            render_controlled_phase(out, -std::f64::consts::FRAC_PI_4, control, target);
        }
    }
}

fn render_controlled_phase(out: &mut String, theta: f64, control: usize, target: usize) {
    let half_theta = theta / 2.0;
    render_rotation_call(out, QirRotation::Rz, half_theta, target);
    render_two_qubit_call(out, "cnot", control, target);
    render_rotation_call(out, QirRotation::Rz, -half_theta, target);
    render_two_qubit_call(out, "cnot", control, target);
    render_rotation_call(out, QirRotation::Rz, half_theta, control);
}

fn render_controlled_rotation(
    out: &mut String,
    gate: QirControlledRotation,
    theta: f64,
    control: usize,
    target: usize,
) {
    match gate {
        QirControlledRotation::Crx => {
            render_one_qubit_call(out, "h", target);
            render_controlled_axis_rotation(out, QirRotation::Rz, theta, control, target);
            render_one_qubit_call(out, "h", target);
        }
        QirControlledRotation::Cry => {
            render_controlled_axis_rotation(out, QirRotation::Ry, theta, control, target);
        }
        QirControlledRotation::Crz => {
            render_controlled_axis_rotation(out, QirRotation::Rz, theta, control, target);
        }
        QirControlledRotation::Cr1 => {
            render_controlled_phase(out, theta, control, target);
        }
    }
}

fn render_controlled_axis_rotation(
    out: &mut String,
    gate: QirRotation,
    theta: f64,
    control: usize,
    target: usize,
) {
    let half_theta = theta / 2.0;
    render_rotation_call(out, gate, half_theta, target);
    render_two_qubit_call(out, "cnot", control, target);
    render_rotation_call(out, gate, -half_theta, target);
    render_two_qubit_call(out, "cnot", control, target);
}

fn render_three_qubit_gate(
    out: &mut String,
    gate: QirThree,
    first: usize,
    second: usize,
    third: usize,
) {
    match gate {
        QirThree::Ccnot => render_ccnot(out, first, second, third),
        QirThree::Ccz => {
            render_one_qubit_call(out, "h", third);
            render_ccnot(out, first, second, third);
            render_one_qubit_call(out, "h", third);
        }
        QirThree::Cswap => {
            render_two_qubit_call(out, "cnot", third, second);
            render_ccnot(out, first, second, third);
            render_two_qubit_call(out, "cnot", third, second);
        }
    }
}

fn render_ccnot(out: &mut String, control_a: usize, control_b: usize, target: usize) {
    render_one_qubit_call(out, "h", target);
    render_two_qubit_call(out, "cnot", control_b, target);
    render_tdg(out, target);
    render_two_qubit_call(out, "cnot", control_a, target);
    render_one_qubit_call(out, "t", target);
    render_two_qubit_call(out, "cnot", control_b, target);
    render_tdg(out, target);
    render_two_qubit_call(out, "cnot", control_a, target);
    render_one_qubit_call(out, "t", control_b);
    render_one_qubit_call(out, "t", target);
    render_two_qubit_call(out, "cnot", control_a, control_b);
    render_one_qubit_call(out, "h", target);
    render_one_qubit_call(out, "t", control_a);
    render_tdg(out, control_b);
    render_two_qubit_call(out, "cnot", control_a, control_b);
}

fn qir_rotation_intrinsic(gate: QirRotation) -> &'static str {
    match gate {
        QirRotation::Rx => "rx",
        QirRotation::Ry => "ry",
        QirRotation::Rz => "rz",
        // R1(theta) = diag(1, e^(i theta)); Rz(theta) is equivalent up to a
        // global phase, and the current qir-runner links `rz` but not `r1`.
        QirRotation::R1 => "rz",
    }
}

fn qir_float_value(v: f64) -> String {
    format!("{v:.17e}")
}

fn const_f64_arg(arg: &Expr) -> Result<f64, String> {
    let value = const_f64_expr(arg)?;
    if value.is_finite() {
        Ok(value)
    } else {
        Err("QIR lowering requires finite rotation angles".into())
    }
}

fn const_f64_expr(expr: &Expr) -> Result<f64, String> {
    match expr {
        Expr::Float(v) => Ok(*v),
        Expr::Int(v) => Ok(*v as f64),
        Expr::Ident(name) if name == PI => Ok(std::f64::consts::PI),
        Expr::Neg(inner) => Ok(-const_f64_expr(inner)?),
        Expr::Add(left, right) => Ok(const_f64_expr(left)? + const_f64_expr(right)?),
        Expr::Sub(left, right) => Ok(const_f64_expr(left)? - const_f64_expr(right)?),
        Expr::Mul(left, right) => Ok(const_f64_expr(left)? * const_f64_expr(right)?),
        Expr::Div(left, right) => Ok(const_f64_expr(left)? / const_f64_expr(right)?),
        _ => Err("QIR lowering currently supports only constant f64 rotation angles".into()),
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
        CNOT(a, b);
        let ar = q_measure(a);
        let br: result = q_measure(b);
        q_record(ar);
        q_record(br);
    }
    0
}
"#,
        );
        assert!(ir.contains("define void @main() #0"), "IR:\n{ir}");
        assert!(ir.contains("\"required_num_qubits\"=\"2\""), "IR:\n{ir}");
        assert!(ir.contains("\"required_num_results\"=\"2\""), "IR:\n{ir}");
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains(
                "call void @__quantum__qis__cnot__body(ptr null, ptr inttoptr (i64 1 to ptr))"
            ),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__mz__body(ptr null, ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__mz__body(ptr inttoptr (i64 1 to ptr), ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__rt__result_record_output(ptr null, ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__rt__result_record_output(ptr inttoptr (i64 1 to ptr), ptr null)"),
            "IR:\n{ir}"
        );
        assert!(ir.contains("qir_major_version"), "IR:\n{ir}");
        assert!(ir.contains("dynamic_qubit_management"), "IR:\n{ir}");
    }

    #[test]
    fn qir_inlines_quant_fn_calls() {
        let ir = emit(
            r#"
quant fn prepare(q: qubit) {
    H(q);
    let r = q_measure(q);
    q_record(r);
}

fn main() i32 {
    quant {
        let a = qubit();
        let b: qubit = qubit();
        prepare(a);
        prepare(b);
    }
    0
}
"#,
        );
        assert!(ir.contains("define void @main() #0"), "IR:\n{ir}");
        assert!(ir.contains("\"required_num_qubits\"=\"2\""), "IR:\n{ir}");
        assert!(ir.contains("\"required_num_results\"=\"2\""), "IR:\n{ir}");
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__mz__body(ptr null, ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__mz__body(ptr inttoptr (i64 1 to ptr), ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__rt__result_record_output(ptr null, ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__rt__result_record_output(ptr inttoptr (i64 1 to ptr), ptr null)"),
            "IR:\n{ir}"
        );
        assert!(ir.contains("qir_major_version"), "IR:\n{ir}");
        assert!(ir.contains("dynamic_qubit_management"), "IR:\n{ir}");
    }

    #[test]
    fn qir_lowers_single_qubit_phase_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let y = qubit();
        let z = qubit();
        let s = qubit();
        let t = qubit();
        Y(y);
        Z(z);
        S(s);
        T(t);
    }
    0
}
"#,
        );
        assert!(
            ir.contains("call void @__quantum__qis__y__body(ptr null)"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__z__body(ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__s__body(ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__t__body(ptr inttoptr (i64 3 to ptr))"),
            "IR:\n{ir}"
        );
    }

    #[test]
    fn qir_lowers_identity_and_adjoint_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let i = qubit();
        let s = qubit();
        let t = qubit();
        I(i);
        Sdg(s);
        Tdg(t);
    }
    0
}
"#,
        );
        assert!(ir.contains("; I(ptr null)"), "IR:\n{ir}");
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double -1.57079632679489656e0, ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double -7.85398163397448279e-1, ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
    }

    #[test]
    fn qir_lowers_two_qubit_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let a = qubit();
        let b = qubit();
        let c = qubit();
        CZ(a, b);
        SWAP(b, c);
    }
    0
}
"#,
        );
        assert!(
            ir.contains(
                "call void @__quantum__qis__cz__body(ptr null, ptr inttoptr (i64 1 to ptr))"
            ),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__swap__body(ptr inttoptr (i64 1 to ptr), ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
    }

    #[test]
    fn qir_lowers_controlled_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let c = qubit();
        let h = qubit();
        let y = qubit();
        let s = qubit();
        let sdg = qubit();
        let t = qubit();
        let tdg = qubit();
        CH(c, h);
        CY(c, y);
        CS(c, s);
        CSdg(c, sdg);
        CT(c, t);
        CTdg(c, tdg);
    }
    0
}
"#,
        );
        assert!(
            ir.contains("call void @__quantum__qis__ry__body(double -7.85398163397448279e-1, ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains(
                "call void @__quantum__qis__cz__body(ptr null, ptr inttoptr (i64 1 to ptr))"
            ),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains(
                "call void @__quantum__qis__cnot__body(ptr null, ptr inttoptr (i64 2 to ptr))"
            ),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double 7.85398163397448279e-1, ptr inttoptr (i64 3 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double 3.92699081698724139e-1, ptr inttoptr (i64 5 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double -7.85398163397448279e-1, ptr inttoptr (i64 4 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double -3.92699081698724139e-1, ptr inttoptr (i64 6 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(!ir.contains("__quantum__qis__csdg__body"), "IR:\n{ir}");
        assert!(!ir.contains("__quantum__qis__ctdg__body"), "IR:\n{ir}");
    }

    #[test]
    fn qir_lowers_three_qubit_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let a = qubit();
        let b = qubit();
        let c = qubit();
        CCNOT(a, b, c);
        CCZ(a, b, c);
        CSWAP(a, b, c);
    }
    0
}
"#,
        );
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__cnot__body(ptr inttoptr (i64 1 to ptr), ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__cnot__body(ptr inttoptr (i64 2 to ptr), ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(!ir.contains("__quantum__qis__ccnot__body"), "IR:\n{ir}");
        assert!(!ir.contains("__quantum__qis__ccz__body"), "IR:\n{ir}");
        assert!(!ir.contains("__quantum__qis__cswap__body"), "IR:\n{ir}");
    }

    #[test]
    fn qir_lowers_rotation_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let a = qubit();
        let b = qubit();
        let c = qubit();
        let d = qubit();
        Rx(1.0, a);
        Ry(PI / 2.0, b);
        Rz(-0.25, c);
        R1(0.125 + 0.125, d);
    }
    0
}
"#,
        );
        assert!(
            ir.contains(
                "call void @__quantum__qis__rx__body(double 1.00000000000000000e0, ptr null)"
            ),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__ry__body(double 1.57079632679489656e0, ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double -2.50000000000000000e-1, ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double 2.50000000000000000e-1, ptr inttoptr (i64 3 to ptr))"),
            "IR:\n{ir}"
        );
    }

    #[test]
    fn qir_lowers_controlled_rotation_gates() {
        let ir = emit(
            r#"
fn main() i32 {
    quant {
        let c = qubit();
        let x = qubit();
        let y = qubit();
        let z = qubit();
        let p = qubit();
        CRx(PI / 2.0, c, x);
        CRy(PI / 4.0, c, y);
        CRz(PI / 8.0, c, z);
        CR1(PI, c, p);
    }
    0
}
"#,
        );
        assert!(
            ir.contains("call void @__quantum__qis__h__body(ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double 7.85398163397448279e-1, ptr inttoptr (i64 1 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__ry__body(double 3.92699081698724139e-1, ptr inttoptr (i64 2 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double 1.96349540849362070e-1, ptr inttoptr (i64 3 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains("call void @__quantum__qis__rz__body(double 1.57079632679489656e0, ptr inttoptr (i64 4 to ptr))"),
            "IR:\n{ir}"
        );
        assert!(
            ir.contains(
                "call void @__quantum__qis__cnot__body(ptr null, ptr inttoptr (i64 4 to ptr))"
            ),
            "IR:\n{ir}"
        );
        assert!(!ir.contains("__quantum__qis__crx__body"), "IR:\n{ir}");
        assert!(!ir.contains("__quantum__qis__cry__body"), "IR:\n{ir}");
        assert!(!ir.contains("__quantum__qis__crz__body"), "IR:\n{ir}");
        assert!(!ir.contains("__quantum__qis__cr1__body"), "IR:\n{ir}");
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
        assert!(err.contains("`let r = q_measure(q);`"), "{err}");
        assert!(err.contains("`q_record(r);`"), "{err}");
    }
}
