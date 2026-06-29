use std::collections::HashSet;

use crate::ast::{
    Ability, Block, EnumDef, EnumVariantDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt,
    StructDef, Ty, VectorDef, method_symbol,
};
use crate::lexer::Token;
use crate::nia_std::{
    ATOMIC_PTR_TYPE, LIST_NEW, LIST_TYPE, LIST_WITH_CAPACITY, OPTION_TYPE, RESULT_TYPE,
};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    module_path: Vec<String>,
    declared_modules: HashSet<String>,
}

impl Parser {
    /// Builds parser over a pre-tokenized stream.
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            pos: 0,
            module_path: Vec::new(),
            declared_modules: HashSet::new(),
        }
    }

    /// Returns current token without consuming it.
    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn peek_n(&self, n: usize) -> &Token {
        self.tokens.get(self.pos + n).unwrap_or(&Token::Eof)
    }

    /// Consumes and returns current token, or `Eof` when stream is exhausted.
    fn bump(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    /// Consumes one token and validates that it matches `want`.
    fn expect(&mut self, want: &Token) -> Result<(), String> {
        let got = self.bump();
        if &got == want {
            Ok(())
        } else {
            Err(format!("expected {want:?}, got {got:?}"))
        }
    }

    fn qualify_item_name(&self, name: &str) -> String {
        if self.module_path.is_empty() {
            name.to_string()
        } else {
            format!("{}::{name}", self.module_path.join("::"))
        }
    }

    fn parse_path_segments(&mut self) -> Result<Vec<String>, String> {
        let mut segments = vec![self.expect_ident()?];
        while matches!(self.peek(), Token::DoubleColon) {
            self.bump();
            segments.push(self.expect_ident()?);
        }
        Ok(segments)
    }

    fn resolve_item_path(&self, segments: &[String]) -> Result<String, String> {
        let Some(first) = segments.first() else {
            return Err("expected path".into());
        };
        let mut out: Vec<String>;
        let mut idx = 0usize;
        match first.as_str() {
            "crate" => {
                out = Vec::new();
                idx = 1;
            }
            "self" => {
                out = self.module_path.clone();
                idx = 1;
            }
            "super" => {
                out = self.module_path.clone();
                while idx < segments.len() && segments[idx] == "super" {
                    if out.pop().is_none() {
                        return Err("too many `super` segments in path".into());
                    }
                    idx += 1;
                }
            }
            _ => {
                out = self.module_path.clone();
            }
        }
        if idx >= segments.len() {
            return Err(format!("expected item after `{first}` in path"));
        }
        out.extend(segments[idx..].iter().cloned());
        Ok(out.join("::"))
    }

    fn resolve_type_path(&self, segments: &[String]) -> Result<String, String> {
        if segments.len() == 1 && crate::nia_std::is_reserved_type_name(&segments[0]) {
            Ok(segments[0].clone())
        } else {
            self.resolve_item_path(segments)
        }
    }

    fn resolve_call_path(&self, segments: &[String]) -> Result<String, String> {
        if segments.len() == 1 && crate::nia_std::is_reserved_fn_name(&segments[0]) {
            Ok(segments[0].clone())
        } else {
            self.resolve_item_path(segments)
        }
    }

    fn resolve_expr_path(&self, segments: &[String]) -> Result<String, String> {
        if segments.len() > 1 && crate::nia_std::is_builtin_enum_type_name(&segments[0]) {
            Ok(segments.join("::"))
        } else {
            self.resolve_item_path(segments)
        }
    }

    fn split_variant_path(path: String) -> Result<(String, String), String> {
        let Some((enum_name, variant)) = path.rsplit_once("::") else {
            return Err("match pattern must use `Enum::Variant` path syntax".into());
        };
        Ok((enum_name.to_string(), variant.to_string()))
    }

    /// Parses a complete source file into `(structs, enums, functions, vectors)`.
    ///
    /// ## Grammar contract
    /// Top-level accepts only:
    /// - `mod name { ... }`
    /// - `struct ...`
    /// - `impl ...`
    /// - `fn ...`
    /// - `quant fn ...`
    /// - `extern fn ...`
    ///
    /// Any other token at top level is a hard parse error. This strictness keeps
    /// recovery and diagnostics simple for a small language.
    pub fn parse_file(
        mut self,
    ) -> Result<(Vec<StructDef>, Vec<EnumDef>, Vec<FnDef>, Vec<VectorDef>), String> {
        let mut structs = Vec::new();
        let mut enums = Vec::new();
        let mut fns = Vec::new();
        let mut vectors = Vec::new();
        self.parse_items(false, &mut structs, &mut enums, &mut fns, &mut vectors)?;
        Ok((structs, enums, fns, vectors))
    }

    fn parse_items(
        &mut self,
        stop_on_rbrace: bool,
        structs: &mut Vec<StructDef>,
        enums: &mut Vec<EnumDef>,
        fns: &mut Vec<FnDef>,
        vectors: &mut Vec<VectorDef>,
    ) -> Result<(), String> {
        while !matches!(self.peek(), Token::Eof) {
            if stop_on_rbrace && matches!(self.peek(), Token::RBrace) {
                break;
            }
            if matches!(self.peek(), Token::Pub) {
                self.bump();
            }
            match self.peek().clone() {
                Token::Mod => {
                    self.parse_module(structs, enums, fns, vectors)?;
                }
                Token::Struct => {
                    structs.push(self.parse_struct()?);
                }
                Token::Vector => {
                    vectors.push(self.parse_vector()?);
                }
                Token::Enum => {
                    enums.push(self.parse_enum()?);
                }
                Token::Impl => {
                    fns.extend(self.parse_impl()?);
                }
                Token::Fn | Token::Extern => {
                    fns.push(self.parse_fn()?);
                }
                Token::Quant if matches!(self.peek_n(1), Token::Fn) => {
                    fns.push(self.parse_fn()?);
                }
                other => return Err(format!("unexpected token at top level: {other:?}")),
            }
        }
        if stop_on_rbrace {
            self.expect(&Token::RBrace)?;
        }
        Ok(())
    }

    fn parse_module(
        &mut self,
        structs: &mut Vec<StructDef>,
        enums: &mut Vec<EnumDef>,
        fns: &mut Vec<FnDef>,
        vectors: &mut Vec<VectorDef>,
    ) -> Result<(), String> {
        self.expect(&Token::Mod)?;
        let name = self.expect_ident()?;
        let full_name = self.qualify_item_name(&name);
        if !self.declared_modules.insert(full_name.clone()) {
            return Err(format!("duplicate module `{full_name}`"));
        }
        match self.peek() {
            Token::LBrace => {
                self.bump();
                self.module_path.push(name);
                let parsed = self.parse_items(true, structs, enums, fns, vectors);
                self.module_path.pop();
                parsed
            }
            Token::Semi => Err(format!(
                "external module `{full_name}` was not loaded; compile from a file path to use `mod {name};`"
            )),
            other => Err(format!(
                "expected `{{` or `;` after module name, got {other:?}"
            )),
        }
    }

    fn parse_impl(&mut self) -> Result<Vec<FnDef>, String> {
        self.expect(&Token::Impl)?;
        let owner = self.parse_ty()?;
        self.expect(&Token::LBrace)?;
        let mut methods = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            if matches!(self.peek(), Token::Pub) {
                self.bump();
            }
            if matches!(self.peek(), Token::Extern) {
                return Err(
                    "extern methods are not supported; export a top-level extern fn instead".into(),
                );
            }
            if !matches!(self.peek(), Token::Fn) {
                return Err(format!("expected method `fn`, got {:?}", self.peek()));
            }
            let mut method = self.parse_method(&owner)?;
            let Some((first_name, first_ty)) = method.params.first() else {
                return Err(format!(
                    "method `{}` must take `self` or `&self` as its first parameter",
                    method.name
                ));
            };
            if first_name != "self" {
                return Err(format!(
                    "method `{}` first parameter must be named `self`",
                    method.name
                ));
            }
            let valid_self_ty = first_ty == &owner
                || matches!(first_ty, Ty::Ptr(inner) if inner.as_ref() == &owner);
            if !valid_self_ty {
                return Err(format!(
                    "method `{}` self type mismatch: expected `self` or `&self` for {owner:?}, got {first_ty:?}",
                    method.name
                ));
            }
            method.name = method_symbol(&owner, &method.name);
            methods.push(method);
        }
        self.expect(&Token::RBrace)?;
        Ok(methods)
    }

    fn parse_method(&mut self, owner: &Ty) -> Result<FnDef, String> {
        self.expect(&Token::Fn)?;
        let name = self.expect_method_name()?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            let mut first = true;
            loop {
                params.push(self.parse_method_param(owner, first, &name)?);
                first = false;
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => break,
                    _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RParen)?;
        let ret = match self.peek() {
            Token::LBrace => None,
            _ => Some(self.parse_ty()?),
        };
        let body = self.parse_block()?;
        Ok(FnDef {
            name,
            is_extern: false,
            is_quantum: false,
            params,
            ret,
            body,
            closure_captures: Vec::new(),
        })
    }

    fn expect_method_name(&mut self) -> Result<String, String> {
        match self.bump() {
            Token::Ident(s) => Ok(s),
            Token::Clone => Ok("clone".into()),
            Token::Drop => Ok("drop".into()),
            Token::Deref => Ok("deref".into()),
            other => Err(format!("expected method name, got {other:?}")),
        }
    }

    fn parse_method_param(
        &mut self,
        owner: &Ty,
        first: bool,
        method_name: &str,
    ) -> Result<(String, Ty), String> {
        if first {
            if matches!(self.peek(), Token::Amp) {
                self.bump();
                let name = self.expect_ident()?;
                if name == "mut" {
                    return Err("`mut self` is not supported".into());
                }
                if name != "self" {
                    return Err(format!(
                        "method `{method_name}` first parameter must be `self` or `&self`"
                    ));
                }
                return Ok(("self".into(), Ty::Ptr(Box::new(owner.clone()))));
            }

            if let Token::Ident(name) = self.peek().clone() {
                if name == "mut" {
                    return Err("`mut self` is not supported".into());
                }
                if name == "self" {
                    self.bump();
                    if matches!(self.peek(), Token::Colon) {
                        self.bump();
                        return Ok(("self".into(), self.parse_ty()?));
                    }
                    return Ok(("self".into(), owner.clone()));
                }
            }
        }

        let pname = self.expect_ident()?;
        self.expect(&Token::Colon)?;
        let pty = self.parse_ty()?;
        Ok((pname, pty))
    }

    fn parse_ability(&mut self) -> Result<Ability, String> {
        match self.bump() {
            Token::Copy => Ok(Ability::Copy),
            Token::Clone => Ok(Ability::Clone),
            Token::Drop => Ok(Ability::Drop),
            Token::Deref => Ok(Ability::Deref),
            Token::Send => Ok(Ability::Send),
            Token::Sync => Ok(Ability::Sync),
            other => Err(format!("expected ability after `has`, got {other:?}")),
        }
    }

    fn ability_label(ability: Ability) -> &'static str {
        match ability {
            Ability::Copy => "copy",
            Ability::Clone => "clone",
            Ability::Drop => "drop",
            Ability::Deref => "deref",
            Ability::Send => "send",
            Ability::Sync => "sync",
        }
    }

    fn parse_abilities_opt(&mut self) -> Result<Vec<Ability>, String> {
        if !matches!(self.peek(), Token::Has) {
            return Ok(Vec::new());
        }
        self.bump();

        let mut abilities = Vec::new();
        let mut seen = HashSet::new();
        loop {
            let ability = self.parse_ability()?;
            if !seen.insert(ability) {
                return Err(format!(
                    "duplicate ability `{}`",
                    Self::ability_label(ability)
                ));
            }
            abilities.push(ability);
            if matches!(self.peek(), Token::Comma) {
                self.bump();
            } else {
                break;
            }
        }
        Ok(abilities)
    }

    fn parse_enum(&mut self) -> Result<EnumDef, String> {
        self.expect(&Token::Enum)?;
        let name = self.expect_ident()?;
        let name = self.qualify_item_name(&name);
        let abilities = self.parse_abilities_opt()?;
        self.expect(&Token::LBrace)?;
        let mut variants = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                let vname = self.expect_ident()?;
                let fields = if matches!(self.peek(), Token::LParen) {
                    self.bump();
                    let mut elems = Vec::new();
                    if !matches!(self.peek(), Token::RParen) {
                        loop {
                            elems.push(self.parse_ty()?);
                            match self.peek() {
                                Token::Comma => {
                                    self.bump();
                                    if matches!(self.peek(), Token::RParen) {
                                        break;
                                    }
                                }
                                Token::RParen => break,
                                _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                            }
                        }
                    }
                    self.expect(&Token::RParen)?;
                    EnumVariantFields::Tuple(elems)
                } else if matches!(self.peek(), Token::LBrace) {
                    self.bump();
                    let mut flds = Vec::new();
                    if !matches!(self.peek(), Token::RBrace) {
                        loop {
                            let fname = self.expect_ident()?;
                            self.expect(&Token::Colon)?;
                            let fty = self.parse_ty()?;
                            flds.push((fname, fty));
                            match self.peek() {
                                Token::Comma => {
                                    self.bump();
                                    if matches!(self.peek(), Token::RBrace) {
                                        break;
                                    }
                                }
                                Token::RBrace => break,
                                _ => {
                                    return Err(format!("expected , or }}, got {:?}", self.peek()));
                                }
                            }
                        }
                    }
                    self.expect(&Token::RBrace)?;
                    EnumVariantFields::Struct(flds)
                } else {
                    EnumVariantFields::Unit
                };
                variants.push(EnumVariantDef {
                    name: vname,
                    fields,
                });
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                        if matches!(self.peek(), Token::RBrace) {
                            break;
                        }
                    }
                    Token::RBrace => break,
                    _ => return Err(format!("expected , or }}, got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(EnumDef {
            name,
            abilities,
            variants,
        })
    }

    /// Parses struct declaration in one of two forms:
    /// - named-field: `struct S { a: T, b: U }`
    /// - tuple: `struct S (T, U, ...)`
    ///
    /// Tuple fields are stored with synthetic names `"0"`, `"1"`, ... so field
    /// access can reuse common field machinery.
    fn parse_struct(&mut self) -> Result<StructDef, String> {
        self.expect(&Token::Struct)?;
        let name = self.expect_ident()?;
        let name = self.qualify_item_name(&name);
        let abilities = self.parse_abilities_opt()?;
        if matches!(self.peek(), Token::LBrace) {
            self.bump();
            let mut fields = Vec::new();
            while !matches!(self.peek(), Token::RBrace) {
                let fname = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let ty = self.parse_ty()?;
                fields.push((fname, ty));
                if matches!(self.peek(), Token::Comma) {
                    self.bump();
                }
            }
            self.expect(&Token::RBrace)?;
            Ok(StructDef {
                name,
                abilities,
                is_tuple: false,
                fields,
            })
        } else if matches!(self.peek(), Token::LParen) {
            self.bump();
            let mut fields = Vec::new();
            let mut idx: usize = 0;
            if !matches!(self.peek(), Token::RParen) {
                loop {
                    let ty = self.parse_ty()?;
                    fields.push((idx.to_string(), ty));
                    idx += 1;
                    match self.peek() {
                        Token::Comma => {
                            self.bump();
                            if matches!(self.peek(), Token::RParen) {
                                break;
                            }
                        }
                        Token::RParen => break,
                        _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                    }
                }
            }
            self.expect(&Token::RParen)?;
            Ok(StructDef {
                name,
                abilities,
                is_tuple: true,
                fields,
            })
        } else {
            Err(format!(
                "expected `{{` or `(` after struct name, got {:?}",
                self.peek()
            ))
        }
    }

    fn parse_vector(&mut self) -> Result<VectorDef, String> {
        self.expect(&Token::Vector)?;
        let name = self.expect_ident()?;
        let name = self.qualify_item_name(&name);
        let ty = self.parse_ty()?;

        let bracket = if matches!(self.peek(), Token::LBracket) {
            self.bump();
            true
        } else if matches!(self.peek(), Token::LBrace) {
            self.bump();
            false
        } else {
            return Err(format!(
                "expected `[` or `{{` after vector type, got {:?}",
                self.peek()
            ));
        };

        let mut fields = Vec::new();
        while if bracket {
            !matches!(self.peek(), Token::RBracket)
        } else {
            !matches!(self.peek(), Token::RBrace)
        } {
            let fname = self.expect_ident()?;
            fields.push(fname);
            if matches!(self.peek(), Token::Comma) {
                self.bump();
            }
        }
        if bracket {
            self.expect(&Token::RBracket)?;
        } else {
            self.expect(&Token::RBrace)?;
        }
        let abilities = self.parse_abilities_opt()?;
        Ok(VectorDef {
            name,
            abilities,
            ty,
            fields,
        })
    }

    /// Parses function declaration: name, params, optional return type, and body.
    ///
    /// Return type is omitted for `void` functions:
    /// - `fn foo() { ... }` => no explicit return type
    /// - `fn foo() i32 { ... }` => typed return
    /// - `extern fn foo() i32 { ... }` => parsed C ABI export marker
    /// - `quant fn foo() { ... }` => quantum-only function
    fn parse_fn(&mut self) -> Result<FnDef, String> {
        let is_quantum = if matches!(self.peek(), Token::Quant) {
            self.bump();
            true
        } else {
            false
        };
        let is_extern = if matches!(self.peek(), Token::Extern) {
            if is_quantum {
                return Err("`quant extern fn` is not supported".into());
            }
            self.bump();
            true
        } else {
            false
        };
        self.expect(&Token::Fn)?;
        let name = self.expect_ident()?;
        let name = self.qualify_item_name(&name);
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                let pname = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let pty = self.parse_ty()?;
                params.push((pname, pty));
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => break,
                    _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RParen)?;
        let ret = match self.peek() {
            Token::LBrace => None,
            _ => Some(self.parse_ty()?),
        };
        let body = self.parse_block()?;
        Ok(FnDef {
            name,
            is_extern,
            is_quantum,
            params,
            ret,
            body,
            closure_captures: Vec::new(),
        })
    }

    /// Parses `{ ... }` block with statements and optional tail expression.
    ///
    /// ## Tail rule
    /// Final expression without `;` becomes `Block.tail`.
    /// This is used as function return value when function has explicit return type.
    ///
    /// Expressions ending with `;` are lowered to `Stmt::Expr`.
    fn parse_block(&mut self) -> Result<Block, String> {
        self.expect(&Token::LBrace)?;
        let mut stmts = Vec::new();
        loop {
            match self.peek().clone() {
                Token::RBrace => {
                    self.bump();
                    return Ok(Block { stmts, tail: None });
                }
                Token::Let => {
                    stmts.push(self.parse_let_stmt()?);
                }
                Token::If => {
                    stmts.push(self.parse_if_stmt()?);
                }
                Token::While => {
                    stmts.push(self.parse_while_stmt()?);
                }
                Token::Loop => {
                    stmts.push(self.parse_loop_stmt()?);
                }
                Token::Break => {
                    stmts.push(self.parse_break_stmt()?);
                }
                Token::For => {
                    stmts.push(self.parse_for_stmt()?);
                }
                Token::Quant => {
                    stmts.push(self.parse_quant_stmt()?);
                }
                Token::Gpu => {
                    stmts.push(self.parse_gpu_stmt()?);
                }
                Token::Return => {
                    stmts.push(self.parse_return_stmt()?);
                }
                _ => {
                    let e = self.parse_expr()?;
                    if matches!(self.peek(), Token::Eq) {
                        self.bump();
                        let value = self.parse_expr()?;
                        self.expect(&Token::Semi)?;
                        stmts.push(Stmt::Assign { target: e, value });
                        continue;
                    }
                    if matches!(
                        self.peek(),
                        Token::PlusEq
                            | Token::MinusEq
                            | Token::StarEq
                            | Token::SlashEq
                            | Token::PercentEq
                            | Token::AmpEq
                            | Token::PipeEq
                            | Token::CaretEq
                            | Token::ShlEq
                            | Token::ShrEq
                    ) {
                        let op_tok = self.bump();
                        let rhs = self.parse_expr()?;
                        self.expect(&Token::Semi)?;
                        let value = match op_tok {
                            Token::PlusEq => Expr::Add(Box::new(e.clone()), Box::new(rhs)),
                            Token::MinusEq => Expr::Sub(Box::new(e.clone()), Box::new(rhs)),
                            Token::StarEq => Expr::Mul(Box::new(e.clone()), Box::new(rhs)),
                            Token::SlashEq => Expr::Div(Box::new(e.clone()), Box::new(rhs)),
                            Token::PercentEq => Expr::Rem(Box::new(e.clone()), Box::new(rhs)),
                            Token::AmpEq => Expr::BitAnd(Box::new(e.clone()), Box::new(rhs)),
                            Token::PipeEq => Expr::BitOr(Box::new(e.clone()), Box::new(rhs)),
                            Token::CaretEq => Expr::BitXor(Box::new(e.clone()), Box::new(rhs)),
                            Token::ShlEq => Expr::Shl(Box::new(e.clone()), Box::new(rhs)),
                            Token::ShrEq => Expr::Shr(Box::new(e.clone()), Box::new(rhs)),
                            _ => unreachable!(),
                        };
                        stmts.push(Stmt::Assign { target: e, value });
                        continue;
                    }
                    if matches!(self.peek(), Token::Semi) {
                        self.bump();
                        stmts.push(Stmt::Expr(e));
                        continue;
                    }
                    self.expect(&Token::RBrace)?;
                    return Ok(Block {
                        stmts,
                        tail: Some(e),
                    });
                }
            }
        }
    }

    /// Parses `let` statement with optional explicit type annotation.
    fn parse_let_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Let)?;
        let name = self.expect_ident()?;
        let (ty, init) = if matches!(self.peek(), Token::Colon) {
            self.bump();
            let t = self.parse_ty()?;
            if matches!(self.peek(), Token::Semi) {
                (Some(t), None)
            } else {
                self.expect(&Token::Eq)?;
                let init = self.parse_expr()?;
                (Some(t), Some(init))
            }
        } else {
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            (None, Some(init))
        };
        self.expect(&Token::Semi)?;
        Ok(Stmt::Let { name, ty, init })
    }

    /// Parses `if <cond> { ... }` statement.
    fn parse_if_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::If)?;
        let cond = self.parse_if_cond()?;
        let then_block = self.parse_block()?;
        Ok(Stmt::If { cond, then_block })
    }

    /// Parses `while <cond> { ... }` (same narrow condition grammar as `if`).
    fn parse_while_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::While)?;
        let cond = self.parse_if_cond()?;
        let body = self.parse_block()?;
        Ok(Stmt::While { cond, body })
    }

    /// Parses `loop { ... }`.
    fn parse_loop_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Loop)?;
        let body = self.parse_block()?;
        Ok(Stmt::Loop { body })
    }

    /// Parses `break;`
    fn parse_break_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Break)?;
        self.expect(&Token::Semi)?;
        Ok(Stmt::Break)
    }

    /// Parses `for <ident> in <expr>..<expr> { ... }` (half-open range).
    fn parse_for_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::For)?;
        let var = self.expect_ident()?;
        self.expect(&Token::In)?;
        let start = self.parse_expr()?;
        self.expect(&Token::DotDot)?;
        let end = self.parse_expr()?;
        let body = self.parse_block()?;
        Ok(Stmt::For {
            var,
            start,
            end,
            body,
        })
    }

    /// Parses `quant { ... }`.
    fn parse_quant_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Quant)?;
        let body = self.parse_block()?;
        Ok(Stmt::Quant { body })
    }

    /// Parses `gpu { ... }`.
    fn parse_gpu_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Gpu)?;
        let body = self.parse_block()?;
        Ok(Stmt::Gpu { body })
    }

    /// Parses condition expression for `if` / `while`.
    ///
    /// Uses full expression grammar (including comparisons), while preserving
    /// `if foo { ... }` by avoiding accidental struct-literal parse for `foo`.
    fn parse_if_cond(&mut self) -> Result<Expr, String> {
        self.parse_expr()
    }

    /// Parses `return <expr>` statement with optional trailing semicolon.
    fn parse_return_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Return)?;
        let e = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.bump();
        }
        Ok(Stmt::Return(e))
    }

    /// Parses type grammar.
    ///
    /// Supported forms:
    /// - primitives (`i32`, `bool`, ...)
    /// - struct names (`MyStruct`)
    /// - pointers (`&T`)
    /// - fixed arrays (`[T; N]`)
    /// - anonymous vector types (`T<N>`)
    /// - heap anonymous vector types (`T<>`)
    ///
    /// Pointer and array forms are recursive, so nested types like `&[i32; 4]`
    /// parse naturally.
    ///
    /// Postfix `T[]` desugars to `Ty::Matrix(T, None)` — a heap-backed matrix
    /// with element type `T` and unknown shape (same as the bare `Matrix` annotation).
    fn parse_ty(&mut self) -> Result<Ty, String> {
        let mut ty = if matches!(self.peek(), Token::Amp) {
            self.bump();
            let inner = self.parse_ty()?;
            Ty::Ptr(Box::new(inner))
        } else if matches!(self.peek(), Token::Fn) {
            self.parse_fn_ty()?
        } else if matches!(self.peek(), Token::LParen) && matches!(self.peek_n(1), Token::RParen) {
            self.bump();
            self.bump();
            Ty::Unit
        } else if matches!(self.peek(), Token::LBracket) {
            self.bump();
            let elem = self.parse_ty()?;
            self.expect(&Token::Semi)?;
            let len = match self.bump() {
                Token::Int(n) if n >= 0 => n as usize,
                other => return Err(format!("expected non-negative array length, got {other:?}")),
            };
            self.expect(&Token::RBracket)?;
            Ty::Array(Box::new(elem), len)
        } else {
            match self.peek() {
                Token::Ident(_) => {
                    let segments = self.parse_path_segments()?;
                    let n = self.resolve_type_path(&segments)?;
                    if n == LIST_TYPE {
                        self.expect(&Token::LBracket)?;
                        let elem = self.parse_ty()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Ty::List(Box::new(elem)))
                    } else if n == OPTION_TYPE {
                        self.expect(&Token::LBracket)?;
                        let elem = self.parse_ty()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Ty::Option(Box::new(elem)))
                    } else if n == RESULT_TYPE {
                        self.expect(&Token::LBracket)?;
                        let ok = self.parse_ty()?;
                        self.expect(&Token::Comma)?;
                        let err = self.parse_ty()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Ty::ResultType(Box::new(ok), Box::new(err)))
                    } else if n == ATOMIC_PTR_TYPE {
                        self.expect(&Token::LBracket)?;
                        let elem = self.parse_ty()?;
                        self.expect(&Token::RBracket)?;
                        Ok(Ty::AtomicPtr(Box::new(elem)))
                    } else {
                        Ok(Ty::Struct(n))
                    }
                }
                _ => match self.bump() {
                    Token::TyI8 => Ok(Ty::I8),
                    Token::TyU8 => Ok(Ty::U8),
                    Token::TyI16 => Ok(Ty::I16),
                    Token::TyU16 => Ok(Ty::U16),
                    Token::TyI32 => Ok(Ty::I32),
                    Token::TyU32 => Ok(Ty::U32),
                    Token::TyI64 => Ok(Ty::I64),
                    Token::TyU64 => Ok(Ty::U64),
                    Token::TyI128 => Ok(Ty::I128),
                    Token::TyIsize => Ok(Ty::Isize),
                    Token::TyUsize => Ok(Ty::Usize),
                    Token::TyU128 => Ok(Ty::U128),
                    Token::TyBool => Ok(Ty::Bool),
                    Token::TyF16 => Ok(Ty::F16),
                    Token::TyF32 => Ok(Ty::F32),
                    Token::TyF64 => Ok(Ty::F64),
                    Token::TyString => Ok(Ty::String),
                    other => Err(format!("expected type, got {other:?}")),
                },
            }?
        };

        loop {
            if matches!(self.peek(), Token::Lt) {
                self.bump();
                if matches!(self.peek(), Token::Gt) {
                    self.bump();
                    ty = Ty::HeapVector(Box::new(ty));
                    continue;
                }
                let len = match self.bump() {
                    Token::Int(n) if n > 0 => n as usize,
                    other => {
                        return Err(format!(
                            "expected positive anonymous vector length, got {other:?}"
                        ));
                    }
                };
                self.expect(&Token::Gt)?;
                ty = Ty::AnonVector(Box::new(ty), len);
            } else if matches!(self.peek(), Token::LBracket)
                && matches!(self.peek_n(1), Token::RBracket)
            {
                self.bump();
                self.bump();
                ty = Ty::Matrix(Box::new(ty), None);
            } else {
                break;
            }
        }

        Ok(ty)
    }

    fn parse_fn_ty(&mut self) -> Result<Ty, String> {
        self.expect(&Token::Fn)?;
        self.expect(&Token::LParen)?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                params.push(self.parse_ty()?);
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => break,
                    _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RParen)?;
        self.expect(&Token::ThinArrow)?;
        let ret = self.parse_ty()?;
        Ok(Ty::Fn(params, Box::new(ret)))
    }

    /// Expression entrypoint. Bitwise operators follow conventional precedence:
    /// comparisons bind tighter than `&`, then `^`, then `|`.
    fn parse_expr(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Token::Pipe) {
            return self.parse_closure_expr(false);
        }
        if matches!(self.peek(), Token::Move) {
            self.bump();
            return self.parse_closure_expr(true);
        }
        self.parse_bit_or()
    }

    fn parse_closure_expr(&mut self, is_move: bool) -> Result<Expr, String> {
        self.expect(&Token::Pipe)?;
        let mut params = Vec::new();
        if !matches!(self.peek(), Token::Pipe) {
            loop {
                let name = self.expect_ident()?;
                let ty = if matches!(self.peek(), Token::Colon) {
                    self.bump();
                    Some(self.parse_ty()?)
                } else {
                    None
                };
                params.push((name, ty));
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::Pipe => break,
                    _ => return Err(format!("expected , or |, got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::Pipe)?;
        let ret = if matches!(self.peek(), Token::ThinArrow) {
            self.bump();
            Some(self.parse_ty()?)
        } else {
            None
        };
        let body = if matches!(self.peek(), Token::LBrace) {
            self.parse_block()?
        } else {
            let tail = self.parse_expr()?;
            Block {
                stmts: Vec::new(),
                tail: Some(tail),
            }
        };
        Ok(Expr::Closure {
            is_move,
            params,
            ret,
            body: Box::new(body),
        })
    }

    fn parse_bit_or(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bit_xor()?;
        while matches!(self.peek(), Token::Pipe) {
            self.bump();
            let right = self.parse_bit_xor()?;
            left = Expr::BitOr(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bit_xor(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_bit_and()?;
        while matches!(self.peek(), Token::Caret) {
            self.bump();
            let right = self.parse_bit_and()?;
            left = Expr::BitXor(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bit_and(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_equality()?;
        while matches!(self.peek(), Token::Amp) {
            self.bump();
            let right = self.parse_equality()?;
            left = Expr::BitAnd(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_comparison()?;
        loop {
            match self.peek().clone() {
                Token::EqEq => {
                    self.bump();
                    let right = self.parse_comparison()?;
                    left = Expr::Eq(Box::new(left), Box::new(right));
                }
                Token::NotEq => {
                    self.bump();
                    let right = self.parse_comparison()?;
                    left = Expr::Ne(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_comparison(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_shift()?;
        loop {
            match self.peek().clone() {
                Token::Lt => {
                    self.bump();
                    let right = self.parse_shift()?;
                    left = Expr::Lt(Box::new(left), Box::new(right));
                }
                Token::Le => {
                    self.bump();
                    let right = self.parse_shift()?;
                    left = Expr::Le(Box::new(left), Box::new(right));
                }
                Token::Gt => {
                    self.bump();
                    let right = self.parse_shift()?;
                    left = Expr::Gt(Box::new(left), Box::new(right));
                }
                Token::Ge => {
                    self.bump();
                    let right = self.parse_shift()?;
                    left = Expr::Ge(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// Parses left-associative integer shifts below additive precedence.
    fn parse_shift(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_additive()?;
        loop {
            match self.peek().clone() {
                Token::Shl => {
                    self.bump();
                    let right = self.parse_additive()?;
                    left = Expr::Shl(Box::new(left), Box::new(right));
                }
                Token::Shr => {
                    self.bump();
                    let right = self.parse_additive()?;
                    left = Expr::Shr(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// Parses left-associative `+` / `-` chains.
    fn parse_additive(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_multiplicative()?;
        loop {
            match self.peek().clone() {
                Token::Plus => {
                    self.bump();
                    let right = self.parse_multiplicative()?;
                    left = Expr::Add(Box::new(left), Box::new(right));
                }
                Token::Minus => {
                    self.bump();
                    let right = self.parse_multiplicative()?;
                    left = Expr::Sub(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    /// Parses left-associative `*` / `/` / `%` chains.
    fn parse_multiplicative(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_unary()?;
        loop {
            match self.peek().clone() {
                Token::Star => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::Mul(Box::new(left), Box::new(right));
                }
                Token::At => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::VecDot(Box::new(left), Box::new(right));
                }
                Token::Slash => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::Div(Box::new(left), Box::new(right));
                }
                Token::Percent => {
                    self.bump();
                    let right = self.parse_unary()?;
                    left = Expr::Rem(Box::new(left), Box::new(right));
                }
                _ => break,
            }
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> Result<Expr, String> {
        if matches!(self.peek(), Token::Minus) {
            self.bump();
            let inner = self.parse_unary()?;
            return Ok(Expr::Neg(Box::new(inner)));
        }
        if matches!(self.peek(), Token::Bang) {
            self.bump();
            let inner = self.parse_unary()?;
            return Ok(Expr::Not(Box::new(inner)));
        }
        if matches!(self.peek(), Token::Tilde) {
            self.bump();
            let inner = self.parse_unary()?;
            return Ok(Expr::BitNot(Box::new(inner)));
        }
        self.parse_suffix_chain()
    }

    /// Parses postfix expression chain with left-to-right folding.
    ///
    /// Handles:
    /// - calls: `foo(a, b)`
    /// - field access: `x.y` / tuple index field `x.0`
    /// - indexing: `arr[i]`
    ///
    /// This function is where "primary expression + suffixes" gets normalized.
    fn parse_suffix_chain(&mut self) -> Result<Expr, String> {
        let mut e = self.parse_atom()?;
        loop {
            match self.peek().clone() {
                Token::LParen => {
                    let args = self.parse_call_args()?;
                    e = Expr::CallExpr {
                        callee: Box::new(e),
                        args,
                    };
                }
                Token::Dot => {
                    self.bump();
                    let field = match self.bump() {
                        Token::Ident(s) => s,
                        Token::Clone => "clone".into(),
                        Token::Drop => "drop".into(),
                        Token::Deref => "deref".into(),
                        Token::Int(n) => n.to_string(),
                        other => {
                            return Err(format!("expected field name or index, got {other:?}"));
                        }
                    };
                    if matches!(self.peek(), Token::LParen) {
                        let args = self.parse_call_args()?;
                        e = Expr::MethodCall {
                            receiver: Box::new(e),
                            name: field,
                            args,
                        };
                    } else {
                        e = Expr::Field(Box::new(e), field);
                    }
                }
                Token::LBracket => {
                    if let Expr::Ident(name) = &e {
                        if name == LIST_NEW || name == LIST_WITH_CAPACITY {
                            let name = name.clone();
                            let ty_args = self.parse_generic_ty_args()?;
                            let args = self.parse_call_args()?;
                            e = Expr::GenericCall {
                                name,
                                ty_args,
                                args,
                            };
                            continue;
                        }
                    }
                    self.bump();
                    let idx = self.parse_expr()?;
                    self.expect(&Token::RBracket)?;
                    e = Expr::Index(Box::new(e), Box::new(idx));
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>, String> {
        self.expect(&Token::LParen)?;
        let mut args = Vec::new();
        if !matches!(self.peek(), Token::RParen) {
            loop {
                args.push(self.parse_expr()?);
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RParen => break,
                    _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RParen)?;
        Ok(args)
    }

    fn parse_generic_ty_args(&mut self) -> Result<Vec<Ty>, String> {
        self.expect(&Token::LBracket)?;
        let mut ty_args = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                ty_args.push(self.parse_ty()?);
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RBracket => break,
                    _ => return Err(format!("expected , or ], got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(ty_args)
    }

    fn looks_like_struct_lit_after_ident(&self) -> bool {
        if !matches!(self.peek(), Token::LBrace) {
            return false;
        }
        matches!(self.peek_n(1), Token::RBrace)
            || matches!(self.peek_n(1), Token::Ident(_)) && matches!(self.peek_n(2), Token::Colon)
    }

    fn looks_like_vector_lit_after_ident(&self) -> bool {
        if !matches!(self.peek(), Token::LBracket) {
            return false;
        }
        matches!(self.peek_n(1), Token::RBracket)
            || matches!(self.peek_n(1), Token::Ident(_)) && matches!(self.peek_n(2), Token::Colon)
    }

    /// Parses expression atoms (base terms before postfix chaining).
    ///
    /// Includes literals, identifiers, parenthesized expressions, unary pointer ops,
    /// and array literals. Struct literals start here too after identifier lookahead.
    fn parse_atom(&mut self) -> Result<Expr, String> {
        match self.peek() {
            Token::Quant => {
                self.bump();
                let body = self.parse_block()?;
                Ok(Expr::Quant {
                    body: Box::new(body),
                })
            }
            Token::Gpu => {
                self.bump();
                let body = self.parse_block()?;
                Ok(Expr::Gpu {
                    body: Box::new(body),
                })
            }
            Token::Spawn => self.parse_spawn_expr(),
            Token::Match => self.parse_match_expr(),
            Token::Drop => {
                self.bump();
                if !matches!(self.peek(), Token::LParen) {
                    return Err("`drop` must be called as `drop(value)`".into());
                }
                let args = self.parse_call_args()?;
                Ok(Expr::Call {
                    name: "drop".into(),
                    args,
                })
            }
            Token::Amp => {
                self.bump();
                let inner = self.parse_atom()?;
                Ok(Expr::AddrOf(Box::new(inner)))
            }
            Token::Star => {
                self.bump();
                let inner = self.parse_atom()?;
                Ok(Expr::Deref(Box::new(inner)))
            }
            Token::LBracket => {
                self.bump();
                self.parse_array_lit_tail()
            }
            Token::Lt => {
                self.bump();
                self.parse_anon_vector_lit_tail()
            }
            _ => match self.bump() {
                Token::Int(n) => Ok(Expr::Int(n)),
                Token::Float(x) => Ok(Expr::Float(x)),
                Token::Bool(b) => Ok(Expr::Bool(b)),
                Token::StrLit(s) => Ok(Expr::String(s)),
                Token::Ident(first) => {
                    let mut segments = vec![first];
                    while matches!(self.peek(), Token::DoubleColon) {
                        self.bump();
                        segments.push(self.expect_ident()?);
                    }

                    if self.looks_like_struct_lit_after_ident() {
                        let name = self.resolve_item_path(&segments)?;
                        return self.parse_struct_lit_tail(name);
                    }
                    if self.looks_like_vector_lit_after_ident() {
                        let name = self.resolve_item_path(&segments)?;
                        return self.parse_vector_lit_tail(name);
                    }
                    if matches!(self.peek(), Token::LParen) {
                        let name = self.resolve_call_path(&segments)?;
                        let args = self.parse_call_args()?;
                        return Ok(Expr::Call { name, args });
                    }
                    if segments.len() == 1
                        && (segments[0] == LIST_NEW || segments[0] == LIST_WITH_CAPACITY)
                        && matches!(self.peek(), Token::LBracket)
                    {
                        let name = segments[0].clone();
                        let ty_args = self.parse_generic_ty_args()?;
                        let args = self.parse_call_args()?;
                        return Ok(Expr::GenericCall {
                            name,
                            ty_args,
                            args,
                        });
                    }

                    if segments.len() == 1 {
                        Ok(Expr::Ident(segments.remove(0)))
                    } else {
                        Ok(Expr::Ident(self.resolve_expr_path(&segments)?))
                    }
                }
                Token::LParen => {
                    let inner = self.parse_expr()?;
                    self.expect(&Token::RParen)?;
                    Ok(inner)
                }
                other => Err(format!("unexpected token in expression: {other:?}")),
            },
        }
    }

    fn parse_spawn_expr(&mut self) -> Result<Expr, String> {
        self.expect(&Token::Spawn)?;
        let segments = self.parse_path_segments()?;
        let target = self.resolve_item_path(&segments)?;
        Ok(Expr::Spawn { target })
    }

    /// Parses named struct literal body after consuming struct identifier.
    fn parse_struct_lit_tail(&mut self, struct_name: String) -> Result<Expr, String> {
        self.expect(&Token::LBrace)?;
        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RBrace) {
            loop {
                let fname = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fe = self.parse_expr()?;
                fields.push((fname, fe));
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RBrace => break,
                    _ => return Err(format!("expected , or }}, got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::StructLit {
            name: struct_name,
            fields,
        })
    }

    fn parse_vector_lit_tail(&mut self, vector_name: String) -> Result<Expr, String> {
        self.expect(&Token::LBracket)?;
        let mut fields = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                let fname = self.expect_ident()?;
                self.expect(&Token::Colon)?;
                let fe = self.parse_expr()?;
                fields.push((fname, fe));
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                    }
                    Token::RBracket => break,
                    _ => return Err(format!("expected , or ], got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RBracket)?;

        Ok(Expr::VectorLit {
            name: vector_name,
            fields,
        })
    }

    fn parse_anon_vector_lit_tail(&mut self) -> Result<Expr, String> {
        let mut elems = Vec::new();
        if matches!(self.peek(), Token::Gt) {
            return Err("anonymous vector literal must not be empty".into());
        }
        loop {
            elems.push(self.parse_additive()?);
            match self.peek() {
                Token::Comma => {
                    self.bump();
                    if matches!(self.peek(), Token::Gt) {
                        break;
                    }
                }
                Token::Gt => break,
                _ => return Err(format!("expected , or >, got {:?}", self.peek())),
            }
        }
        self.expect(&Token::Gt)?;
        Ok(Expr::AnonVectorLit(elems))
    }

    /// Parses array literal body after consuming opening bracket.
    fn parse_array_lit_tail(&mut self) -> Result<Expr, String> {
        let mut elems = Vec::new();
        if !matches!(self.peek(), Token::RBracket) {
            loop {
                elems.push(self.parse_expr()?);
                match self.peek() {
                    Token::Comma => {
                        self.bump();
                        if matches!(self.peek(), Token::RBracket) {
                            break;
                        }
                    }
                    Token::RBracket => break,
                    _ => return Err(format!("expected , or ], got {:?}", self.peek())),
                }
            }
        }
        self.expect(&Token::RBracket)?;
        Ok(Expr::ArrayLit(elems))
    }

    /// Consumes and returns identifier token.
    fn expect_ident(&mut self) -> Result<String, String> {
        match self.bump() {
            Token::Ident(s) => Ok(s),
            other => Err(format!("expected identifier, got {other:?}")),
        }
    }

    fn parse_match_expr(&mut self) -> Result<Expr, String> {
        self.expect(&Token::Match)?;
        let scrutinee =
            if matches!(self.peek(), Token::Ident(_)) && matches!(self.peek_n(1), Token::LBrace) {
                Expr::Ident(self.expect_ident()?)
            } else {
                self.parse_expr()?
            };
        self.expect(&Token::LBrace)?;
        let mut arms = Vec::new();
        while !matches!(self.peek(), Token::RBrace) {
            let segments = self.parse_path_segments()?;
            let path = self.resolve_item_path(&segments)?;
            let (enum_name, variant) = Self::split_variant_path(path)?;
            let pat = if matches!(self.peek(), Token::LParen) {
                self.bump();
                let mut bindings = Vec::new();
                if !matches!(self.peek(), Token::RParen) {
                    loop {
                        bindings.push(self.expect_ident()?);
                        match self.peek() {
                            Token::Comma => {
                                self.bump();
                                if matches!(self.peek(), Token::RParen) {
                                    break;
                                }
                            }
                            Token::RParen => break,
                            _ => return Err(format!("expected , or ), got {:?}", self.peek())),
                        }
                    }
                }
                self.expect(&Token::RParen)?;
                MatchPattern::Tuple {
                    enum_name,
                    variant,
                    bindings,
                }
            } else if matches!(self.peek(), Token::LBrace) {
                self.bump();
                let mut bindings = Vec::new();
                if !matches!(self.peek(), Token::RBrace) {
                    loop {
                        bindings.push(self.expect_ident()?);
                        match self.peek() {
                            Token::Comma => {
                                self.bump();
                                if matches!(self.peek(), Token::RBrace) {
                                    break;
                                }
                            }
                            Token::RBrace => break,
                            _ => return Err(format!("expected , or }}, got {:?}", self.peek())),
                        }
                    }
                }
                self.expect(&Token::RBrace)?;
                MatchPattern::Struct {
                    enum_name,
                    variant,
                    bindings,
                }
            } else {
                MatchPattern::Unit { enum_name, variant }
            };
            self.expect(&Token::FatArrow)?;
            let arm_expr = self.parse_expr()?;
            arms.push((pat, arm_expr));
            if matches!(self.peek(), Token::Comma) {
                self.bump();
            }
        }
        self.expect(&Token::RBrace)?;
        Ok(Expr::Match {
            scrutinee: Box::new(scrutinee),
            arms,
        })
    }
}
pub fn tokenize(input: &str) -> Vec<Token> {
    // Turn lexer stream into parser-friendly vector and drop explicit EOF token.
    let mut l = crate::lexer::Lexer::new(input);
    let mut v = Vec::new();
    loop {
        let t = l.next_token();
        if matches!(t, Token::Eof) {
            break;
        }
        v.push(t);
    }
    v
}

#[cfg(test)]
mod tests;
