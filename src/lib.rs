//! Nialang compiler: AST, lexer, parser, type checking, LLVM IR codegen, and driver.

pub mod ast;
pub mod backend;
pub mod driver;
pub mod frontend;
pub mod lexer;
pub mod nia_std;
pub mod parser;
pub mod semantics;
