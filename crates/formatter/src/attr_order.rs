//! Canonical attribute order for `CREATE FUNCTION` / `CREATE PROCEDURE`.
//!
//! Postgres accepts function attributes in any order; squill emits
//! pg_dump's: `returns`, then `language`, then the modifier soup, with
//! the `as` body last. The same permutation runs in two places — the
//! lowerer reorders the statement's CST elements, and the safety oracle
//! canonicalizes both token streams before comparing — so a reordered
//! output is still token-equivalent, and any disagreement between the
//! two views falls back to verbatim rather than emitting a reordering
//! the oracle cannot vouch for.

use parser::syntax::SyntaxKind;

/// One statement element, reduced to what segment detection needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum W {
    Open,
    Close,
    Dot,
    Comma,
    Semi,
    Trivia,
    /// A bare word (lowercased) or a quoted identifier (kept verbatim,
    /// quotes included, so it can never match an attribute keyword).
    Word(String),
    Other,
}

pub(crate) fn classify(kind: SyntaxKind, text: &str) -> W {
    if kind.is_trivia() {
        return W::Trivia;
    }
    match kind {
        SyntaxKind::LParen => W::Open,
        SyntaxKind::RParen => W::Close,
        SyntaxKind::Dot => W::Dot,
        SyntaxKind::Comma => W::Comma,
        SyntaxKind::Semicolon => W::Semi,
        SyntaxKind::Ident => W::Word(text.to_ascii_lowercase()),
        SyntaxKind::QuotedIdent => W::Word(text.to_string()),
        _ => W::Other,
    }
}

const CREATE_MODIFIERS: &[&str] = &["or", "replace", "if", "not", "exists"];

/// The canonical position of an attribute segment, or `None` for a word
/// that does not start one.
fn sort_key(word: &str, next: Option<&str>) -> Option<u8> {
    Some(match word {
        // RETURNS NULL ON NULL INPUT is strictness, not a result type.
        "returns" if next == Some("null") => 5,
        "returns" => 0,
        "language" => 1,
        "transform" => 2,
        "window" => 3,
        "immutable" | "stable" | "volatile" => 4,
        "called" | "strict" => 5,
        "not" if next == Some("leakproof") => 6,
        "leakproof" => 6,
        "external" | "security" => 7,
        "parallel" => 8,
        "cost" => 9,
        "rows" => 10,
        "support" => 11,
        "set" => 12,
        "with" => 13,
        "as" | "return" => 14,
        _ => return None,
    })
}

/// The next bare word at or after `i`, skipping trivia only.
fn word_at(words: &[W], mut i: usize) -> Option<(usize, &str)> {
    loop {
        match words.get(i)? {
            W::Trivia => i += 1,
            W::Word(text) => return Some((i, text.as_str())),
            _ => return None,
        }
    }
}

