//! Name resolution for surface modules.

mod ids;
mod module;

pub use ids::{ConstructorId, DefId, EffectId, LocalId};
pub use module::{
    format_resolved_module, resolve_module, ConstructorInfo, ResolvedEnum, ResolvedFn,
    ResolvedModule, ResolvedStruct, ResolvedVector, TypeDefKind,
};
