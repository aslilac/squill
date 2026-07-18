//! TREE-96 acceptance: the identifier quoting transform is
//! semantics-preserving — for identifiers across syntactic positions, the
//! output re-lexes to a single identifier token that resolves to the same
//! name under each dialect's folding rules.

use formatter::doc::{IdentPos, ident};
use formatter::{IdentQuoting, Options, render};
use parser::Dialect;
use parser::lexer::lex;
use parser::syntax::SyntaxKind;

fn options(dialect: Dialect, quoting: IdentQuoting) -> Options {
    Options {
        dialect,
        quoting,
        ..Options::default()
    }
}

/// The name an identifier token denotes, canonicalized for comparison.
/// Postgres folds bare identifiers to ASCII lowercase and takes quoted
/// ones exactly; SQLite compares everything ASCII-case-insensitively.
fn resolve(token: &str, dialect: Dialect) -> String {
    let (inner, was_quoted) = if let Some(body) = token.strip_prefix('"') {
        (body.strip_suffix('"').unwrap().replace("\"\"", "\""), true)
    } else if let Some(body) = token.strip_prefix('`') {
        (body.strip_suffix('`').unwrap().replace("``", "`"), true)
    } else if let Some(body) = token.strip_prefix('[') {
        (body.strip_suffix(']').unwrap().to_string(), true)
    } else {
        (token.to_string(), false)
    };
    match dialect {
        Dialect::Postgres => {
            if was_quoted {
                inner
            } else {
                inner.to_ascii_lowercase()
            }
        }
        Dialect::Sqlite => inner.to_ascii_lowercase(),
    }
}

/// All identifier forms of `name` that lex as one identifier token in
/// `dialect`.
fn forms(name: &str, dialect: Dialect) -> Vec<String> {
    let mut forms = vec![format!("\"{}\"", name.replace('"', "\"\""))];
    if dialect == Dialect::Sqlite {
        forms.push(format!("`{}`", name.replace('`', "``")));
        if !name.contains([']', '[']) {
            forms.push(format!("[{name}]"));
        }
    }
    // Bare form, when it lexes as a single Ident token.
    let tokens = lex(name, dialect);
    if tokens.len() == 1 && tokens[0].kind == SyntaxKind::Ident {
        forms.push(name.to_string());
    }
    forms
}

#[test]
fn quoting_transform_preserves_semantics() {
    let names = [
        "foo",
        "foo_bar",
        "_x",
        "f$1",
        "x9",
        "Foo",
        "FOO",
        "MiXeD",
        "a b",
        "a\"b",
        "a`b",
        "über",
        "1st",
        "$weird",
        // Keywords across the PG categories (stable across versions).
        "select",       // reserved
        "table",        // reserved
        "abort",        // unreserved
        "between",      // col_name
        "int",          // col_name
        "binary",       // type_func_name
        "concurrently", // type_func_name
        "order",        // reserved in both dialects
        "group",
        "end",
    ];
    let positions = [IdentPos::ColumnOrTable, IdentPos::TypeOrFunction];
    let quotings = [IdentQuoting::UnquotedWhenSafe, IdentQuoting::AlwaysQuoted];
    let dialects = [Dialect::Postgres, Dialect::Sqlite];

    for dialect in dialects {
        for name in names {
            for input in forms(name, dialect) {
                for pos in positions {
                    for quoting in quotings {
                        let rendered =
                            render(&ident(input.clone(), pos), &options(dialect, quoting));
                        // Must re-lex as exactly one identifier token.
                        let tokens = lex(&rendered, dialect);
                        assert_eq!(
                            tokens.len(),
                            1,
                            "{dialect:?}/{quoting:?}/{pos:?}: {input:?} rendered to \
                             {rendered:?} which is not one token"
                        );
                        assert!(
                            matches!(tokens[0].kind, SyntaxKind::Ident | SyntaxKind::QuotedIdent),
                            "{dialect:?}/{quoting:?}/{pos:?}: {input:?} rendered to \
                             non-identifier {rendered:?}"
                        );
                        // And resolve to the same name.
                        assert_eq!(
                            resolve(&rendered, dialect),
                            resolve(&input, dialect),
                            "{dialect:?}/{quoting:?}/{pos:?}: {input:?} -> {rendered:?} \
                             changed the resolved identifier"
                        );
                    }
                }
            }
        }
    }
}

// ---- targeted expectations ----

#[test]
fn pg_strips_only_safe_lowercase() {
    let opts = options(Dialect::Postgres, IdentQuoting::UnquotedWhenSafe);
    let cases = [
        ("\"foo\"", IdentPos::ColumnOrTable, "foo"),
        ("\"Foo\"", IdentPos::ColumnOrTable, "\"Foo\""), // mixed case: keep
        ("\"select\"", IdentPos::ColumnOrTable, "\"select\""), // reserved
        ("\"abort\"", IdentPos::ColumnOrTable, "abort"), // unreserved
        ("\"between\"", IdentPos::ColumnOrTable, "between"), // col_name ok here
        ("\"between\"", IdentPos::TypeOrFunction, "\"between\""), // not here
        ("\"binary\"", IdentPos::TypeOrFunction, "binary"), // type_func ok here
        ("\"binary\"", IdentPos::ColumnOrTable, "\"binary\""), // not here
        ("\"a b\"", IdentPos::ColumnOrTable, "\"a b\""), // not identifier-shaped
        ("\"1st\"", IdentPos::ColumnOrTable, "\"1st\""), // digit start
    ];
    for (input, pos, expected) in cases {
        assert_eq!(render(&ident(input, pos), &opts), expected, "for {input:?}");
    }
}

#[test]
fn pg_always_quoted_folds_bare_names() {
    let opts = options(Dialect::Postgres, IdentQuoting::AlwaysQuoted);
    assert_eq!(
        render(&ident("foo", IdentPos::ColumnOrTable), &opts),
        "\"foo\""
    );
    // A bare MixedCase name denotes the folded name; quoting must fold.
    assert_eq!(
        render(&ident("MixedCase", IdentPos::ColumnOrTable), &opts),
        "\"mixedcase\""
    );
    // Already-quoted names are exact and stay untouched.
    assert_eq!(
        render(&ident("\"MixedCase\"", IdentPos::ColumnOrTable), &opts),
        "\"MixedCase\""
    );
}

#[test]
fn sqlite_strips_case_insensitively_and_normalizes_quotes() {
    let opts = options(Dialect::Sqlite, IdentQuoting::UnquotedWhenSafe);
    let cases = [
        ("\"Foo\"", "Foo"),           // case-insensitive: safe to strip
        ("`foo`", "foo"),             // backtick strip
        ("[foo]", "foo"),             // bracket strip
        ("`a b`", "\"a b\""),         // must stay quoted: normalize to "
        ("[a b]", "\"a b\""),         // must stay quoted: normalize to "
        ("\"order\"", "\"order\""),   // keyword stays, double quotes kept
        ("`order`", "\"order\""),     // keyword stays, normalized
        ("\"SELECT\"", "\"SELECT\""), // keyword (case-insensitive) stays
    ];
    for (input, expected) in cases {
        assert_eq!(
            render(&ident(input, IdentPos::ColumnOrTable), &opts),
            expected,
            "for {input:?}"
        );
    }
}
