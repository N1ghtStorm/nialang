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
    let src = "extern pub fn let mod struct impl quant gpu if return true false i8 u8 i16 u16 i32 i64 u64 i128 isize usize u128 bool f16 f32 f64";
    let toks = collect(src);
    assert_eq!(
        toks,
        vec![
            Token::Extern,
            Token::Pub,
            Token::Fn,
            Token::Let,
            Token::Mod,
            Token::Struct,
            Token::Impl,
            Token::Quant,
            Token::Gpu,
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
fn lex_ability_keywords() {
    assert_eq!(
        collect("has copy clone drop deref"),
        vec![
            Token::Has,
            Token::Copy,
            Token::Clone,
            Token::Drop,
            Token::Deref,
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
fn lex_integer_method_postfix_keeps_dot_token() {
    let toks = collect("1.clone()");
    assert_eq!(
        toks,
        vec![
            Token::Int(1),
            Token::Dot,
            Token::Clone,
            Token::LParen,
            Token::RParen,
        ]
    );
}

#[test]
fn lex_numeric_literals_with_digit_separators() {
    let toks = collect("1_000 3.141_592 1.0e1_0 1_000..2_000");
    assert_eq!(
        toks,
        vec![
            Token::Int(1_000),
            Token::Float(3.141_592),
            Token::Float(1.0e10),
            Token::Int(1_000),
            Token::DotDot,
            Token::Int(2_000),
        ]
    );
}

#[test]
fn lex_numeric_separator_must_be_between_digits() {
    assert_eq!(
        collect("1__000"),
        vec![Token::Int(1), Token::Ident("__000".into())]
    );
    assert_eq!(collect("1_"), vec![Token::Int(1), Token::Ident("_".into())]);
    assert_eq!(collect("_1"), vec![Token::Ident("_1".into())]);
}

#[test]
fn lex_while_keyword() {
    let toks = collect("while x");
    assert_eq!(toks, vec![Token::While, Token::Ident("x".into()),]);
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
fn lex_thin_arrow_for_function_types() {
    let toks = collect("fn(i32) -> ()");
    assert_eq!(
        toks,
        vec![
            Token::Fn,
            Token::LParen,
            Token::TyI32,
            Token::RParen,
            Token::ThinArrow,
            Token::LParen,
            Token::RParen,
        ]
    );
}

#[test]
fn lex_bitwise_remainder_and_compound() {
    let toks = collect("a % b & c | d ^ ~e << f >> g %= h &= i |= j ^= k <<= l >>= m");
    assert_eq!(
        toks,
        vec![
            Token::Ident("a".into()),
            Token::Percent,
            Token::Ident("b".into()),
            Token::Amp,
            Token::Ident("c".into()),
            Token::Pipe,
            Token::Ident("d".into()),
            Token::Caret,
            Token::Tilde,
            Token::Ident("e".into()),
            Token::Shl,
            Token::Ident("f".into()),
            Token::Shr,
            Token::Ident("g".into()),
            Token::PercentEq,
            Token::Ident("h".into()),
            Token::AmpEq,
            Token::Ident("i".into()),
            Token::PipeEq,
            Token::Ident("j".into()),
            Token::CaretEq,
            Token::Ident("k".into()),
            Token::ShlEq,
            Token::Ident("l".into()),
            Token::ShrEq,
            Token::Ident("m".into()),
        ]
    );
}

#[test]
fn lex_comparison_ops() {
    let toks = collect("!a == b != c < d <= e > f >= g");
    assert_eq!(
        toks,
        vec![
            Token::Bang,
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
fn lex_at_vector_dot() {
    let toks = collect("u @ v");
    assert_eq!(
        toks,
        vec![
            Token::Ident("u".into()),
            Token::At,
            Token::Ident("v".into()),
        ]
    );
}

#[test]
/// Verifies punctuation/operator tokens and comment skipping behavior.
fn lex_symbols_and_comments() {
    let src = "a: b, c; ( ) { } [ ] + * @ & . = // comment\n42";
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
            Token::At,
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
/// Documents current fallback behavior for unknown characters (`#` stops tokenization).
fn lex_unknown_char_stops_token_stream() {
    let src = "let x = 1 # 2";
    let toks = collect(src);
    assert_eq!(
        toks,
        vec![
            Token::Let,
            Token::Ident("x".into()),
            Token::Eq,
            Token::Int(1)
        ]
    );
}

#[test]
fn lex_string_type_and_literals() {
    let src = r#"string x "hi" "a\nb""#;
    assert_eq!(
        collect(src),
        vec![
            Token::TyString,
            Token::Ident("x".into()),
            Token::StrLit("hi".into()),
            Token::StrLit("a\nb".into()),
        ]
    );
}
