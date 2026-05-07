use std::iter::Peekable;
use std::str::Chars;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Fn,
    Let,
    Struct,
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
    Star,
    Amp,
    Dot,
    DotDot,
    DoubleColon,
    FatArrow,
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
            '+' => Token::Plus,
            '*' => Token::Star,
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
                    _ => Token::Ident(s),
                }
            }
            _ => Token::Eof,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test helper that tokenizes full input (excluding EOF marker).
    fn collect(src: &str) -> Vec<Token> {
        let mut l = Lexer::new(src);
        let mut out = Vec::new();
        loop {
            let t = l.next_token();
            if matches!(t, Token::Eof) {
                break;
            }
            out.push(t);
        }
        out
    }

    #[test]
    /// Verifies all supported keywords and primitive type names are recognized.
    fn lex_keywords_and_types() {
        let src = "fn let struct if return true false i8 u8 i16 u16 i32 i64 u64 i128 isize usize u128 bool";
        let toks = collect(src);
        assert_eq!(
            toks,
            vec![
                Token::Fn,
                Token::Let,
                Token::Struct,
                Token::If,
                Token::Return,
                Token::Bool(true),
                Token::Bool(false),
                Token::TyI8,
                Token::TyU8,
                Token::TyI16,
                Token::TyU16,
                Token::TyI32,
                Token::TyI64,
                Token::TyU64,
                Token::TyI128,
                Token::TyIsize,
                Token::TyUsize,
                Token::TyU128,
                Token::TyBool,
            ]
        );
    }

    #[test]
    fn lex_while_keyword() {
        let toks = collect("while x");
        assert_eq!(
            toks,
            vec![Token::While, Token::Ident("x".into()),]
        );
    }

    #[test]
    fn lex_loop_and_break() {
        let toks = collect("loop { break; }");
        assert_eq!(
            toks,
            vec![
                Token::Loop,
                Token::LBrace,
                Token::Break,
                Token::Semi,
                Token::RBrace,
            ]
        );
    }

    #[test]
    fn lex_for_in_and_dotdot() {
        let src = "for i in 0..1";
        let toks = collect(src);
        assert_eq!(
            toks,
            vec![
                Token::For,
                Token::Ident("i".into()),
                Token::In,
                Token::Int(0),
                Token::DotDot,
                Token::Int(1),
            ]
        );
    }

    #[test]
    /// Verifies punctuation/operator tokens and comment skipping behavior.
    fn lex_symbols_and_comments() {
        let src = "a: b, c; ( ) { } [ ] + * & . = // comment\n42";
        let toks = collect(src);
        assert_eq!(
            toks,
            vec![
                Token::Ident("a".into()),
                Token::Colon,
                Token::Ident("b".into()),
                Token::Comma,
                Token::Ident("c".into()),
                Token::Semi,
                Token::LParen,
                Token::RParen,
                Token::LBrace,
                Token::RBrace,
                Token::LBracket,
                Token::RBracket,
                Token::Plus,
                Token::Star,
                Token::Amp,
                Token::Dot,
                Token::Eq,
                Token::Int(42),
            ]
        );
    }

    #[test]
    /// Verifies mixed whitespace/comments with multiple identifier tokens.
    fn lex_multiline_comments_whitespace_and_identifiers() {
        let src = "\n  // skip\nfoo\n// skip2\nbar_1";
        let toks = collect(src);
        assert_eq!(
            toks,
            vec![Token::Ident("foo".into()), Token::Ident("bar_1".into())]
        );
    }

    #[test]
    /// Documents current fallback behavior for unknown characters (`@` => EOF).
    fn lex_unknown_char_stops_token_stream() {
        let src = "let x = 1 @ 2";
        let toks = collect(src);
        // Current lexer behavior returns EOF on unknown char.
        assert_eq!(
            toks,
            vec![Token::Let, Token::Ident("x".into()), Token::Eq, Token::Int(1)]
        );
    }
}
