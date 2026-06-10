//! Nialang compiler: parse → resolve → elaborate → erase → HIR → LLVM/QIR.

pub mod ast;
pub mod backend;
pub mod core;
pub mod driver;
pub mod elab;
pub mod erase;
pub mod hir;
pub mod frontend;
pub mod lexer;
pub mod nia_std;
pub mod parser;
pub mod verify;
