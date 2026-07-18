//! Pratt expression parsing with the Postgres precedence table
//! (dialect-parameterized where SQLite differs).

use crate::dialect::Dialect;
use crate::syntax::SyntaxKind;

use super::grammar::{arg_list, at_subquery_start, ident, paren_expr_list, qualified_name};
use super::{PResult, Parser};

// Binding powers, mirroring Postgres's operator precedence (high binds
// tighter). Left-associative throughout: an operator binds when
// `bp > min_bp`, and its right operand is parsed with `min_bp = bp`.
const BP_CAST: u8 = 200; // ::
const BP_SUBSCRIPT: u8 = 190; // [ ]
const BP_UNARY: u8 = 170; // prefix + - and custom prefix operators
const BP_COLLATE: u8 = 160;
const BP_AT: u8 = 150; // AT TIME ZONE
const BP_EXP: u8 = 140; // ^
const BP_MUL: u8 = 130; // * / %
const BP_ADD: u8 = 120; // + -
const BP_OTHER: u8 = 110; // any other operator (||, @>, ->, ...)
const BP_RANGE: u8 = 100; // BETWEEN, IN, LIKE, ILIKE, SIMILAR
const BP_CMP: u8 = 90; // < > = <= >= <> !=
const BP_IS: u8 = 80;
const BP_NOT: u8 = 70; // prefix NOT
const BP_AND: u8 = 60;
const BP_OR: u8 = 50;

pub(crate) fn expr(p: &mut Parser<'_>, min_bp: u8) -> PResult {
    let checkpoint = p.checkpoint();
    prefix(p)?;
    infix_loop(p, checkpoint, min_bp)
}

