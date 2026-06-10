use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Extern,
    Fn,
    Let,
    Struct,
    Vector,
    Enum,
    Impl,
    Quant,
    Gpu,
    If,
    While,
    Loop,
    Break,
    For,
    In,
    Match,
    Return,
    Decreases,
    Partial,
    Requires,
    Ensures,
    Admit,
    Ident(String),
    Int(i128),
    Float(f64),
    Bool(bool),
    /// Decoded UTF-8 contents (no surrounding quotes).
    StrLit(String),
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
    /// `@` — vector dot product (only between two vectors in the typechecker).
    At,
    Slash,
    SlashEq,
    Percent,
    PercentEq,
    Amp,
    AmpEq,
    Pipe,
    PipeEq,
    Caret,
    CaretEq,
    Bang,
    Hash,
    Tilde,
    Shl,
    ShlEq,
    Shr,
    ShrEq,
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
    TyString,
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

    /// Reads a double-quoted string; opening `"` already consumed.
    fn lex_string_literal(&mut self) -> Token {
        let mut s = String::new();
        loop {
            let Some(c) = self.src.next() else {
                return Token::StrLit(s);
            };
            match c {
                '"' => return Token::StrLit(s),
                '\\' => match self.src.next() {
                    Some('n') => s.push('\n'),
                    Some('t') => s.push('\t'),
                    Some('r') => s.push('\r'),
                    Some('0') => s.push('\0'),
                    Some('"') => s.push('"'),
                    Some('\\') => s.push('\\'),
                    Some(other) => s.push(other),
                    None => return Token::StrLit(s),
                },
                other => s.push(other),
            }
        }
    }

    /// Appends decimal digits while allowing `_` only between two digits.
    ///
    /// Separators are omitted from `buf`, so the result can be parsed directly
    /// by Rust's numeric parsers.
    fn lex_decimal_digits(&mut self, buf: &mut String) {
        loop {
            match self.src.peek() {
                Some(&d @ '0'..='9') => {
                    buf.push(d);
                    self.src.next();
                }
                Some('_') => {
                    let mut ahead = self.src.clone();
                    ahead.next();
                    if matches!(ahead.peek(), Some('0'..='9')) {
                        self.src.next();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
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
    /// - Parses decimal numeric literals with optional `_` digit separators.
    /// - Parses double-quoted string literals with escapes (`\\`, `\"`, `\n`, `\t`, `\r`, `\0`).
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
            '@' => Token::At,
            '/' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::SlashEq
            }
            '/' => Token::Slash,
            '%' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::PercentEq
            }
            '%' => Token::Percent,
            '&' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::AmpEq
            }
            '&' => Token::Amp,
            '|' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::PipeEq
            }
            '|' => Token::Pipe,
            '^' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::CaretEq
            }
            '^' => Token::Caret,
            '~' => Token::Tilde,
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
            '!' => Token::Bang,
            '#' => Token::Hash,
            '<' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::Le
            }
            '<' if matches!(self.src.peek(), Some('<')) => {
                self.src.next();
                if matches!(self.src.peek(), Some('=')) {
                    self.src.next();
                    Token::ShlEq
                } else {
                    Token::Shl
                }
            }
            '<' => Token::Lt,
            '>' if matches!(self.src.peek(), Some('=')) => {
                self.src.next();
                Token::Ge
            }
            '>' if matches!(self.src.peek(), Some('>')) => {
                self.src.next();
                if matches!(self.src.peek(), Some('=')) {
                    self.src.next();
                    Token::ShrEq
                } else {
                    Token::Shr
                }
            }
            '>' => Token::Gt,
            '"' => self.lex_string_literal(),
            '0'..='9' => {
                let mut buf = String::new();
                buf.push(c);
                self.lex_decimal_digits(&mut buf);
                if matches!(self.src.peek(), Some(&'.')) {
                    let mut ahead = self.src.clone();
                    ahead.next();
                    if matches!(ahead.peek(), Some(&'.')) {
                        let n = buf.parse::<i128>().unwrap_or(0);
                        return Token::Int(n);
                    }
                    buf.push(self.src.next().unwrap());
                    self.lex_decimal_digits(&mut buf);
                    if matches!(self.src.peek(), Some('e') | Some('E')) {
                        buf.push(self.src.next().unwrap());
                        if matches!(self.src.peek(), Some('+') | Some('-')) {
                            buf.push(self.src.next().unwrap());
                        }
                        self.lex_decimal_digits(&mut buf);
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
                    "extern" => Token::Extern,
                    "fn" => Token::Fn,
                    "let" => Token::Let,
                    "struct" => Token::Struct,
                    "vector" => Token::Vector,
                    "enum" => Token::Enum,
                    "impl" => Token::Impl,
                    "quant" => Token::Quant,
                    "gpu" => Token::Gpu,
                    "if" => Token::If,
                    "while" => Token::While,
                    "loop" => Token::Loop,
                    "break" => Token::Break,
                    "for" => Token::For,
                    "in" => Token::In,
                    "match" => Token::Match,
                    "return" => Token::Return,
                    "decreases" => Token::Decreases,
                    "partial" => Token::Partial,
                    "requires" => Token::Requires,
                    "ensures" => Token::Ensures,
                    "admit" => Token::Admit,
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
                    "string" => Token::TyString,
                    _ => Token::Ident(s),
                }
            }
            _ => Token::Eof,
        }
    }
}

#[cfg(test)]
mod tests;
