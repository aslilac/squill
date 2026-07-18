//! DDL and utility statements: structured-but-tolerant.
//!
//! One `DdlStmt` node covers the CREATE/ALTER/DROP families plus utility
//! commands (SET, LOCK, BEGIN, COMMENT ON, GRANT, ...). The parser keeps
//! keyword soup as flat tokens but recognizes structure where it matters
//! for formatting and safety:
//!
//! - Parenthesized lists become `ElementList` nodes; each element is a
//!   real expression when it parses as one (index expressions, CHECK
//!   bodies) or a tolerant `ColumnDef` otherwise (column definitions,
//!   table constraints).
//! - `DEFAULT <expr>`, `USING <expr>`, `EXECUTE FUNCTION f(...)` parse
//!   real expressions, so function calls format tightly.
//! - `AS SELECT ...` (views, CREATE TABLE AS) parses a real query body.
//! - `WHERE <expr>` (partial indexes) becomes a real `WhereClause`.
//! - Dollar-quoted bodies (`CREATE FUNCTION ... AS $$...$$`, `DO $$...$$`)
//!   are single opaque tokens: verbatim passthrough by construction.
//!
//! Statement keywords the dispatcher routes here are in
//! [`DDL_STARTERS`].

use crate::syntax::SyntaxKind;

use super::expr::expr;
use super::grammar::{query_body, where_clause};
use super::{PResult, Parser};

/// First keywords that route a statement to the DDL/utility parser.
pub(crate) const DDL_STARTERS: &[&str] = &[
    "create",
    "alter",
    "drop",
    "comment",
    "grant",
    "revoke",
    "set",
    "reset",
    "lock",
    "begin",
    "commit",
    "end",
    "rollback",
    "abort",
    "savepoint",
    "release",
    "truncate",
    "do",
    "vacuum",
    "analyze",
    "analyse",
    "reindex",
    "cluster",
    "refresh",
    "notify",
    "listen",
    "unlisten",
    "discard",
    "checkpoint",
    "prepare",
    "deallocate",
    "explain",
    "call",
    "merge",
    "copy",
    "import",
    "pragma",
    "attach",
    "detach",
];

pub(crate) fn ddl_stmt(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::DdlStmt);
    ddl_tokens(p, Stop::Semicolon)?;
    p.finish();
    Ok(())
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Stop {
    /// Top level: consume through the terminating `;` (or EOF).
    Semicolon,
    /// Inside parens: stop before `,` or `)`.
    CommaOrParen,
}

fn ddl_tokens(p: &mut Parser<'_>, stop: Stop) -> PResult {
    p.enter_depth()?;
    let result = ddl_tokens_inner(p, stop);
    p.exit_depth();
    result
}

fn ddl_tokens_inner(p: &mut Parser<'_>, stop: Stop) -> PResult {
    // SQLite `CREATE TRIGGER ... BEGIN stmt; stmt; END;` bodies contain
    // semicolons that do not terminate the statement.
    let mut begin_depth = 0u32;
    loop {
        if p.dialect() == crate::dialect::Dialect::Sqlite && stop == Stop::Semicolon {
            if p.at_kw("begin") && !p.begin_is_transaction() {
                begin_depth += 1;
            } else if p.at_kw("end") {
                begin_depth = begin_depth.saturating_sub(1);
            }
        }
        match p.kind() {
            None => {
                return if stop == Stop::CommaOrParen {
                    Err(p.error("expected `)`"))
                } else {
                    Ok(())
                };
            }
            Some(SyntaxKind::Semicolon) if begin_depth > 0 => p.bump(),
            Some(SyntaxKind::Semicolon) => {
                return if stop == Stop::CommaOrParen {
                    Err(p.error("unexpected `;` inside `(...)`"))
                } else {
                    p.bump();
                    Ok(())
                };
            }
            Some(SyntaxKind::RParen | SyntaxKind::Comma) if stop == Stop::CommaOrParen => {
                return Ok(());
            }
            Some(SyntaxKind::RParen) => {
                return Err(p.error("unexpected `)`"));
            }
            Some(SyntaxKind::LParen) => element_list(p)?,
            Some(SyntaxKind::Ident) => {
                let word = p.text().to_ascii_lowercase();
                match word.as_str() {
                    "where" if stop == Stop::Semicolon => where_clause(p)?,
                    "as" => {
                        p.bump();
                        // Views / CREATE TABLE AS: a real query follows.
                        if p.at_any_kw(&["select", "with", "values"]) {
                            query_body(p)?;
                        }
                    }
                    "default" => {
                        p.bump();
                        if can_start_expr(p) && !p.at_kw("values") {
                            expr(p, 0)?;
                        }
                    }
                    "using" | "function" | "procedure" | "when" => {
                        p.bump();
                        // Try an expression (`USING gin (col)`, `EXECUTE
                        // FUNCTION f()`, trigger `WHEN (cond)`); back off
                        // to keyword soup if it does not parse.
                        if can_start_expr(p) {
                            let state = p.state();
                            if expr(p, 0).is_err() {
                                p.backtrack(state);
                            }
                        }
                    }
                    _ => p.bump(),
                }
            }
            Some(_) => p.bump(),
        }
    }
}

/// `( element, element, ... )` — each element an expression when it fully
/// parses as one, a tolerant `ColumnDef` otherwise.
fn element_list(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::ElementList);
    p.expect(SyntaxKind::LParen, "`(`")?;
    if !p.at(SyntaxKind::RParen) {
        loop {
            element(p)?;
            if !p.eat(SyntaxKind::Comma) {
                break;
            }
        }
    }
    p.expect(SyntaxKind::RParen, "`)`")?;
    p.finish();
    Ok(())
}

fn element(p: &mut Parser<'_>) -> PResult {
    let state = p.state();
    if expr(p, 0).is_ok() && (p.at(SyntaxKind::Comma) || p.at(SyntaxKind::RParen)) {
        return Ok(());
    }
    p.backtrack(state);
    p.start(SyntaxKind::ColumnDef);
    ddl_tokens(p, Stop::CommaOrParen)?;
    p.finish();
    Ok(())
}

fn can_start_expr(p: &Parser<'_>) -> bool {
    !matches!(
        p.kind(),
        None | Some(
            SyntaxKind::Comma | SyntaxKind::Semicolon | SyntaxKind::RParen | SyntaxKind::RBracket
        )
    )
}