fn infix_loop(p: &mut Parser<'_>, checkpoint: usize, min_bp: u8) -> PResult {
    loop {
        // Postfix: casts and subscripts.
        if p.at(SyntaxKind::ColonColon) && BP_CAST > min_bp {
            p.open_at(checkpoint, SyntaxKind::CastExpr);
            p.bump();
            type_name(p)?;
            p.finish();
            continue;
        }
        if p.at(SyntaxKind::LBracket) && BP_SUBSCRIPT > min_bp {
            p.open_at(checkpoint, SyntaxKind::SubscriptExpr);
            p.bump();
            if !p.at(SyntaxKind::Colon) && !p.at(SyntaxKind::RBracket) {
                expr(p, 0)?;
            }
            if p.eat(SyntaxKind::Colon) && !p.at(SyntaxKind::RBracket) {
                expr(p, 0)?;
            }
            p.expect(SyntaxKind::RBracket, "`]`")?;
            p.finish();
            continue;
        }

        // Operator tokens.
        if p.at(SyntaxKind::Operator) {
            let bp = operator_bp(p.text(), p.dialect());
            if bp > min_bp {
                p.open_at(checkpoint, SyntaxKind::BinaryExpr);
                p.bump();
                expr(p, bp)?;
                p.finish();
                continue;
            }
            break;
        }

        // Keyword operators.
        if p.at_kw("collate") && BP_COLLATE > min_bp {
            p.open_at(checkpoint, SyntaxKind::BinaryExpr);
            p.bump();
            qualified_name(p)?;
            p.finish();
            continue;
        }
        if p.at_kw("at") && BP_AT > min_bp {
            if p.nth_at_kw(1, "time") && p.nth_at_kw(2, "zone") {
                p.open_at(checkpoint, SyntaxKind::BinaryExpr);
                p.bump();
                p.bump();
                p.bump();
                expr(p, BP_AT)?;
                p.finish();
                continue;
            }
            if p.nth_at_kw(1, "local") {
                p.open_at(checkpoint, SyntaxKind::BinaryExpr);
                p.bump();
                p.bump();
                p.finish();
                continue;
            }
        }
        if p.at_kw("is") && BP_IS > min_bp {
            p.open_at(checkpoint, SyntaxKind::IsExpr);
            p.bump();
            p.eat_kw("not");
            if p.eat_kw("distinct") {
                p.expect_kw("from")?;
                expr(p, BP_IS)?;
            } else if !(p.eat_kw("null")
                || p.eat_kw("true")
                || p.eat_kw("false")
                || p.eat_kw("unknown"))
            {
                return Err(
                    p.error("expected `NULL`, `TRUE`, `FALSE`, `UNKNOWN`, or `DISTINCT FROM`")
                );
            }
            p.finish();
            continue;
        }
        if (p.at_kw("isnull") || p.at_kw("notnull")) && BP_IS > min_bp {
            p.open_at(checkpoint, SyntaxKind::IsExpr);
            p.bump();
            p.finish();
            continue;
        }
        if p.at_kw("and") && BP_AND > min_bp {
            p.open_at(checkpoint, SyntaxKind::BinaryExpr);
            p.bump();
            expr(p, BP_AND)?;
            p.finish();
            continue;
        }
        if p.at_kw("or") && BP_OR > min_bp {
            p.open_at(checkpoint, SyntaxKind::BinaryExpr);
            p.bump();
            expr(p, BP_OR)?;
            p.finish();
            continue;
        }

        // [NOT] BETWEEN / IN / LIKE / ILIKE / SIMILAR TO
        let (negated, range_kw) = if p.at_kw("not") {
            (true, 1)
        } else {
            (false, 0)
        };
        let is_range = ["between", "in", "like", "ilike", "similar"]
            .iter()
            .any(|kw| p.nth_at_kw(range_kw, kw));
        if is_range && BP_RANGE > min_bp {
            if p.nth_at_kw(range_kw, "between") {
                p.open_at(checkpoint, SyntaxKind::BetweenExpr);
                if negated {
                    p.bump();
                }
                p.bump(); // BETWEEN
                p.eat_kw("symmetric");
                expr(p, BP_RANGE)?;
                p.expect_kw("and")?;
                expr(p, BP_RANGE)?;
                p.finish();
            } else if p.nth_at_kw(range_kw, "in") {
                p.open_at(checkpoint, SyntaxKind::InExpr);
                if negated {
                    p.bump();
                }
                p.bump(); // IN
                if p.at(SyntaxKind::LParen) && at_subquery_start(p, 1) {
                    subquery(p)?;
                } else {
                    paren_expr_list(p)?;
                }
                p.finish();
            } else {
                p.open_at(checkpoint, SyntaxKind::BinaryExpr);
                if negated {
                    p.bump();
                }
                p.bump(); // LIKE | ILIKE | SIMILAR
                p.eat_kw("to");
                expr(p, BP_RANGE)?;
                if p.eat_kw("escape") {
                    expr(p, BP_RANGE)?;
                }
                p.finish();
            }
            continue;
        }

        break;
    }
    Ok(())
}

fn operator_bp(op: &str, _dialect: Dialect) -> u8 {
    match op {
        "^" => BP_EXP,
        "*" | "/" | "%" => BP_MUL,
        "+" | "-" => BP_ADD,
        "<" | ">" | "=" | "<=" | ">=" | "<>" | "!=" | "==" => BP_CMP,
        _ => BP_OTHER,
    }
}

