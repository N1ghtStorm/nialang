//! Pretty-printing for classical HIR after erasure.

use crate::erase::ErasedModule;
use crate::hir::classical::ClassicalModule;

pub fn format_classical_module(module: &ClassicalModule) -> String {
    let mut out = String::from(";; nialang classical hir\n");
    out.push_str(&format_erased_module(&module.erased));
    out
}

pub fn format_erased_module(module: &ErasedModule) -> String {
    let mut out = String::from(";; erased runtime module\n");
    if !module.strings.is_empty() {
        out.push_str(&format!(";; strings ({})\n", module.strings.len()));
        for (i, s) in module.strings.iter().enumerate() {
            out.push_str(&format!(";;   [{i}] {s:?}\n"));
        }
    }
    for f in &module.fns {
        out.push_str(&format!("\n;; fn {} -> {:?}\n", f.name, f.ret));
        for (name, ty) in &f.params {
            out.push_str(&format!(";;   param {name}: {ty:?}\n"));
        }
        out.push_str(&format!("{:#?}\n", f.body));
    }
    out
}
