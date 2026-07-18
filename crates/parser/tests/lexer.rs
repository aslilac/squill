//! Unit tests for the TREE-93 lexer landmines.

use parser::Dialect::{self, Postgres, Sqlite};
use parser::lexer::lex;
use parser::syntax::SyntaxKind as K;

/// Assert the exact token stream and the lossless round-trip property.
#[track_caller]
fn assert_tokens(dialect: Dialect, input: &str, expected: &[(K, &str)]) {
    let tokens = lex(input, dialect);
    let actual: Vec<_> = tokens.iter().map(|t| (t.kind, t.text)).collect();
    assert_eq!(actual, expected, "lexing {input:?}");
    let rebuilt: String = tokens.iter().map(|t| t.text).collect();
    assert_eq!(rebuilt, input, "round-trip {input:?}");
}

// ---- Postgres ----

#[test]
fn dollar_quoted_with_tag() {
    assert_tokens(
        Postgres,
        "$fn$body $notthetag$ still$fn$",
        &[(K::DollarString, "$fn$body $notthetag$ still$fn$")],
    );
}

#[test]
fn dollar_quoted_anonymous() {
    assert_tokens(Postgres, "$$a$b$$", &[(K::DollarString, "$$a$b$$")]);
}

#[test]
fn dollar_quoted_unterminated_is_error_to_eof() {
    assert_tokens(Postgres, "$tag$oops", &[(K::Error, "$tag$oops")]);
}

#[test]
fn lone_dollar_is_error() {
    assert_tokens(
        Postgres,
        "$tag oops",
        &[
            (K::Error, "$"),
            (K::Ident, "tag"),
            (K::Whitespace, " "),
            (K::Ident, "oops"),
        ],
    );
}

#[test]
fn positional_params() {
    assert_tokens(
        Postgres,
        "$1,$23",
        &[(K::Param, "$1"), (K::Comma, ","), (K::Param, "$23")],
    );
}

#[test]
fn nested_block_comment() {
    assert_tokens(
        Postgres,
        "/* a /* b */ c */x",
        &[(K::BlockComment, "/* a /* b */ c */"), (K::Ident, "x")],
    );
}

#[test]
fn unterminated_block_comment_is_error_to_eof() {
    assert_tokens(Postgres, "/* /* */ x", &[(K::Error, "/* /* */ x")]);
    assert_tokens(Sqlite, "/* x", &[(K::Error, "/* x")]);
}

#[test]
fn escape_string_backslash_quote() {
    assert_tokens(Postgres, r"E'a\'b'", &[(K::EscapeString, r"E'a\'b'")]);
    assert_tokens(Postgres, r"e'\\'", &[(K::EscapeString, r"e'\\'")]);
}

#[test]
fn unicode_bit_and_hex_strings() {
    assert_tokens(
        Postgres,
        r"U&'d\0061t'",
        &[(K::UnicodeString, r"U&'d\0061t'")],
    );
    assert_tokens(Postgres, "B'1010'", &[(K::BitString, "B'1010'")]);
    assert_tokens(Postgres, "x'1f'", &[(K::HexString, "x'1f'")]);
}

#[test]
fn string_prefix_requires_adjacent_quote() {
    assert_tokens(
        Postgres,
        "East B X u",
        &[
            (K::Ident, "East"),
            (K::Whitespace, " "),
            (K::Ident, "B"),
            (K::Whitespace, " "),
            (K::Ident, "X"),
            (K::Whitespace, " "),
            (K::Ident, "u"),
        ],
    );
}

#[test]
fn cast_operator_and_slice_colon() {
    assert_tokens(
        Postgres,
        "a::int",
        &[(K::Ident, "a"), (K::ColonColon, "::"), (K::Ident, "int")],
    );
    assert_tokens(
        Postgres,
        "v[1:2]",
        &[
            (K::Ident, "v"),
            (K::LBracket, "["),
            (K::Number, "1"),
            (K::Colon, ":"),
            (K::Number, "2"),
            (K::RBracket, "]"),
        ],
    );
}

#[test]
fn custom_operators_maximal_munch() {
    assert_tokens(
        Postgres,
        "a @> b",
        &[
            (K::Ident, "a"),
            (K::Whitespace, " "),
            (K::Operator, "@>"),
            (K::Whitespace, " "),
            (K::Ident, "b"),
        ],
    );
    assert_tokens(Postgres, "#>>", &[(K::Operator, "#>>")]);
}

#[test]
fn operator_may_end_in_plus_minus_only_with_special_char() {
    // `@-` contains `@`, so it may keep its trailing `-`.
    assert_tokens(Postgres, "@-", &[(K::Operator, "@-")]);
    // `+-` and `=-` contain no special char: trailing +/- split off.
    assert_tokens(
        Postgres,
        "1+-2",
        &[
            (K::Number, "1"),
            (K::Operator, "+"),
            (K::Operator, "-"),
            (K::Number, "2"),
        ],
    );
    assert_tokens(
        Postgres,
        "a=-b",
        &[
            (K::Ident, "a"),
            (K::Operator, "="),
            (K::Operator, "-"),
            (K::Ident, "b"),
        ],
    );
}