fn prefix(p: &mut Parser<'_>) -> PResult {
    match p.kind() {
        Some(
            SyntaxKind::Number
            | SyntaxKind::String
            | SyntaxKind::EscapeString
            | SyntaxKind::UnicodeString
            | SyntaxKind::BitString
            | SyntaxKind::HexString
            | SyntaxKind::DollarString
            | SyntaxKind::Param,
        ) => {
            p.start(SyntaxKind::Literal);
            p.bump();
            p.finish();
            Ok(())
        }
        Some(SyntaxKind::Operator) => {
            if p.text() == "*" {
                return Err(p.error("expected an expression"));
            }
            p.start(SyntaxKind::PrefixExpr);
            p.bump();
            expr(p, BP_UNARY)?;
            p.finish();
            Ok(())
        }
        Some(SyntaxKind::LParen) => {
            if !at_subquery_start(p, 1) {
                paren_or_row(p)
            } else if p.nth_at(1, SyntaxKind::LParen) {
                // `((SELECT ...` is ambiguous: a nested subquery, or a
                // row/paren whose first element is a subquery. Try the
                // subquery reading and back off if it doesn't close.
                let state = p.state();
                if subquery(p).is_ok() {
                    Ok(())
                } else {
                    p.backtrack(state);
                    paren_or_row(p)
                }
            } else {
                subquery(p)
            }
        }
        Some(SyntaxKind::Ident) => ident_prefix(p),
        Some(SyntaxKind::QuotedIdent) => column_or_call(p),
        _ => Err(p.error("expected an expression")),
    }
}

/// `( query )` wrapped as a subquery expression.
fn subquery(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SubqueryExpr);
    p.expect(SyntaxKind::LParen, "`(`")?;
    super::grammar::query_body(p)?;
    p.expect(SyntaxKind::RParen, "`)`")?;
    p.finish();
    Ok(())
}

fn paren_or_row(p: &mut Parser<'_>) -> PResult {
    let checkpoint = p.checkpoint();
    p.start(SyntaxKind::ParenExpr);
    p.bump(); // (
    expr(p, 0)?;
    let mut is_row = false;
    while p.eat(SyntaxKind::Comma) {
        is_row = true;
        expr(p, 0)?;
    }
    p.expect(SyntaxKind::RParen, "`)`")?;
    p.finish();
    if is_row {
        // Rewrite the node kind: reopen as RowExpr.
        p.rewrite_start(checkpoint, SyntaxKind::RowExpr);
    }
    Ok(())
}

fn ident_prefix(p: &mut Parser<'_>) -> PResult {
    // Keyword-introduced expression forms first.
    if p.at_kw("case") {
        return case_expr(p);
    }
    if p.at_kw("cast") && p.nth_at(1, SyntaxKind::LParen) {
        p.start(SyntaxKind::CastExpr);
        p.bump();
        p.bump();
        expr(p, 0)?;
        p.expect_kw("as")?;
        type_name(p)?;
        p.expect(SyntaxKind::RParen, "`)`")?;
        p.finish();
        return Ok(());
    }
    if p.at_kw("exists") && p.nth_at(1, SyntaxKind::LParen) {
        p.start(SyntaxKind::SubqueryExpr);
        p.bump();
        p.bump();
        super::grammar::query_body(p)?;
        p.expect(SyntaxKind::RParen, "`)`")?;
        p.finish();
        return Ok(());
    }
    if p.at_kw("array") {
        return array_expr(p);
    }
    if p.at_kw("row") && p.nth_at(1, SyntaxKind::LParen) {
        p.start(SyntaxKind::RowExpr);
        p.bump();
        paren_expr_list(p)?;
        p.finish();
        return Ok(());
    }
    if p.at_kw("not") {
        p.start(SyntaxKind::PrefixExpr);
        p.bump();
        expr(p, BP_NOT)?;
        p.finish();
        return Ok(());
    }
    if (p.at_kw("any") || p.at_kw("some") || p.at_kw("all")) && p.nth_at(1, SyntaxKind::LParen) {
        p.start(SyntaxKind::QuantifiedExpr);
        p.bump();
        if at_subquery_start(p, 1) {
            subquery(p)?;
        } else {
            paren_expr_list(p)?;
        }
        p.finish();
        return Ok(());
    }
    if p.at_kw("true") || p.at_kw("false") || p.at_kw("null") || p.at_kw("default") {
        p.start(SyntaxKind::Literal);
        p.bump();
        p.finish();
        return Ok(());
    }
    // `interval '...'` / `date '...'` style typed literals.
    if matches!(
        p.nth_kind(1),
        Some(SyntaxKind::String | SyntaxKind::EscapeString | SyntaxKind::UnicodeString)
    ) {
        p.start(SyntaxKind::Literal);
        p.bump();
        p.bump();
        p.finish();
        return Ok(());
    }
    column_or_call(p)
}

