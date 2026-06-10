//! Compatibility re-exports of the surface AST.
//!
//! New code should import from [`crate::frontend::surface`]. This thin shim
//! remains for QIR lowering and internal surface-type helpers during cleanup.

pub use crate::frontend::surface::{
    method_symbol, Block, EnumDef, EnumVariantDef, EnumVariantFields, Expr, FnDef, MatchPattern,
    Stmt, StructDef, SurfaceModule, SurfaceTy, VectorDef,
};

/// Legacy alias retained for the old typechecker and codegen during migration.
pub type Ty = SurfaceTy;