/// For a `CREATE FUNCTION`/`CREATE PROCEDURE` element sequence, the
/// permutation that puts its attribute segments in canonical order.
/// `None` when the statement is not one, is already canonical, or
/// cannot be reordered safely (`BEGIN ATOMIC` bodies hold top-level
/// semicolons).
pub(crate) fn canonical_order(words: &[W]) -> Option<Vec<usize>> {
    // Header: `create [or replace] function|procedure name[.name…]`.
    let (i, first) = word_at(words, 0)?;
    if first != "create" {
        return None;
    }
    let mut pos = i + 1;
    let object = loop {
        let (i, word) = word_at(words, pos)?;
        pos = i + 1;
        if !CREATE_MODIFIERS.contains(&word) {
            break word.to_string();
        }
    };
    if object != "function" && object != "procedure" {
        return None;
    }
    // The name: never an attribute starter, however it is spelled. In
    // the lowerer's element view, `f()` parses as a call *expression*,
    // so name and parens can arrive as one opaque node.
    let mut j = pos;
    while matches!(words.get(j), Some(W::Trivia)) {
        j += 1;
    }
    match words.get(j)? {
        W::Other => pos = j + 1,
        W::Word(_) => {
            pos = j + 1;
            // Qualified-name continuations: `.` then another name part.
            loop {
                let mut j = pos;
                while matches!(words.get(j), Some(W::Trivia)) {
                    j += 1;
                }
                if !matches!(words.get(j), Some(W::Dot)) {
                    break;
                }
                let (k, _) = word_at(words, j + 1)?;
                pos = k + 1;
            }
        }
        _ => return None,
    }

    // Tail scan: segment starts at each attribute keyword at paren
    // depth 0. Clauses that grammatically take an argument word have it
    // force-consumed, so a type or name that collides with an attribute
    // keyword (`returns setof language`) cannot start a bogus segment;
    // a keyword right after `,` or `.` is list/name continuation
    // (`set search_path = a, cost`), not a segment start.
    let mut depth = 0usize;
    let mut starts: Vec<(u8, usize)> = Vec::new();
    let mut end = words.len();
    let mut skip_words = 0usize;
    let mut after_separator = false;
    let mut i = pos;
    while i < words.len() {
        match &words[i] {
            W::Trivia => {
                i += 1;
                continue;
            }
            W::Open => depth += 1,
            W::Close => depth = depth.saturating_sub(1),
            W::Semi if depth == 0 => {
                end = i;
                break;
            }
            W::Dot | W::Comma => {
                after_separator = true;
                i += 1;
                continue;
            }
            W::Word(word) if depth == 0 => {
                if skip_words > 0 {
                    skip_words -= 1;
                } else if !after_separator {
                    let next = word_at(words, i + 1).map(|(_, w)| w);
                    if word == "begin" && next == Some("atomic") {
                        return None;
                    }
                    if let Some(key) = sort_key(word, next) {
                        starts.push((key, i));
                        skip_words = match word.as_str() {
                            "returns" if next == Some("setof") => 2,
                            "returns" => 1,
                            "language" | "security" | "external" | "parallel" | "support" => 1,
                            _ => 0,
                        };
                    }
                }
            }
            _ => {}
        }
        after_separator = false;
        i += 1;
    }
    if starts.len() < 2 {
        return None;
    }

    // Segment ranges, stably sorted into canonical order.
    let mut ranges: Vec<(u8, std::ops::Range<usize>)> = Vec::with_capacity(starts.len());
    for (index, &(key, start)) in starts.iter().enumerate() {
        let stop = starts.get(index + 1).map_or(end, |&(_, next)| next);
        ranges.push((key, start..stop));
    }
    let mut sorted = ranges.clone();
    sorted.sort_by_key(|&(key, _)| key);
    if sorted == ranges {
        return None;
    }
    let mut perm: Vec<usize> = (0..starts[0].1).collect();
    for (_, range) in &sorted {
        perm.extend(range.clone());
    }
    perm.extend(end..words.len());
    Some(perm)
}

#[cfg(test)]
mod tests {
    use super::*;
    use parser::Dialect;
    use parser::lexer::{LexOptions, lex_with};

    /// Reordered non-trivia token texts, space-joined (spacing is the
    /// printer's job; only the order matters here).
    fn order(sql: &str) -> Option<String> {
        let tokens: Vec<_> = lex_with(sql, Dialect::Postgres, LexOptions::default())
            .into_iter()
            .filter(|t| !t.kind.is_trivia())
            .collect();
        let words: Vec<W> = tokens.iter().map(|t| classify(t.kind, t.text)).collect();
        let perm = canonical_order(&words)?;
        let out: Vec<&str> = perm.iter().map(|&i| tokens[i].text).collect();
        Some(out.join(" "))
    }

    #[test]
    fn language_moves_before_as() {
        assert_eq!(
            order("create function f() as $$select 1$$ language sql;"),
            Some("create function f ( ) language sql as $$select 1$$ ;".into())
        );
    }

    #[test]
    fn full_soup_normalizes() {
        assert_eq!(
            order(
                "create or replace function s.f(a int) language plpgsql \
                 security definer as $$begin end$$ immutable returns trigger;"
            ),
            Some(
                "create or replace function s . f ( a int ) returns trigger \
                 language plpgsql immutable security definer as $$begin end$$ ;"
                    .into()
            )
        );
    }

    #[test]
    fn canonical_input_is_identity() {
        assert_eq!(
            order("create function f() returns int language sql as $$select 1$$;"),
            None
        );
    }

    #[test]
    fn type_colliding_with_keyword_is_not_a_segment() {
        // `language` here is the return type's name, then the real
        // language clause; the type must stay glued to `returns`.
        assert_eq!(
            order("create function f() as $$x$$ returns setof language language sql;"),
            Some("create function f ( ) returns setof language language sql as $$x$$ ;".into())
        );
    }

    #[test]
    fn set_values_do_not_start_segments() {
        // `cost` after the comma is a schema in the search_path list.
        assert_eq!(
            order("create function f() as $$x$$ set search_path = a, cost language sql;"),
            Some("create function f ( ) language sql set search_path = a , cost as $$x$$ ;".into())
        );
    }

    #[test]
    fn begin_atomic_bails() {
        assert_eq!(
            order("create function f() return 1 language sql;"),
            Some("create function f ( ) language sql return 1 ;".into())
        );
        assert_eq!(
            order("create function f() language sql begin atomic select 1; end;"),
            None
        );
    }

    #[test]
    fn non_functions_are_untouched() {
        assert_eq!(order("create table t (a int);"), None);
        assert_eq!(order("select 1;"), None);
    }
}
