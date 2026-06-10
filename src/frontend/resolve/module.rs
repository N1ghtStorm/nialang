use std::collections::HashMap;

use crate::frontend::resolve::ids::{ConstructorId, DefId};
use crate::frontend::surface::{EnumDef, FnDef, StructDef, SurfaceModule, VectorDef};
use crate::nia_std::{builtin_structs, is_reserved_fn_name, is_reserved_type_name};

/// Which top-level type definition a source name refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeDefKind {
    Struct(DefId),
    Enum(DefId),
    Vector(DefId),
}

/// Metadata for one enum constructor after resolution.
#[derive(Debug, Clone)]
pub struct ConstructorInfo {
    pub id: ConstructorId,
    pub enum_id: DefId,
    pub enum_name: String,
    pub variant_name: String,
    pub variant_index: usize,
}

#[derive(Debug, Clone)]
pub struct ResolvedStruct {
    pub id: DefId,
    pub name: String,
    pub def: StructDef,
    pub is_builtin: bool,
}

#[derive(Debug, Clone)]
pub struct ResolvedEnum {
    pub id: DefId,
    pub name: String,
    pub def: EnumDef,
}

#[derive(Debug, Clone)]
pub struct ResolvedVector {
    pub id: DefId,
    pub name: String,
    pub def: VectorDef,
}

#[derive(Debug, Clone)]
pub struct ResolvedFn {
    pub id: DefId,
    pub name: String,
    pub def: FnDef,
}

/// Surface module plus stable top-level identifiers and lookup tables.
#[derive(Debug, Clone)]
pub struct ResolvedModule {
    pub surface: SurfaceModule,
    pub structs: Vec<ResolvedStruct>,
    pub enums: Vec<ResolvedEnum>,
    pub vectors: Vec<ResolvedVector>,
    pub fns: Vec<ResolvedFn>,
    pub constructors: Vec<ConstructorInfo>,
    pub type_names: HashMap<String, TypeDefKind>,
    pub fn_names: HashMap<String, DefId>,
}

impl ResolvedModule {
    pub fn lookup_type(&self, name: &str) -> Option<TypeDefKind> {
        self.type_names.get(name).copied()
    }

    pub fn lookup_fn(&self, name: &str) -> Option<DefId> {
        self.fn_names.get(name).copied()
    }

    pub fn struct_by_id(&self, id: DefId) -> Option<&ResolvedStruct> {
        self.structs.get(id.index())
    }

    pub fn enum_by_id(&self, id: DefId) -> Option<&ResolvedEnum> {
        self.enums.get(id.index())
    }

    pub fn vector_by_id(&self, id: DefId) -> Option<&ResolvedVector> {
        self.vectors.get(id.index())
    }

    pub fn fn_by_id(&self, id: DefId) -> Option<&ResolvedFn> {
        self.fns.get(id.index())
    }

    pub fn constructor_by_id(&self, id: ConstructorId) -> Option<&ConstructorInfo> {
        self.constructors.get(id.index())
    }
}

/// Assigns stable `DefId`s to top-level items and validates the module name map.
pub fn resolve_module(mut surface: SurfaceModule) -> Result<ResolvedModule, String> {
    let user_structs = std::mem::take(&mut surface.structs);
    let user_enums = std::mem::take(&mut surface.enums);
    let user_vectors = std::mem::take(&mut surface.vectors);
    let user_fns = std::mem::take(&mut surface.fns);

    let mut out = ResolvedModule {
        surface,
        structs: Vec::new(),
        enums: Vec::new(),
        vectors: Vec::new(),
        fns: Vec::new(),
        constructors: Vec::new(),
        type_names: HashMap::new(),
        fn_names: HashMap::new(),
    };

    for builtin in builtin_structs() {
        out.register_struct(builtin, true)?;
    }

    for item in user_structs {
        out.register_struct(item, false)?;
    }
    for item in user_vectors {
        out.register_vector(item)?;
    }
    for item in user_enums {
        out.register_enum(item)?;
    }
    for item in user_fns {
        out.register_fn(item)?;
    }

    Ok(out)
}

