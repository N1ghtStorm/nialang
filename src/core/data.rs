use std::collections::HashMap;

use crate::core::term::Term;
use crate::frontend::resolve::DefId;

/// Field layout for a resolved struct type.
#[derive(Debug, Clone)]
pub struct StructInfo {
    pub fields: Vec<Term>,
}

/// One enum variant after elaboration.
#[derive(Debug, Clone)]
pub enum VariantFields {
    Unit,
    Tuple(Vec<Term>),
    Struct(Vec<Term>),
}

/// Field layout for a resolved enum type.
#[derive(Debug, Clone)]
pub struct EnumInfo {
    pub variants: Vec<VariantFields>,
}

#[derive(Debug, Clone)]
pub struct ArrayInfo {
    pub elem: Term,
    pub len: u32,
}

#[derive(Debug, Clone)]
pub struct PtrInfo {
    pub inner: Term,
}

#[derive(Debug, Clone)]
pub struct MatrixInfo {
    pub elem: Term,
}

/// One constructor of an inductive family.
#[derive(Debug, Clone)]
pub struct InductiveCtor {
    pub name: String,
    pub ty: Term,
    pub result: Term,
}

/// Metadata for a dependent inductive family (`Nat`, `Vec`, …).
#[derive(Debug, Clone)]
pub struct InductiveInfo {
    pub params: Vec<crate::core::term::Binder>,
    pub indices: Vec<crate::core::term::Binder>,
    pub constructors: Vec<InductiveCtor>,
}

/// Nominal metadata for struct and enum globals used by the Core checker.
#[derive(Debug, Clone, Default)]
pub struct DataEnv {
    pub structs: HashMap<DefId, StructInfo>,
    pub enums: HashMap<DefId, EnumInfo>,
    pub arrays: HashMap<DefId, ArrayInfo>,
    pub ptrs: HashMap<DefId, PtrInfo>,
    pub matrices: HashMap<DefId, MatrixInfo>,
    pub inductives: HashMap<DefId, InductiveInfo>,
}

impl DataEnv {
    pub fn struct_fields(&self, id: DefId) -> Option<&[Term]> {
        self.structs.get(&id).map(|s| s.fields.as_slice())
    }

    pub fn enum_variants(&self, id: DefId) -> Option<&[VariantFields]> {
        self.enums.get(&id).map(|e| e.variants.as_slice())
    }

    pub fn variant_fields(&self, enum_id: DefId, variant: u32) -> Option<&VariantFields> {
        self.enums
            .get(&enum_id)
            .and_then(|e| e.variants.get(variant as usize))
    }

    pub fn array_info(&self, id: DefId) -> Option<&ArrayInfo> {
        self.arrays.get(&id)
    }

    pub fn ptr_info(&self, id: DefId) -> Option<&PtrInfo> {
        self.ptrs.get(&id)
    }

    pub fn matrix_info(&self, id: DefId) -> Option<&MatrixInfo> {
        self.matrices.get(&id)
    }

    pub fn inductive(&self, id: DefId) -> Option<&InductiveInfo> {
        self.inductives.get(&id)
    }

    pub fn inductive_ctor(&self, id: DefId, variant: u32) -> Option<&InductiveCtor> {
        self.inductives
            .get(&id)
            .and_then(|info| info.constructors.get(variant as usize))
    }
}
