//! Classical (non-quantum) HIR consumed by the LLVM backend.

use crate::erase::ErasedModule;
use crate::frontend::resolve::ResolvedModule;

/// Runtime module ready for LLVM lowering.
#[derive(Debug, Clone)]
pub struct ClassicalModule {
    pub resolved: ResolvedModule,
    pub erased: ErasedModule,
}

pub fn lower(resolved: ResolvedModule, erased: ErasedModule) -> ClassicalModule {
    ClassicalModule { resolved, erased }
}
