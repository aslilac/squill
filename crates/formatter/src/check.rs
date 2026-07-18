//! The safety oracle's primitives: token equivalence and comment
//! conservation between input and formatted output.
//!
//! Used both by the per-statement self-check inside [`crate::format_cst`]
//! (fall back to verbatim if a statement would change meaning) and by the
//! corpus-wide oracle tests in CI.

use parser::Dialect;
use parser::lexer::{LexOptions, lex_with};
use parser::syntax::SyntaxKind;

/// Are the two sources token-identical modulo trivia, keyword case, and
/// sanctioned identifier-quote changes?
pub fn tokens_equivalent(
    input: &str,
    output: &str,
    dialect: Dialect,
    lex_options: LexOptions,
) -> bool {
    let a = lex_with(input, dialect, lex_options);
    let b = lex_with(output, dialect, lex_options);
    let a: Vec<_> = a.iter().filter(|t| !t.kind.is_trivia()).collect();
    let b: Vec<_> = b.iter().filter(|t| !t.kind.is_trivia()).collect();
    a.len() == b.len()
        && a.iter()
            .zip(&b)
            .all(|(x, y)| token_equivalent(x.kind, x.text, y.kind, y.text, dialect))
}

fn token_equivalent(
    kind_a: SyntaxKind,
    text_a: &str,
    kind_b: SyntaxKind,
    text_b: &str,
    dialect: Dialect,
) -> bool {
    use SyntaxKind::*;
    match (kind_a, kind_b) {
        // Bare word vs bare word: keyword casing is sanctioned, and bare
        // identifiers fold case-insensitively in both dialects anyway.
        (Ident, Ident) => text_a.eq_ignore_ascii_case(text_b),
        // Sanctioned quote changes: both sides must resolve to the same
        // identifier under the dialect's folding rules.
        (Ident, QuotedIdent) | (QuotedIdent, Ident) | (QuotedIdent, QuotedIdent) => {
            resolve_ident(text_a, dialect) == resolve_ident(text_b, dialect)
        }
        // Everything else must match exactly.
        (a, b) => a == b && text_a == text_b,
    }
}

/// The name an identifier token denotes, canonicalized for comparison.
fn resolve_ident(token: &str, dialect: Dialect) -> String {
    let (inner, quoted) = if let Some(body) = token.strip_prefix('"') {
        (
            body.strip_suffix('"').unwrap_or(body).replace("\"\"", "\""),
            true,
        )
    } else if let Some(body) = token.strip_prefix('`') {
        (
            body.strip_suffix('`').unwrap_or(body).replace("``", "`"),
            true,
        )
    } else if let Some(body) = token.strip_prefix('[') {
        (body.strip_suffix(']').unwrap_or(body).to_string(), true)
    } else {
        (token.to_string(), false)
    };
    match dialect {
        // Postgres: quoted names are exact, bare names fold to lowercase.
        Dialect::Postgres => {
            if quoted {
                inner
            } else {
                inner.to_ascii_lowercase()
            }
        }
        // SQLite compares everything ASCII-case-insensitively.
        Dialect::Sqlite => inner.to_ascii_lowercase(),
    }
}

/// The comment texts of `source`, in order. Trailing whitespace inside a
/// comment is not significant (the formatter never emits trailing
/// whitespace), so it is trimmed for comparison.
pub fn comment_texts(source: &str, dialect: Dialect, lex_options: LexOptions) -> Vec<String> {
    lex_with(source, dialect, lex_options)
        .iter()
        .filter(|t| matches!(t.kind, SyntaxKind::LineComment | SyntaxKind::BlockComment))
        .map(|t| t.text.trim_end().to_string())
        .collect()
}

/// Comment conservation: same comments, same order, none dropped or
/// duplicated.
pub fn comments_conserved(
    input: &str,
    output: &str,
    dialect: Dialect,
    lex_options: LexOptions,
) -> bool {
    comment_texts(input, dialect, lex_options) == comment_texts(output, dialect, lex_options)
}