/// A qualified name that is either a column reference or, when followed by
/// `(`, a function call with its clauses.
fn column_or_call(p: &mut Parser<'_>) -> PResult {
    let checkpoint = p.checkpoint();
    p.start(SyntaxKind::ColumnRef);
    qualified_name(p)?;
    p.finish();
    if !p.at(SyntaxKind::LParen) {
        return Ok(());
    }
    p.open_at(checkpoint, SyntaxKind::FunctionCall);
    arg_list(p)?;
    if p.at_kw("within") && p.nth_at_kw(1, "group") {
        p.start(SyntaxKind::WithinGroupClause);
        p.bump();
        p.bump();
        p.expect(SyntaxKind::LParen, "`(`")?;
        super::grammar::order_by_clause(p)?;
        p.expect(SyntaxKind::RParen, "`)`")?;
        p.finish();
    }
    if p.at_kw("filter") && p.nth_at(1, SyntaxKind::LParen) {
        p.start(SyntaxKind::FilterClause);
        p.bump();
        p.bump();
        p.expect_kw("where")?;
        expr(p, 0)?;
        p.expect(SyntaxKind::RParen, "`)`")?;
        p.finish();
    }
    if p.at_kw("over") {
        p.start(SyntaxKind::OverClause);
        p.bump();
        if p.at(SyntaxKind::LParen) {
            super::grammar::window_spec(p)?;
        } else {
            ident(p, "a window name")?;
        }
        p.finish();
    }
    p.finish();
    Ok(())
}

fn case_expr(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::CaseExpr);
    p.expect_kw("case")?;
    if !p.at_kw("when") {
        expr(p, 0)?;
    }
    while p.at_kw("when") {
        p.start(SyntaxKind::WhenClause);
        p.bump();
        expr(p, 0)?;
        p.expect_kw("then")?;
        expr(p, 0)?;
        p.finish();
    }
    if p.eat_kw("else") {
        expr(p, 0)?;
    }
    p.expect_kw("end")?;
    p.finish();
    Ok(())
}

fn array_expr(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::ArrayExpr);
    p.expect_kw("array")?;
    if p.at(SyntaxKind::LParen) {
        subquery(p)?;
    } else {
        p.expect(SyntaxKind::LBracket, "`[`")?;
        if !p.at(SyntaxKind::RBracket) {
            loop {
                expr(p, 0)?;
                if !p.eat(SyntaxKind::Comma) {
                    break;
                }
            }
        }
        p.expect(SyntaxKind::RBracket, "`]`")?;
    }
    p.finish();
    Ok(())
}

/// A type name: qualified name, multi-word suffixes (`double precision`,
/// `character varying`, `with/without time zone`), optional `(...)`
/// modifiers, and array suffixes.
pub(crate) fn type_name(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::TypeName);
    ident(p, "a type name")?;
    while p.at(SyntaxKind::Dot) {
        p.bump();
        ident(p, "a type name")?;
    }
    while p.at_kw("precision") || p.at_kw("varying") {
        p.bump();
    }
    if (p.at_kw("with") || p.at_kw("without")) && p.nth_at_kw(1, "time") && p.nth_at_kw(2, "zone") {
        p.bump();
        p.bump();
        p.bump();
    }
    if p.at(SyntaxKind::LParen) {
        paren_expr_list(p)?;
    }
    if p.at_kw("array") {
        p.bump();
    }
    while p.at(SyntaxKind::LBracket) {
        p.bump();
        p.eat(SyntaxKind::Number);
        p.expect(SyntaxKind::RBracket, "`]`")?;
    }
    p.finish();
    Ok(())
}