impl ResolvedModule {
    fn register_struct(&mut self, def: StructDef, is_builtin: bool) -> Result<(), String> {
        if !is_builtin && is_reserved_type_name(&def.name) {
            return Err(format!("type name `{}` is reserved", def.name));
        }
        if self.type_names.contains_key(&def.name) {
            return Err(format!("duplicate struct {}", def.name));
        }

        let id = DefId(self.structs.len() as u32);
        self.type_names
            .insert(def.name.clone(), TypeDefKind::Struct(id));
        self.structs.push(ResolvedStruct {
            id,
            name: def.name.clone(),
            def,
            is_builtin,
        });
        Ok(())
    }

    fn register_vector(&mut self, def: VectorDef) -> Result<(), String> {
        if is_reserved_type_name(&def.name) {
            return Err(format!("type name `{}` is reserved", def.name));
        }
        if self.type_names.contains_key(&def.name) {
            return Err(format!("duplicate type name {}", def.name));
        }

        let id = DefId(self.vectors.len() as u32);
        self.type_names
            .insert(def.name.clone(), TypeDefKind::Vector(id));
        self.vectors.push(ResolvedVector {
            id,
            name: def.name.clone(),
            def,
        });
        Ok(())
    }

    fn register_enum(&mut self, def: EnumDef) -> Result<(), String> {
        if is_reserved_type_name(&def.name) {
            return Err(format!("type name `{}` is reserved", def.name));
        }
        if self.type_names.contains_key(&def.name) {
            return Err(format!("duplicate type name {}", def.name));
        }

        let id = DefId(self.enums.len() as u32);
        self.type_names
            .insert(def.name.clone(), TypeDefKind::Enum(id));
        self.register_constructors(id, &def)?;
        self.enums.push(ResolvedEnum {
            id,
            name: def.name.clone(),
            def,
        });
        Ok(())
    }

    fn register_constructors(&mut self, enum_id: DefId, def: &EnumDef) -> Result<(), String> {
        let mut seen = HashMap::new();
        for (variant_index, variant) in def.variants.iter().enumerate() {
            if seen.insert(variant.name.clone(), ()).is_some() {
                return Err(format!(
                    "duplicate enum variant `{}` in enum `{}`",
                    variant.name, def.name
                ));
            }
            self.constructors.push(ConstructorInfo {
                id: ConstructorId(self.constructors.len() as u32),
                enum_id,
                enum_name: def.name.clone(),
                variant_name: variant.name.clone(),
                variant_index,
            });
        }
        Ok(())
    }

    fn register_fn(&mut self, def: FnDef) -> Result<(), String> {
        if is_reserved_fn_name(&def.name) {
            return Err(format!(
                "function name `{}` is reserved for the standard library",
                def.name
            ));
        }
        if def.is_quantum && def.is_extern {
            return Err(format!(
                "function `{}` cannot be both `quant` and `extern`",
                def.name
            ));
        }
        if self.fn_names.contains_key(&def.name) {
            return Err(format!("duplicate function {}", def.name));
        }

        let id = DefId(self.fns.len() as u32);
        self.fn_names.insert(def.name.clone(), id);
        self.fns.push(ResolvedFn {
            id,
            name: def.name.clone(),
            def,
        });
        Ok(())
    }
}

