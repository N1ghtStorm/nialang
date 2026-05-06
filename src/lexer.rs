use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Fn,
    Let,
    Struct,
    If,
    Return,
    Ident(String),
    Int(i128),
    Bool(bool),
    Colon,
    Comma,
    Semi,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Plus,
    Star,
    Amp,
    Dot,
    Eq,
    TyI8,
    TyU8,
    TyI16,
    TyU16,
    TyI32,
    TyI64,
    TyU64,
    TyI128,
    TyIsize,
    TyUsize,
    TyU128,
    TyBool,
    Eof,
}

pub struct Lexer<'a> {
    src: Peekable<Chars<'a>>,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self {
            src: input.chars().peekable(),
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            match self.src.peek() {
                Some(&c) if c.is_whitespace() => {
                    self.src.next();
                }
                Some(&'/') => {
                    let mut it = self.src.clone();
                    it.next();
                    match it.peek() {
                        Some(&'/') => {
                            self.src.next();
                            self.src.next();
                            while let Some(&ch) = self.src.peek() {
                                if ch == '\n' {
                                    break;
                                }
                                self.src.next();
                            }
                        }
                        _ => break,
                    }
                }
                _ => break,
            }
        }
    }

    pub fn next_token(&mut self) -> Token {
        self.skip_ws_and_comments();
        let Some(c) = self.src.next() else {
            return Token::Eof;
        };
        match c {
            ':' => Token::Colon,
            ',' => Token::Comma,
            ';' => Token::Semi,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '+' => Token::Plus,
            '*' => Token::Star,
            '&' => Token::Amp,
            '.' => Token::Dot,
            '=' => Token::Eq,
            '0'..='9' => {
                let mut n = (c as u8 - b'0') as i128;
                while let Some(&d @ '0'..='9') = self.src.peek() {
                    self.src.next();
                    n = n
                        .saturating_mul(10)
                        .saturating_add((d as u8 - b'0') as i128);
                }
                Token::Int(n)
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                let mut s = String::new();
                s.push(c);
                while let Some(&ch) = self.src.peek() {
                    if ch.is_ascii_alphanumeric() || ch == '_' {
                        s.push(self.src.next().unwrap());
                    } else {
                        break;
                    }
                }
                match s.as_str() {
                    "fn" => Token::Fn,
                    "let" => Token::Let,
                    "struct" => Token::Struct,
                    "if" => Token::If,
                    "return" => Token::Return,
                    "true" => Token::Bool(true),
                    "false" => Token::Bool(false),
                    "i8" => Token::TyI8,
                    "u8" => Token::TyU8,
                    "i16" => Token::TyI16,
                    "u16" => Token::TyU16,
                    "i32" => Token::TyI32,
                    "i64" => Token::TyI64,
                    "u64" => Token::TyU64,
                    "i128" => Token::TyI128,
                    "isize" => Token::TyIsize,
                    "usize" => Token::TyUsize,
                    "u128" => Token::TyU128,
                    "bool" => Token::TyBool,
                    _ => Token::Ident(s),
                }
            }
            _ => Token::Eof,
        }
    }
}
