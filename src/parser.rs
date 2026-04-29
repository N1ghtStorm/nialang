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
        self.expect(&Token::LBrace)?;
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
        Ok(StructDef { name, fields })
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
                _ => {
                    let e = self.parse_expr()?;
                    if matches!(self.peek(), Token::Semi) {
                        return Err(
                            "trailing expression must not end with ';' (use it as function tail)"
                                .into(),
                        );
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

    fn parse_ty(&mut self) -> Result<Ty, String> {
        match self.bump() {
            Token::TyI32 => Ok(Ty::I32),
            Token::TyU128 => Ok(Ty::U128),
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
                    let field = self.expect_ident()?;
                    e = Expr::Field(Box::new(e), field);
                }
                _ => break,
            }
        }
        Ok(e)
    }

    fn parse_atom(&mut self) -> Result<Expr, String> {
        match self.bump() {
            Token::Int(n) => Ok(Expr::Int(n)),
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

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.bump() {
            Token::Ident(s) => Ok(s),
            other => Err(format!("expected identifier, got {other:?}")),
        }
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
