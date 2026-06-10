//! Quantum builtin elaboration (phase 13).

use crate::core::globals::prim;
use crate::core::quant::QuantKind;
use crate::core::term::Term;
use crate::elab::affine::{check_qubit_available, mark_qubit_measured};
use crate::elab::env::ElabEnv;
use crate::elab::expr::{elab_expr, infer_expr};
use crate::frontend::surface::Expr;
use crate::nia_std::{
    GATE_CCNOT, GATE_CCZ, GATE_CH, GATE_CNOT, GATE_CR1, GATE_CRX, GATE_CRY, GATE_CRZ, GATE_CS,
    GATE_CSDG, GATE_CSWAP, GATE_CT, GATE_CTDG, GATE_CY, GATE_CZ, GATE_H, GATE_I, GATE_R1, GATE_RX,
    GATE_RY, GATE_RZ, GATE_S, GATE_SDG, GATE_SWAP, GATE_T, GATE_TDG, GATE_X, GATE_Y, GATE_Z,
    MEASURE, QUBIT, READ, RECORD,
};

pub fn elab_quantum_call(
    env: &mut ElabEnv,
    name: &str,
    args: &[Expr],
) -> Result<(Term, Term), String> {
    let mk = |kind: QuantKind, terms: Vec<Term>, ret: Term| {
        Ok((Term::Quant { kind, args: terms }, ret))
    };

    match name {
        QUBIT if args.is_empty() => mk(QuantKind::QubitNew, vec![], Term::Global(prim::QUBIT)),
        GATE_I if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateI, vec![a], Term::Global(prim::UNIT))
        }
        GATE_H if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateH, vec![a], Term::Global(prim::UNIT))
        }
        GATE_X if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateX, vec![a], Term::Global(prim::UNIT))
        }
        GATE_Y if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateY, vec![a], Term::Global(prim::UNIT))
        }
        GATE_Z if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateZ, vec![a], Term::Global(prim::UNIT))
        }
        GATE_S if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateS, vec![a], Term::Global(prim::UNIT))
        }
        GATE_SDG if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateSdg, vec![a], Term::Global(prim::UNIT))
        }
        GATE_T if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateT, vec![a], Term::Global(prim::UNIT))
        }
        GATE_TDG if args.len() == 1 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            mk(QuantKind::GateTdg, vec![a], Term::Global(prim::UNIT))
        }
        GATE_CNOT if args.len() == 2 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            let (b, _) = elab_qubit_arg(env, &args[1])?;
            mk(QuantKind::GateCnot, vec![a, b], Term::Global(prim::UNIT))
        }
        GATE_CZ if args.len() == 2 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            let (b, _) = elab_qubit_arg(env, &args[1])?;
            mk(QuantKind::GateCz, vec![a, b], Term::Global(prim::UNIT))
        }
        GATE_SWAP if args.len() == 2 => {
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            let (b, _) = elab_qubit_arg(env, &args[1])?;
            mk(QuantKind::GateSwap, vec![a, b], Term::Global(prim::UNIT))
        }
        GATE_CH | GATE_CY | GATE_CS | GATE_CSDG | GATE_CT | GATE_CTDG if args.len() == 2 => {
            let kind = match name {
                GATE_CH => QuantKind::GateCh,
                GATE_CY => QuantKind::GateCy,
                GATE_CS => QuantKind::GateCs,
                GATE_CSDG => QuantKind::GateCsdg,
                GATE_CT => QuantKind::GateCt,
                GATE_CTDG => QuantKind::GateCtdg,
                _ => unreachable!(),
            };
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            let (b, _) = elab_qubit_arg(env, &args[1])?;
            mk(kind, vec![a, b], Term::Global(prim::UNIT))
        }
        GATE_CCNOT | GATE_CCZ | GATE_CSWAP if args.len() == 3 => {
            let kind = match name {
                GATE_CCNOT => QuantKind::GateCcnot,
                GATE_CCZ => QuantKind::GateCcz,
                GATE_CSWAP => QuantKind::GateCswap,
                _ => unreachable!(),
            };
            let (a, _) = elab_qubit_arg(env, &args[0])?;
            let (b, _) = elab_qubit_arg(env, &args[1])?;
            let (c, _) = elab_qubit_arg(env, &args[2])?;
            mk(kind, vec![a, b, c], Term::Global(prim::UNIT))
        }
        GATE_RX | GATE_RY | GATE_RZ | GATE_R1 if args.len() == 2 => {
            let kind = match name {
                GATE_RX => QuantKind::GateRx,
                GATE_RY => QuantKind::GateRy,
                GATE_RZ => QuantKind::GateRz,
                GATE_R1 => QuantKind::GateR1,
                _ => unreachable!(),
            };
            let (theta, _) = elab_expr(env, &args[0], Some(&Term::Global(prim::F64)))?;
            let (q, _) = elab_qubit_arg(env, &args[1])?;
            mk(kind, vec![theta, q], Term::Global(prim::UNIT))
        }
        GATE_CRX | GATE_CRY | GATE_CRZ | GATE_CR1 if args.len() == 3 => {
            let kind = match name {
                GATE_CRX => QuantKind::GateCrx,
                GATE_CRY => QuantKind::GateCry,
                GATE_CRZ => QuantKind::GateCrz,
                GATE_CR1 => QuantKind::GateCr1,
                _ => unreachable!(),
            };
            let (theta, _) = elab_expr(env, &args[0], Some(&Term::Global(prim::F64)))?;
            let (c, _) = elab_qubit_arg(env, &args[1])?;
            let (t, _) = elab_qubit_arg(env, &args[2])?;
            mk(kind, vec![theta, c, t], Term::Global(prim::UNIT))
        }
        MEASURE if args.len() == 1 => {
            let (q, _) = elab_qubit_arg(env, &args[0])?;
            mark_qubit_measured(env, &args[0])?;
            mk(QuantKind::Measure, vec![q], Term::Global(prim::RESULT))
        }
        READ if args.len() == 1 => {
            let (r, _) = elab_result_arg(env, &args[0])?;
            mk(QuantKind::Read, vec![r], Term::Global(prim::BOOL))
        }
        RECORD if args.len() == 1 => {
            let (arg, _) = infer_expr(env, &args[0])?;
            mk(QuantKind::Record, vec![arg], Term::Global(prim::UNIT))
        }
        _ => Err(format!("unsupported quantum operation `{name}`")),
    }
}

fn elab_qubit_arg(env: &mut ElabEnv, expr: &Expr) -> Result<(Term, Term), String> {
    check_qubit_available(env, expr)?;
    elab_expr(env, expr, Some(&Term::Global(prim::QUBIT)))
}

fn elab_result_arg(env: &mut ElabEnv, expr: &Expr) -> Result<(Term, Term), String> {
    elab_expr(env, expr, Some(&Term::Global(prim::RESULT)))
}
