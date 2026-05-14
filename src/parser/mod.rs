use crate::ast::{
    Block, EnumDef, EnumVariantDef, EnumVariantFields, Expr, FnDef, MatchPattern, Stmt, StructDef,
    Ty, VectorDef,
};
use crate::lexer::Token;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    /// Builds parser over a pre-tokenized stream.
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
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

    /// Parses a complete source file into `(structs, enums, functions, vectors)`.
    ///
    /// ## Grammar contract
    /// Top-level accepts only:
    /// - `struct ...`
    /// - `fn ...`
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
        while !matches!(self.peek(), Token::Eof) {
            match self.peek().clone() {
                Token::Struct => {
                    structs.push(self.parse_struct()?);
                }
                Token::Vector => {
                    vectors.push(self.parse_vector()?);
                }
                Token::Enum => {
                    enums.push(self.parse_enum()?);
                }
                Token::Fn => {
                    fns.push(self.parse_fn()?);
                }
                other => return Err(format!("unexpected token at top level: {other:?}")),
            }
        }
        Ok((structs, enums, fns, vectors))
    }

    fn parse_enum(&mut self) -> Result<EnumDef, String> {
        self.expect(&Token::Enum)?;
        let name = self.expect_ident()?;
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
        Ok(EnumDef { name, variants })
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
        Ok(VectorDef { name, ty, fields })
    }

    /// Parses function declaration: name, params, optional return type, and body.
    ///
    /// Return type is omitted for `void` functions:
    /// - `fn foo() { ... }` => no explicit return type
    /// - `fn foo() i32 { ... }` => typed return
    fn parse_fn(&mut self) -> Result<FnDef, String> {
        self.expect(&Token::Fn)?;
        let name = self.expect_ident()?;
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
            params,
            ret,
            body,
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
                        Token::PlusEq | Token::MinusEq | Token::StarEq | Token::SlashEq
                    ) {
                        let op_tok = self.bump();
                        let rhs = self.parse_expr()?;
                        self.expect(&Token::Semi)?;
                        let value = match op_tok {
                            Token::PlusEq => Expr::Add(Box::new(e.clone()), Box::new(rhs)),
                            Token::MinusEq => Expr::Sub(Box::new(e.clone()), Box::new(rhs)),
                            Token::StarEq => Expr::Mul(Box::new(e.clone()), Box::new(rhs)),
                            Token::SlashEq => Expr::Div(Box::new(e.clone()), Box::new(rhs)),
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
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            (Some(t), init)
        } else {
            self.expect(&Token::Eq)?;
            let init = self.parse_expr()?;
            (None, init)
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
    ///
    /// Pointer and array forms are recursive, so nested types like `&[i32; 4]`
    /// parse naturally.
    fn parse_ty(&mut self) -> Result<Ty, String> {
        if matches!(self.peek(), Token::Amp) {
            self.bump();
            let inner = self.parse_ty()?;
            return Ok(Ty::Ptr(Box::new(inner)));
        }
        if matches!(self.peek(), Token::LBracket) {
            self.bump();
            let elem = self.parse_ty()?;
            self.expect(&Token::Semi)?;
            let len = match self.bump() {
                Token::Int(n) if n >= 0 => n as usize,
                other => return Err(format!("expected non-negative array length, got {other:?}")),
            };
            self.expect(&Token::RBracket)?;
            return Ok(Ty::Array(Box::new(elem), len));
        }
        match self.bump() {
            Token::TyI8 => Ok(Ty::I8),
            Token::TyU8 => Ok(Ty::U8),
            Token::TyI16 => Ok(Ty::I16),
            Token::TyU16 => Ok(Ty::U16),
            Token::TyI32 => Ok(Ty::I32),
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
            Token::Ident(n) => Ok(Ty::Struct(n)),
            other => Err(format!("expected type, got {other:?}")),
        }
    }

    /// Expression entrypoint with comparison precedence above arithmetic.
    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_equality()
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
        let mut left = self.parse_additive()?;
        loop {
            match self.peek().clone() {
                Token::Lt => {
                    self.bump();
                    let right = self.parse_additive()?;
                    left = Expr::Lt(Box::new(left), Box::new(right));
                }
                Token::Le => {
                    self.bump();
                    let right = self.parse_additive()?;
                    left = Expr::Le(Box::new(left), Box::new(right));
                }
                Token::Gt => {
                    self.bump();
                    let right = self.parse_additive()?;
                    left = Expr::Gt(Box::new(left), Box::new(right));
                }
                Token::Ge => {
                    self.bump();
                    let right = self.parse_additive()?;
                    left = Expr::Ge(Box::new(left), Box::new(right));
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

    /// Parses left-associative `*` / `/` chains.
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
                    let Expr::Ident(name) = e else {
                        return Err("call requires identifier".into());
                    };
                    self.bump();
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
                    e = Expr::Call { name, args };
                }
                Token::Dot => {
                    self.bump();
                    let field = match self.bump() {
                        Token::Ident(s) => s,
                        Token::Int(n) => n.to_string(),
                        other => {
                            return Err(format!("expected field name or index, got {other:?}"));
                        }
                    };
                    e = Expr::Field(Box::new(e), field);
                }
                Token::LBracket => {
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
            Token::Match => self.parse_match_expr(),
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
                Token::Ident(name) => {
                    if matches!(self.peek(), Token::DoubleColon) {
                        self.bump();
                        let variant = self.expect_ident()?;
                        if matches!(self.peek(), Token::LParen) {
                            self.bump();
                            let mut args = Vec::new();
                            if !matches!(self.peek(), Token::RParen) {
                                loop {
                                    args.push(self.parse_expr()?);
                                    match self.peek() {
                                        Token::Comma => {
                                            self.bump();
                                            if matches!(self.peek(), Token::RParen) {
                                                break;
                                            }
                                        }
                                        Token::RParen => break,
                                        _ => {
                                            return Err(format!(
                                                "expected , or ), got {:?}",
                                                self.peek()
                                            ));
                                        }
                                    }
                                }
                            }
                            self.expect(&Token::RParen)?;
                            return Ok(Expr::EnumTuple {
                                enum_name: name,
                                variant,
                                args,
                            });
                        }
                        if matches!(self.peek(), Token::LBrace) {
                            self.bump();
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
                                            if matches!(self.peek(), Token::RBrace) {
                                                break;
                                            }
                                        }
                                        Token::RBrace => break,
                                        _ => {
                                            return Err(format!(
                                                "expected , or }}, got {:?}",
                                                self.peek()
                                            ));
                                        }
                                    }
                                }
                            }
                            self.expect(&Token::RBrace)?;
                            return Ok(Expr::EnumStruct {
                                enum_name: name,
                                variant,
                                fields,
                            });
                        }
                        return Ok(Expr::EnumVariant {
                            enum_name: name,
                            variant,
                        });
                    }
                    if self.looks_like_struct_lit_after_ident() {
                        self.parse_struct_lit_tail(name)
                    } else if self.looks_like_vector_lit_after_ident() {
                        self.parse_vector_lit_tail(name)
                    } else {
                        Ok(Expr::Ident(name))
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
            let enum_name = self.expect_ident()?;
            self.expect(&Token::DoubleColon)?;
            let variant = self.expect_ident()?;
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
