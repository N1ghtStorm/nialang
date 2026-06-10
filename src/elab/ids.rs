use crate::frontend::resolve::{DefId, TypeDefKind};

pub const TYPE_BASE: u32 = 0x0100_0000;
pub const FN_BASE: u32 = 0x0200_0000;
pub const BUILTIN_BASE: u32 = 0x0300_0000;
pub const ARRAY_BASE: u32 = 0x0400_0000;
pub const PTR_BASE: u32 = 0x0500_0000;
pub const MATRIX_BASE: u32 = 0x0600_0000;

pub fn type_gid(kind: TypeDefKind) -> DefId {
    let (tag, resolved) = match kind {
        TypeDefKind::Struct(id) => (0, id),
        TypeDefKind::Enum(id) => (1, id),
        TypeDefKind::Vector(id) => (2, id),
    };
    DefId(TYPE_BASE + tag * 0x10000 + resolved.0)
}

pub fn fn_gid(resolved: DefId) -> DefId {
    DefId(FN_BASE + resolved.0)
}

pub fn builtin_gid(index: u32) -> DefId {
    DefId(BUILTIN_BASE + index)
}

pub fn array_gid(elem: DefId, len: u32) -> DefId {
    DefId(ARRAY_BASE + (elem.0 & 0xFFFF) * 0x1000 + len)
}

pub fn ptr_gid(inner: DefId) -> DefId {
    DefId(PTR_BASE + (inner.0 & 0xFFFFFF))
}

pub fn matrix_gid(elem: DefId) -> DefId {
    DefId(MATRIX_BASE + (elem.0 & 0xFFFFFF))
}

pub fn inductive_gid(index: u32) -> DefId {
    crate::core::inductive::inductive_gid(index)
}