/// Pretty-prints resolved top-level symbols for debugging.
pub fn format_resolved_module(module: &ResolvedModule) -> String {
    let mut out = String::from(";; nialang resolved module\n");

    if !module.structs.is_empty() {
        out.push_str(&format!("\n;; structs ({})\n", module.structs.len()));
        for item in &module.structs {
            let tag = if item.is_builtin { "builtin" } else { "user" };
            out.push_str(&format!(
                ";;   DefId({}) {} [{tag}]\n",
                item.id.0, item.name
            ));
        }
    }

    if !module.vectors.is_empty() {
        out.push_str(&format!("\n;; vectors ({})\n", module.vectors.len()));
        for item in &module.vectors {
            out.push_str(&format!(";;   DefId({}) {}\n", item.id.0, item.name));
        }
    }

    if !module.enums.is_empty() {
        out.push_str(&format!("\n;; enums ({})\n", module.enums.len()));
        for item in &module.enums {
            out.push_str(&format!(";;   DefId({}) {}\n", item.id.0, item.name));
        }
    }

    if !module.constructors.is_empty() {
        out.push_str(&format!(
            "\n;; constructors ({})\n",
            module.constructors.len()
        ));
        for ctor in &module.constructors {
            out.push_str(&format!(
                ";;   ConstructorId({}) {}::{}\n",
                ctor.id.0, ctor.enum_name, ctor.variant_name
            ));
        }
    }

    if !module.fns.is_empty() {
        out.push_str(&format!("\n;; functions ({})\n", module.fns.len()));
        for item in &module.fns {
            out.push_str(&format!(";;   DefId({}) {}\n", item.id.0, item.name));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontend::parser::{Parser, tokenize};

    fn resolve_src(src: &str) -> Result<ResolvedModule, String> {
        let surface = Parser::new(tokenize(src)).parse_file().map_err(|e| e)?;
        resolve_module(surface)
    }

    #[test]
    fn resolve_assigns_distinct_def_ids() {
        let module = resolve_src(
            r#"
struct Point { x: i32, y: i32 }
enum Shape { Dot, Circle(i32) }
vector V2 i32 [ X, Y ]
fn main() i32 { 0 }
"#,
        )
        .expect("resolve");

        assert_eq!(module.structs.len(), 2);
        assert!(module.struct_by_id(DefId(0)).expect("builtin").is_builtin);
        assert_eq!(
            module.struct_by_id(DefId(1)).expect("point").name,
            "Point"
        );
        assert_eq!(module.enums[0].id, DefId(0));
        assert_eq!(module.vectors[0].id, DefId(0));
        assert_eq!(module.fns[0].id, DefId(0));
        assert_eq!(module.constructors.len(), 2);
    }

    #[test]
    fn resolve_rejects_duplicate_struct_name() {
        let err = resolve_src(
            r#"
struct Point { x: i32 }
struct Point { y: i32 }
fn main() i32 { 0 }
"#,
        )
        .expect_err("duplicate struct");
        assert!(err.contains("duplicate struct Point"), "{err}");
    }

    #[test]
    fn resolve_rejects_struct_vector_name_collision() {
        let err = resolve_src(
            r#"
struct Point { x: i32 }
vector Point i32 [ X, Y, Z ]
fn main() i32 { 0 }
"#,
        )
        .expect_err("collision");
        assert!(err.contains("duplicate type name Point"), "{err}");
    }

    #[test]
    fn resolve_rejects_enum_struct_name_collision() {
        let err = resolve_src(
            r#"
struct Shape { x: i32 }
enum Shape { Dot }
fn main() i32 { 0 }
"#,
        )
        .expect_err("collision");
        assert!(err.contains("duplicate type name Shape"), "{err}");
    }

    #[test]
    fn resolve_rejects_duplicate_function_name() {
        let err = resolve_src(
            r#"
fn foo() i32 { 0 }
fn foo() i32 { 1 }
"#,
        )
        .expect_err("duplicate fn");
        assert!(err.contains("duplicate function foo"), "{err}");
    }

    #[test]
    fn resolve_rejects_duplicate_enum_variant() {
        let err = resolve_src(
            r#"
enum Shape { Dot, Dot }
fn main() i32 { 0 }
"#,
        )
        .expect_err("duplicate variant");
        assert!(
            err.contains("duplicate enum variant `Dot` in enum `Shape`"),
            "{err}"
        );
    }

    #[test]
    fn resolve_rejects_reserved_type_name() {
        let err = resolve_src(
            r#"
struct List { x: i32 }
fn main() i32 { 0 }
"#,
        )
        .expect_err("reserved");
        assert!(err.contains("type name `List` is reserved"), "{err}");
    }

    #[test]
    fn resolve_rejects_user_redefinition_of_builtin_struct() {
        let err = resolve_src(
            r#"
struct Complex { x: i32 }
fn main() i32 { 0 }
"#,
        )
        .expect_err("reserved builtin");
        assert!(err.contains("type name `Complex` is reserved"), "{err}");
    }

    #[test]
    fn resolve_rejects_reserved_function_name() {
        let err = resolve_src(
            r#"
fn println() i32 { 0 }
"#,
        )
        .expect_err("reserved fn");
        assert!(
            err.contains("function name `println` is reserved"),
            "{err}"
        );
    }
}
