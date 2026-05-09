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
    let src = "fn let struct if return true false i8 u8 i16 u16 i32 i64 u64 i128 isize usize u128 bool f16 f32 f64";
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
            Token::TyF16,
            Token::TyF32,
            Token::TyF64,
        ]
    );
}

#[test]
fn lex_float_literals_fraction_and_exponent() {
    let toks = collect("1.5 0.25 1.0e3 2.5e-2");
    assert_eq!(
        toks,
        vec![
            Token::Float(1.5),
            Token::Float(0.25),
            Token::Float(1.0e3),
            Token::Float(2.5e-2),
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
fn lex_arithmetic_and_compound() {
    let toks = collect("a += b - c * d / e");
    assert_eq!(
        toks,
        vec![
            Token::Ident("a".into()),
            Token::PlusEq,
            Token::Ident("b".into()),
            Token::Minus,
            Token::Ident("c".into()),
            Token::Star,
            Token::Ident("d".into()),
            Token::Slash,
            Token::Ident("e".into()),
        ]
    );
}

#[test]
fn lex_comparison_ops() {
    let toks = collect("a == b != c < d <= e > f >= g");
    assert_eq!(
        toks,
        vec![
            Token::Ident("a".into()),
            Token::EqEq,
            Token::Ident("b".into()),
            Token::NotEq,
            Token::Ident("c".into()),
            Token::Lt,
            Token::Ident("d".into()),
            Token::Le,
            Token::Ident("e".into()),
            Token::Gt,
            Token::Ident("f".into()),
            Token::Ge,
            Token::Ident("g".into()),
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
