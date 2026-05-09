use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Fn,
    Let,
    Struct,
    Vector,
    Enum,
    If,
    While,
    Loop,
    Break,
    For,
    In,
    Match,
    Return,
    Ident(String),
    Int(i128),
    Float(f64),
    Bool(bool),
    Colon,
    Comma,
    Semi,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Plus,
    PlusEq,
    Minus,
    MinusEq,
    Star,
    StarEq,
    Slash,
    SlashEq,
    Amp,
    Dot,
    DotDot,
    DoubleColon,
    FatArrow,
    Eq,
    EqEq,
    NotEq,
    Lt,
    Le,
    Gt,
    Ge,
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
    TyF16,
    TyF32,
    TyF64,
    Eof,
}

pub struct Lexer<'a> {
    src: Peekable<Chars<'a>>,
}

impl<'a> Lexer<'a> {
    /// Creates a lexer over the provided source text.
    ///
    /// The lexer is a simple single-pass scanner with one-token lookahead
    /// implemented via `Peekable<Chars>`.
    pub fn new(input: &'a str) -> Self {
        Self {
            src: input.chars().peekable(),
        }
    }

    /// Consumes whitespace and line comments (`// ...`) before tokenization.
    ///
    /// This method loops until it reaches a character that can start a real token.
    /// Comments are skipped up to, but not including, the newline terminator.
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

    /// Produces the next token from input stream.
    ///
    /// ## Current behavior details
    /// - Skips whitespace/comments first.
    /// - Parses decimal integers only.
    /// - Parses identifiers/keywords with ASCII alnum + `_` rule.
    /// - On unknown characters returns `Token::Eof` (simple fail-stop behavior).
    ///
    /// Integer parsing uses saturating arithmetic to avoid panics for huge literals.
    pub fn next_token(&mut self) -> Token {
        self.skip_ws_and_comments();
        let Some(c) = self.src.next() else {
            return Token::Eof;
        };
        match c {
            ':' if matches!(self.src.peek(), Some(':')) => {
                self.src.next();
                Token::DoubleColon
            }
            ':' => Token::Colon,
            ',' => Token::Comma,
            ';' => Token::Semi,
            '(' => Token::LParen,
            ')' => Token::RParen,
            '{' => Token::LBrace,
            '}' => Token::RBrace,
            '[' => Token::LBracket,
            ']' => Token::RBracket,
            '+' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::PlusEq
            }
            '+' => Token::Plus,
            '-' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::MinusEq
            }
            '-' => Token::Minus,
            '*' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::StarEq
            }
            '*' => Token::Star,
            '/' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::SlashEq
            }
            '/' => Token::Slash,
            '&' => Token::Amp,
            '.' if matches!(self.src.peek(), Some('.')) => {
                self.src.next();
                Token::DotDot
            }
            '.' => Token::Dot,
            '=' if matches!(self.src.peek(), Some('>')) => {
                self.src.next();
                Token::FatArrow
            }
            '=' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::EqEq
            }
            '=' => Token::Eq,
            '!' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::NotEq
            }
            '<' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::Le
            }
            '<' => Token::Lt,
            '>' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::Ge
            }
            '>' => Token::Gt,
            '0'..='9' => {
                let mut buf = String::new();
                buf.push(c);
                while let Some(&_d @ '0'..='9') = self.src.peek() {
                    buf.push(self.src.next().unwrap());
                }
                if matches!(self.src.peek(), Some(&'.')) {
                    let mut ahead = self.src.clone();
                    ahead.next();
                    if matches!(ahead.peek(), Some(&'.')) {
                        let n = buf.parse::<i128>().unwrap_or(0);
                        return Token::Int(n);
                    }
                    buf.push(self.src.next().unwrap());
                    while let Some(&_d @ '0'..='9') = self.src.peek() {
                        buf.push(self.src.next().unwrap());
                    }
                    if matches!(self.src.peek(), Some('e') | Some('E')) {
                        buf.push(self.src.next().unwrap());
                        if matches!(self.src.peek(), Some('+') | Some('-')) {
                            buf.push(self.src.next().unwrap());
                        }
                        while let Some(&_d @ '0'..='9') = self.src.peek() {
                            buf.push(self.src.next().unwrap());
                        }
                    }
                    let v = buf.parse::<f64>().unwrap_or(0.0);
                    return Token::Float(v);
                }
                let n = buf.parse::<i128>().unwrap_or(0);
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
                    "vector" => Token::Vector,
                    "enum" => Token::Enum,
                    "if" => Token::If,
                    "while" => Token::While,
                    "loop" => Token::Loop,
                    "break" => Token::Break,
                    "for" => Token::For,
                    "in" => Token::In,
                    "match" => Token::Match,
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
                    "f16" => Token::TyF16,
                    "f32" => Token::TyF32,
                    "f64" => Token::TyF64,
                    _ => Token::Ident(s),
                }
            }
            _ => Token::Eof,
        }
    }
}

#[cfg(test)]
mod tests;
