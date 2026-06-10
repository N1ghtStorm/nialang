//! High-level IR after erasure (phase 8.2).

pub mod classical;
pub mod format;
pub mod quantum;

pub use classical::{lower as lower_classical, ClassicalModule};
pub use format::{format_classical_module, format_erased_module};
pub use quantum::{emit_qir, lower as lower_quantum, QuantumModule};
