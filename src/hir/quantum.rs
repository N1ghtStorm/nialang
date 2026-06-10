//! Quantum HIR consumed by the QIR backend (phase 13).

use crate::backend::qir;
use crate::frontend::resolve::ResolvedModule;

/// Resolved surface module validated by the new elaborator; QIR lowering reads `main`
/// and `quant fn` bodies from the embedded surface AST.
#[derive(Debug, Clone)]
pub struct QuantumModule {
    pub resolved: ResolvedModule,
}

pub fn lower(resolved: ResolvedModule) -> QuantumModule {
    QuantumModule { resolved }
}

pub fn emit_qir(module: &QuantumModule) -> Result<String, String> {
    qir::emit_from_resolved(&module.resolved)
}
