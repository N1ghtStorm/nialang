//! Code generation backends.
//!
//! The driver uses [`llvm`] (classical) and [`qir`] (quantum). The [`codegen`]
//! and [`legacy_typecheck`] modules are retained only for unit-test coverage of
//! the pre-rewrite lowering path.

pub mod codegen;
pub mod legacy_typecheck;
pub mod llvm;
pub mod qir;
