use crate::ast::{Block, Expr, FnDef, Stmt, StructDef, Ty};
use crate::lexer::Token;

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::Eof)
    }

    fn bump(&mut self) -> Token {
        let t = self.tokens.get(self.pos).cloned().unwrap_or(Token::Eof);
        if self.pos < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn expect(&mut self, want: &Token) -> Result<(), String> {
        let got = self.bump();
        if &got == want {
            Ok(())
        } else {
            Err(format!("expected {want:?}, got {got:?}"))
        }
    }

    pub fn parse_file(mut self) -> Result<(Vec<StructDef>, Vec<FnDef>), String> {
        let mut structs = Vec::new();
        let mut fns = Vec::new();
        while !matches!(self.peek(), Token::Eof) {
            match self.peek().clone() {
                Token::Struct => {
                    structs.push(self.parse_struct()?);
                }
                Token::Fn => {
                    fns.push(self.parse_fn()?);
                }
                other => return Err(format!("unexpected token at top level: {other:?}")),
            }
        }
        Ok((structs, fns))
    }

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
                Token::Return => {
                    stmts.push(self.parse_return_stmt()?);
                }
                _ => {
                    let e = self.parse_expr()?;
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

    fn parse_if_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::If)?;
        let cond = self.parse_if_cond()?;
        let then_block = self.parse_block()?;
        Ok(Stmt::If { cond, then_block })
    }

    fn parse_if_cond(&mut self) -> Result<Expr, String> {
        // Keep `if foo { ... }` unambiguous with struct literals `Foo { ... }`.
        match self.bump() {
            Token::Ident(n) => Ok(Expr::Ident(n)),
            Token::Bool(b) => Ok(Expr::Bool(b)),
            Token::Int(n) => Ok(Expr::Int(n)),
            Token::LParen => {
                let e = self.parse_expr()?;
                self.expect(&Token::RParen)?;
                Ok(e)
            }
            Token::Amp => Ok(Expr::AddrOf(Box::new(self.parse_atom()?))),
            Token::Star => Ok(Expr::Deref(Box::new(self.parse_atom()?))),
            other => Err(format!("unexpected token in if condition: {other:?}")),
        }
    }

    fn parse_return_stmt(&mut self) -> Result<Stmt, String> {
        self.expect(&Token::Return)?;
        let e = self.parse_expr()?;
        if matches!(self.peek(), Token::Semi) {
            self.bump();
        }
        Ok(Stmt::Return(e))
    }

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
            Token::Ident(n) => Ok(Ty::Struct(n)),
            other => Err(format!("expected type, got {other:?}")),
        }
    }

    fn parse_expr(&mut self) -> Result<Expr, String> {
        self.parse_add()
    }

    fn parse_add(&mut self) -> Result<Expr, String> {
        let mut left = self.parse_suffix_chain()?;
        while matches!(self.peek(), Token::Plus) {
            self.bump();
            let right = self.parse_suffix_chain()?;
            left = Expr::Add(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

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
                        other => return Err(format!("expected field name or index, got {other:?}")),
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

    fn parse_atom(&mut self) -> Result<Expr, String> {
        match self.peek() {
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
            _ => match self.bump() {
                Token::Int(n) => Ok(Expr::Int(n)),
                Token::Bool(b) => Ok(Expr::Bool(b)),
                Token::Ident(name) => {
                    if matches!(self.peek(), Token::LBrace) {
                        self.parse_struct_lit_tail(name)
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

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.bump() {
            Token::Ident(s) => Ok(s),
            other => Err(format!("expected identifier, got {other:?}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) {
        let toks = tokenize(src);
        let r = Parser::new(toks).parse_file();
        assert!(r.is_ok(), "{r:?}");
    }

    #[test]
    fn parse_fixture_minimal() {
        parse_ok(include_str!("../examples/tests/ok_minimal.nia"));
    }

    #[test]
    fn parse_fixture_if_return() {
        parse_ok(include_str!("../examples/tests/ok_if_return.nia"));
    }

    #[test]
    fn parse_fixture_tuple_struct() {
        parse_ok(include_str!("../examples/tests/ok_tuple_struct.nia"));
    }

    #[test]
    fn parse_fixture_named_struct() {
        parse_ok(include_str!("../examples/tests/ok_struct_named.nia"));
    }

    #[test]
    fn parse_fixture_pointers() {
        parse_ok(include_str!("../examples/tests/ok_pointers.nia"));
    }

    #[test]
    fn parse_fixture_nested_if() {
        parse_ok(include_str!("../examples/tests/ok_nested_if.nia"));
    }

    #[test]
    fn parse_fixture_tuple_named_mix() {
        parse_ok(include_str!("../examples/tests/ok_tuple_named_mix.nia"));
    }

    #[test]
    fn parse_array_type_and_literal() {
        parse_ok(include_str!("../examples/tests/ok_array.nia"));
    }

    #[test]
    fn parse_array_index_expression() {
        let src = r#"
fn main() i32 {
    let arr: [u8; 3] = [1, 2, 3];
    let x: u8 = arr[1];
    0
}
"#;
        parse_ok(src);
    }

    #[test]
    fn parse_inline_if_return_bool() {
        let src = r#"
fn bar(foo: bool) i32 {
    if foo {
        return 1
    }
    0
}
"#;
        parse_ok(src);
    }

    #[test]
    fn parse_tuple_struct_and_index_field() {
        let src = r#"
struct Foo (u8, i32, u8, u128)
fn main() i32 {
    let f = Foo(1, 2, 3, 4);
    f.1
}
"#;
        parse_ok(src);
    }

    #[test]
    fn parse_rejects_bad_tuple_struct() {
        let src = "struct Foo (u8, i32";
        let toks = tokenize(src);
        let r = Parser::new(toks).parse_file();
        assert!(r.is_err());
    }

    #[test]
    fn parse_rejects_missing_struct_colon() {
        let src = "struct A { x i32 }";
        let toks = tokenize(src);
        let r = Parser::new(toks).parse_file();
        assert!(r.is_err());
    }

    #[test]
    fn parse_rejects_unclosed_block() {
        let src = "fn main() i32 { let a = 1;";
        let toks = tokenize(src);
        let r = Parser::new(toks).parse_file();
        assert!(r.is_err());
    }
}

pub fn tokenize(input: &str) -> Vec<Token> {
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
