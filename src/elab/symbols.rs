use std::collections::HashMap;

use crate::core::globals::prim;
use crate::frontend::resolve::DefId;

/// Runtime type carried through erasure and codegen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeTy {
    I8,
    U8,
    I16,
    U16,
    I32,
    I64,
    U64,
    I128,
    U128,
    F16,
    F32,
    F64,
    String,
    Bool,
    Unit,
    Struct(String),
    Enum(String),
    Array {
        elem: Box<RuntimeTy>,
        len: u32,
    },
    Ptr(Box<RuntimeTy>),
    Matrix {
        elem: Box<RuntimeTy>,
    },
    Qubit,
    Result,
}

impl RuntimeTy {
    pub fn from_prim(id: DefId) -> Option<Self> {
        match id {
            prim::I8 => Some(Self::I8),
            prim::U8 => Some(Self::U8),
            prim::I16 => Some(Self::I16),
            prim::U16 => Some(Self::U16),
            prim::I32 => Some(Self::I32),
            prim::I64 => Some(Self::I64),
            prim::U64 => Some(Self::U64),
            prim::I128 => Some(Self::I128),
            prim::U128 => Some(Self::U128),
            prim::F16 => Some(Self::F16),
            prim::F32 => Some(Self::F32),
            prim::F64 => Some(Self::F64),
            prim::STRING => Some(Self::String),
            prim::BOOL => Some(Self::Bool),
            prim::UNIT => Some(Self::Unit),
            prim::QUBIT => Some(Self::Qubit),
            prim::RESULT => Some(Self::Result),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmpOp {
    Lt,
    Gt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeBuiltin {
    BinOp(BinOp, RuntimeTy),
    Cmp(CmpOp, RuntimeTy),
    StrEq,
    If(RuntimeTy),
    Println(RuntimeTy),
}

/// Maps elaborator `DefId`s to runtime codegen symbols.
#[derive(Debug, Clone, Default)]
pub struct CodegenSymbols {
    pub fns: HashMap<DefId, String>,
    pub structs: HashMap<DefId, String>,
    pub enums: HashMap<DefId, String>,
    pub arrays: HashMap<DefId, RuntimeTy>,
    pub ptrs: HashMap<DefId, RuntimeTy>,
    pub matrices: HashMap<DefId, RuntimeTy>,
    pub builtins: HashMap<DefId, RuntimeBuiltin>,
    pub fn_rets: HashMap<String, RuntimeTy>,
}