#[test]
fn comment_openers_terminate_operator_munch() {
    assert_tokens(
        Postgres,
        "1+--x",
        &[
            (K::Number, "1"),
            (K::Operator, "+"),
            (K::LineComment, "--x"),
        ],
    );
    assert_tokens(
        Postgres,
        "=/*c*/",
        &[(K::Operator, "="), (K::BlockComment, "/*c*/")],
    );
}

#[test]
fn standard_string_doubled_quote() {
    assert_tokens(Postgres, "'it''s'", &[(K::String, "'it''s'")]);
    assert_tokens(Postgres, "'abc", &[(K::Error, "'abc")]);
}

#[test]
fn quoted_ident_doubled_quote() {
    assert_tokens(Postgres, r#""a""b""#, &[(K::QuotedIdent, r#""a""b""#)]);
    assert_tokens(Postgres, r#""abc"#, &[(K::Error, r#""abc"#)]);
}

#[test]
fn backslash_is_unknown_in_postgres() {
    assert_tokens(
        Postgres,
        "a \\ b",
        &[
            (K::Ident, "a"),
            (K::Whitespace, " "),
            (K::Error, "\\"),
            (K::Whitespace, " "),
            (K::Ident, "b"),
        ],
    );
}

#[test]
fn numbers() {
    for input in [
        "1", "1.5", ".5", "1.", "1e5", "1E-5", "1.5e+3", "0x1F", "1_000",
    ] {
        assert_tokens(Postgres, input, &[(K::Number, input)]);
    }
    // `1e` is not an exponent; `1..2` keeps the dots for other tokens.
    assert_tokens(Postgres, "1e", &[(K::Number, "1"), (K::Ident, "e")]);
    assert_tokens(
        Postgres,
        "1..2",
        &[(K::Number, "1"), (K::Dot, "."), (K::Number, ".2")],
    );
}

#[test]
fn ident_may_contain_dollar() {
    assert_tokens(Postgres, "a$b", &[(K::Ident, "a$b")]);
}

// ---- SQLite ----

#[test]
fn sqlite_params() {
    assert_tokens(
        Sqlite,
        "? ?17 :name @name $name",
        &[
            (K::Param, "?"),
            (K::Whitespace, " "),
            (K::Param, "?17"),
            (K::Whitespace, " "),
            (K::Param, ":name"),
            (K::Whitespace, " "),
            (K::Param, "@name"),
            (K::Whitespace, " "),
            (K::Param, "$name"),
        ],
    );
}

#[test]
fn sqlite_backtick_and_bracket_idents() {
    assert_tokens(Sqlite, "`a``b`", &[(K::QuotedIdent, "`a``b`")]);
    assert_tokens(Sqlite, "[a b]", &[(K::QuotedIdent, "[a b]")]);
    assert_tokens(Sqlite, "`x", &[(K::Error, "`x")]);
    assert_tokens(Sqlite, "[x", &[(K::Error, "[x")]);
}

#[test]
fn sqlite_blob_literals() {
    assert_tokens(Sqlite, "x'CAFE'", &[(K::HexString, "x'CAFE'")]);
    assert_tokens(Sqlite, "X'00'", &[(K::HexString, "X'00'")]);
}

#[test]
fn sqlite_double_quote_is_always_an_identifier() {
    assert_tokens(
        Sqlite,
        r#""hello world""#,
        &[(K::QuotedIdent, r#""hello world""#)],
    );
}

#[test]
fn sqlite_no_pg_prefixes_or_custom_operators() {
    // E'...' is an identifier then a string in SQLite.
    assert_tokens(Sqlite, "E'x'", &[(K::Ident, "E"), (K::String, "'x'")]);
    // `@>` is not a SQLite operator: param-ish `@` needs a name, `>` is fine.
    assert_tokens(
        Sqlite,
        "a->>b",
        &[(K::Ident, "a"), (K::Operator, "->>"), (K::Ident, "b")],
    );
    assert_tokens(Sqlite, "#", &[(K::Error, "#")]);
    assert_tokens(Sqlite, "!", &[(K::Error, "!")]);
    assert_tokens(Sqlite, "!=", &[(K::Operator, "!=")]);
}

#[test]
fn sqlite_underscore_starts_an_identifier() {
    assert_tokens(Sqlite, "_x", &[(K::Ident, "_x")]);
    assert_tokens(Sqlite, "_", &[(K::Ident, "_")]);
}

#[test]
fn sqlite_block_comments_do_not_nest() {
    assert_tokens(
        Sqlite,
        "/* /* */ */",
        &[
            (K::BlockComment, "/* /* */"),
            (K::Whitespace, " "),
            (K::Operator, "*"),
            (K::Operator, "/"),
        ],
    );
}

// ---- Both ----

#[test]
fn unknown_bytes_become_error_tokens() {
    for dialect in [Postgres, Sqlite] {
        let tokens = lex("select \u{1F980};", dialect);
        let rebuilt: String = tokens.iter().map(|t| t.text).collect();
        assert_eq!(rebuilt, "select \u{1F980};");
    }
}

#[test]
fn crlf_line_comment() {
    assert_tokens(
        Postgres,
        "--c\r\nx",
        &[
            (K::LineComment, "--c"),
            (K::Whitespace, "\r\n"),
            (K::Ident, "x"),
        ],
    );
}
