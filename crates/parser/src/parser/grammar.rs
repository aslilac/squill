//! Statement and clause grammar for queries (SELECT and friends).

use crate::syntax::SyntaxKind;

use super::expr::expr;
use super::{PResult, Parser};

/// Keywords that end an expression in a select-item position, so a bare
/// identifier after an expression can be taken as an alias.
const SELECT_ITEM_STOP: &[&str] = &[
    "from",
    "where",
    "group",
    "having",
    "window",
    "order",
    "limit",
    "offset",
    "fetch",
    "for",
    "union",
    "intersect",
    "except",
    "into",
    "returning",
    // INSERT ... SELECT tails.
    "on",
    "do",
];

/// Keywords that stop a bare table alias in FROM position.
const TABLE_ALIAS_STOP: &[&str] = &[
    "on",
    "using",
    "join",
    "inner",
    "left",
    "right",
    "full",
    "cross",
    "natural",
    "lateral",
    "where",
    "group",
    "having",
    "window",
    "order",
    "limit",
    "offset",
    "fetch",
    "for",
    "union",
    "intersect",
    "except",
    "with",
    "returning",
];

/// A full query statement: `[WITH ...] query [trailing clauses] [;]`.
pub(crate) fn select_stmt(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SelectStmt);
    query_body(p)?;
    if !p.at_eof() {
        p.expect(SyntaxKind::Semicolon, "`;` or end of statement")?;
    }
    p.finish();
    Ok(())
}

/// `[WITH ...] set-op-expr [ORDER BY] [LIMIT/OFFSET/FETCH] [FOR ...]` —
/// the reusable query body, also used inside parentheses.
pub(crate) fn query_body(p: &mut Parser<'_>) -> PResult {
    if p.at_kw("with") {
        with_clause(p)?;
    }
    query_tail(p)
}

/// A query body after any WITH clause has been consumed.
pub(crate) fn query_tail(p: &mut Parser<'_>) -> PResult {
    query_expr(p, 0)?;
    trailing_clauses(p)?;
    Ok(())
}

pub(crate) fn with_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::WithClause);
    p.expect_kw("with")?;
    p.eat_kw("recursive");
    loop {
        cte(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

fn cte(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::Cte);
    ident(p, "CTE name")?;
    if p.at(SyntaxKind::LParen) {
        paren_name_list(p)?;
    }
    p.expect_kw("as")?;
    if p.at_kw("not") {
        p.bump();
        p.expect_kw("materialized")?;
    } else {
        p.eat_kw("materialized");
    }
    p.expect(SyntaxKind::LParen, "`(`")?;
    if p.at_any_kw(&["insert", "update", "delete"]) {
        // Data-modifying CTE body.
        super::dml::dml_in_parens(p)?;
    } else {
        query_body(p)?;
    }
    p.expect(SyntaxKind::RParen, "`)`")?;
    if p.at_kw("search") {
        search_clause(p)?;
    }
    if p.at_kw("cycle") {
        cycle_clause(p)?;
    }
    p.finish();
    Ok(())
}

fn search_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SearchClause);
    p.expect_kw("search")?;
    if !p.eat_kw("breadth") && !p.eat_kw("depth") {
        return Err(p.error("expected `BREADTH` or `DEPTH`"));
    }
    p.expect_kws(&["first", "by"])?;
    name_list(p)?;
    p.expect_kws(&["set"])?;
    ident(p, "column name")?;
    p.finish();
    Ok(())
}

fn cycle_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::CycleClause);
    p.expect_kw("cycle")?;
    name_list(p)?;
    p.expect_kw("set")?;
    ident(p, "column name")?;
    if p.eat_kw("to") {
        expr(p, 0)?;
        p.expect_kw("default")?;
        expr(p, 0)?;
    }
    p.expect_kw("using")?;
    ident(p, "column name")?;
    p.finish();
    Ok(())
}

/// Set operations with precedence: INTERSECT binds tighter than
/// UNION/EXCEPT; all left-associative.
fn query_expr(p: &mut Parser<'_>, min_bp: u8) -> PResult {
    let checkpoint = p.checkpoint();
    query_primary(p)?;
    loop {
        let bp = if p.at_kw("intersect") {
            2
        } else if p.at_kw("union") || p.at_kw("except") {
            1
        } else {
            break;
        };
        if bp <= min_bp {
            break;
        }
        p.open_at(checkpoint, SyntaxKind::SetOperation);
        p.bump(); // UNION | INTERSECT | EXCEPT
        if !p.eat_kw("all") {
            p.eat_kw("distinct");
        }
        query_expr(p, bp)?;
        p.finish();
    }
    Ok(())
}

fn query_primary(p: &mut Parser<'_>) -> PResult {
    p.enter_depth()?;
    let result = query_primary_inner(p);
    p.exit_depth();
    result
}

fn query_primary_inner(p: &mut Parser<'_>) -> PResult {
    if p.at(SyntaxKind::LParen) {
        p.start(SyntaxKind::ParenSelect);
        p.bump();
        query_body(p)?;
        p.expect(SyntaxKind::RParen, "`)`")?;
        p.finish();
        Ok(())
    } else if p.at_kw("select") {
        select_core(p)
    } else if p.at_kw("values") {
        values_clause(p)
    } else if p.at_kw("table") {
        p.start(SyntaxKind::TableCore);
        p.bump();
        qualified_name(p)?;
        p.finish();
        Ok(())
    } else {
        Err(p.error("expected `SELECT`, `VALUES`, `TABLE`, or `(`"))
    }
}

fn select_core(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SelectCore);
    p.expect_kw("select")?;
    if p.at_kw("distinct") {
        p.bump();
        if p.at_kw("on") {
            p.bump();
            paren_expr_list(p)?;
        }
    } else {
        p.eat_kw("all");
    }
    select_list(p)?;
    if p.at_kw("from") {
        from_clause(p)?;
    }
    if p.at_kw("where") {
        where_clause(p)?;
    }
    if p.at_kw("group") {
        group_by_clause(p)?;
    }
    if p.at_kw("having") {
        p.start(SyntaxKind::HavingClause);
        p.bump();
        expr(p, 0)?;
        p.finish();
    }
    if p.at_kw("window") {
        window_clause(p)?;
    }
    p.finish();
    Ok(())
}

pub(crate) fn select_list(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SelectList);
    loop {
        select_item(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

fn select_item(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SelectItem);
    if p.at_op("*") {
        p.bump();
    } else {
        expr(p, 0)?;
        if p.eat_kw("as") || at_bare_alias(p, SELECT_ITEM_STOP) {
            alias_name(p)?;
        }
    }
    p.finish();
    Ok(())
}

pub(crate) fn where_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::WhereClause);
    p.expect_kw("where")?;
    expr(p, 0)?;
    p.finish();
    Ok(())
}

/// A bare (no `AS`) alias: an identifier that is not a clause keyword.
pub(crate) fn at_bare_alias(p: &Parser<'_>, stop: &[&str]) -> bool {
    (p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent)) && !p.at_any_kw(stop)
}

pub(crate) fn alias_name(p: &mut Parser<'_>) -> PResult {
    if p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent) {
        p.bump();
        Ok(())
    } else {
        Err(p.error("expected an alias name"))
    }
}

pub(crate) fn from_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::FromClause);
    p.expect_kw("from")?;
    loop {
        from_item(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

/// One FROM element: a primary table reference plus any number of joins,
/// built left-associatively as nested `JoinExpr` nodes.
pub(crate) fn from_item(p: &mut Parser<'_>) -> PResult {
    let checkpoint = p.checkpoint();
    table_primary(p)?;
    while at_join_start(p) {
        p.open_at(checkpoint, SyntaxKind::JoinExpr);
        join_keywords(p)?;
        table_primary(p)?;
        if p.at_kw("on") {
            p.start(SyntaxKind::JoinCondition);
            p.bump();
            expr(p, 0)?;
            p.finish();
        } else if p.at_kw("using") {
            p.start(SyntaxKind::JoinCondition);
            p.bump();
            paren_name_list(p)?;
            if p.eat_kw("as") {
                alias_name(p)?;
            }
            p.finish();
        }
        p.finish();
    }
    Ok(())
}

fn at_join_start(p: &Parser<'_>) -> bool {
    p.at_any_kw(&["join", "inner", "left", "right", "full", "cross", "natural"])
}

fn join_keywords(p: &mut Parser<'_>) -> PResult {
    p.eat_kw("natural");
    if p.eat_kw("cross") {
        p.expect_kw("join")?;
        return Ok(());
    }
    if p.eat_kw("inner") {
        p.expect_kw("join")?;
        return Ok(());
    }
    if p.eat_kw("left") || p.eat_kw("right") || p.eat_kw("full") {
        p.eat_kw("outer");
        p.expect_kw("join")?;
        return Ok(());
    }
    p.expect_kw("join")
}

fn table_primary(p: &mut Parser<'_>) -> PResult {
    p.enter_depth()?;
    let result = table_primary_inner(p);
    p.exit_depth();
    result
}

fn table_primary_inner(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::TableRef);
    p.eat_kw("lateral");
    if p.at(SyntaxKind::LParen) {
        let paren_join = |p: &mut Parser<'_>| -> PResult {
            // Parenthesized join: `(a join b on ...)`.
            p.start(SyntaxKind::ParenTableRef);
            p.bump();
            from_item(p)?;
            p.expect(SyntaxKind::RParen, "`)`")?;
            p.finish();
            Ok(())
        };
        let subquery = |p: &mut Parser<'_>| -> PResult {
            p.start(SyntaxKind::SubqueryExpr);
            p.bump();
            query_body(p)?;
            p.expect(SyntaxKind::RParen, "`)`")?;
            p.finish();
            Ok(())
        };
        if !at_subquery_start(p, 1) {
            paren_join(p)?;
        } else if p.nth_at(1, SyntaxKind::LParen) {
            // `((SELECT ...` may be a parenthesized set-op subquery or a
            // paren join whose first table is a subquery; try, back off.
            let state = p.state();
            if subquery(p).is_err() {
                p.backtrack(state);
                paren_join(p)?;
            }
        } else {
            subquery(p)?;
        }
    } else {
        p.eat_kw("only");
        qualified_name(p)?;
        if p.at(SyntaxKind::LParen) {
            // Table function call: name(args).
            arg_list(p)?;
            if p.at_kw("with") && p.nth_at_kw(1, "ordinality") {
                p.bump();
                p.bump();
            }
        } else {
            p.eat_op_star();
        }
    }
    if p.eat_kw("as") || at_bare_alias(p, TABLE_ALIAS_STOP) {
        alias_name(p)?;
        if p.at(SyntaxKind::LParen) {
            paren_name_list(p)?;
        }
    }
    p.finish();
    Ok(())
}

/// Does a `(` at offset `n` open a subquery (`SELECT`/`WITH`/`VALUES`/
/// `TABLE`), looking through further parens?
pub(crate) fn at_subquery_start(p: &Parser<'_>, mut n: usize) -> bool {
    while p.nth_at(n, SyntaxKind::LParen) {
        n += 1;
    }
    p.nth_at_kw(n, "select")
        || p.nth_at_kw(n, "with")
        || p.nth_at_kw(n, "values")
        || p.nth_at_kw(n, "table")
}

fn group_by_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::GroupByClause);
    p.expect_kws(&["group", "by"])?;
    if !p.eat_kw("all") {
        p.eat_kw("distinct");
    }
    loop {
        grouping_element(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

fn grouping_element(p: &mut Parser<'_>) -> PResult {
    if p.at_kw("rollup") || p.at_kw("cube") {
        p.start(SyntaxKind::GroupingElement);
        p.bump();
        paren_expr_list(p)?;
        p.finish();
        Ok(())
    } else if p.at_kw("grouping") && p.nth_at_kw(1, "sets") {
        p.start(SyntaxKind::GroupingElement);
        p.bump();
        p.bump();
        p.expect(SyntaxKind::LParen, "`(`")?;
        loop {
            grouping_element(p)?;
            if !p.eat(SyntaxKind::Comma) {
                break;
            }
        }
        p.expect(SyntaxKind::RParen, "`)`")?;
        p.finish();
        Ok(())
    } else if p.at(SyntaxKind::LParen) && p.nth_at(1, SyntaxKind::RParen) {
        // Empty grouping set `()`.
        p.start(SyntaxKind::GroupingElement);
        p.bump();
        p.bump();
        p.finish();
        Ok(())
    } else {
        expr(p, 0)
    }
}

fn window_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::WindowClause);
    p.expect_kw("window")?;
    loop {
        p.start(SyntaxKind::WindowDef);
        ident(p, "window name")?;
        p.expect_kw("as")?;
        window_spec(p)?;
        p.finish();
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

/// `( [existing_window] [PARTITION BY ...] [ORDER BY ...] [frame] )`
pub(crate) fn window_spec(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::WindowSpec);
    p.expect(SyntaxKind::LParen, "`(`")?;
    if (p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent))
        && !p.at_any_kw(&["partition", "order", "rows", "range", "groups"])
    {
        p.bump();
    }
    if p.at_kw("partition") {
        p.bump();
        p.expect_kw("by")?;
        loop {
            expr(p, 0)?;
            if !p.eat(SyntaxKind::Comma) {
                break;
            }
        }
    }
    if p.at_kw("order") {
        order_by_clause(p)?;
    }
    if p.at_any_kw(&["rows", "range", "groups"]) {
        frame_clause(p)?;
    }
    p.expect(SyntaxKind::RParen, "`)`")?;
    p.finish();
    Ok(())
}

fn frame_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::FrameClause);
    p.bump(); // rows | range | groups
    if p.eat_kw("between") {
        frame_bound(p)?;
        p.expect_kw("and")?;
        frame_bound(p)?;
    } else {
        frame_bound(p)?;
    }
    if p.eat_kw("exclude") {
        if p.eat_kw("current") {
            p.expect_kw("row")?;
        } else if p.eat_kw("no") {
            p.expect_kw("others")?;
        } else if !p.eat_kw("group") && !p.eat_kw("ties") {
            return Err(p.error("expected frame exclusion"));
        }
    }
    p.finish();
    Ok(())
}

fn frame_bound(p: &mut Parser<'_>) -> PResult {
    if p.eat_kw("unbounded") {
        if !p.eat_kw("preceding") && !p.eat_kw("following") {
            return Err(p.error("expected `PRECEDING` or `FOLLOWING`"));
        }
        Ok(())
    } else if p.eat_kw("current") {
        p.expect_kw("row")
    } else {
        expr(p, 0)?;
        if !p.eat_kw("preceding") && !p.eat_kw("following") {
            return Err(p.error("expected `PRECEDING` or `FOLLOWING`"));
        }
        Ok(())
    }
}

pub(crate) fn order_by_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::OrderByClause);
    p.expect_kws(&["order", "by"])?;
    loop {
        p.start(SyntaxKind::OrderingTerm);
        expr(p, 0)?;
        if p.eat_kw("using") {
            if !p.at(SyntaxKind::Operator) {
                return Err(p.error("expected an operator after `USING`"));
            }
            p.bump();
        } else if !p.eat_kw("asc") {
            p.eat_kw("desc");
        }
        if p.eat_kw("nulls") && !p.eat_kw("first") && !p.eat_kw("last") {
            return Err(p.error("expected `FIRST` or `LAST`"));
        }
        p.finish();
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

fn values_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::ValuesClause);
    p.expect_kw("values")?;
    loop {
        paren_expr_list(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

fn trailing_clauses(p: &mut Parser<'_>) -> PResult {
    if p.at_kw("order") {
        order_by_clause(p)?;
    }
    // Postgres accepts LIMIT and OFFSET in either order.
    loop {
        if p.at_kw("limit") {
            p.start(SyntaxKind::LimitClause);
            p.bump();
            if !p.eat_kw("all") {
                expr(p, 0)?;
            }
            p.finish();
        } else if p.at_kw("offset") {
            p.start(SyntaxKind::OffsetClause);
            p.bump();
            expr(p, 0)?;
            if !p.eat_kw("rows") {
                p.eat_kw("row");
            }
            p.finish();
        } else if p.at_kw("fetch") {
            fetch_clause(p)?;
        } else if p.at_kw("for") {
            locking_clause(p)?;
        } else {
            break;
        }
    }
    Ok(())
}

fn fetch_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::FetchClause);
    p.expect_kw("fetch")?;
    if !p.eat_kw("first") && !p.eat_kw("next") {
        return Err(p.error("expected `FIRST` or `NEXT`"));
    }
    if !p.at_kw("row") && !p.at_kw("rows") {
        expr(p, 0)?;
    }
    if !p.eat_kw("rows") && !p.eat_kw("row") {
        return Err(p.error("expected `ROW` or `ROWS`"));
    }
    if p.eat_kw("with") {
        p.expect_kw("ties")?;
    } else {
        p.expect_kw("only")?;
    }
    p.finish();
    Ok(())
}

fn locking_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::LockingClause);
    p.expect_kw("for")?;
    if p.eat_kw("update") {
    } else if p.eat_kw("no") {
        p.expect_kws(&["key", "update"])?;
    } else if p.eat_kw("key") {
        p.expect_kw("share")?;
    } else if !p.eat_kw("share") {
        return Err(p.error("expected a lock strength"));
    }
    if p.eat_kw("of") {
        name_list(p)?;
    }
    if p.eat_kw("skip") {
        p.expect_kw("locked")?;
    } else {
        p.eat_kw("nowait");
    }
    p.finish();
    Ok(())
}

// ---- small shared pieces ----

pub(crate) fn ident(p: &mut Parser<'_>, what: &str) -> PResult {
    if p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent) {
        p.bump();
        Ok(())
    } else {
        Err(p.error(&format!("expected {what}")))
    }
}

/// `name[.name[.name]]`, with a possible trailing `.*`.
pub(crate) fn qualified_name(p: &mut Parser<'_>) -> PResult {
    ident(p, "a name")?;
    while p.at(SyntaxKind::Dot) {
        p.bump();
        if p.at_op("*") {
            p.bump();
            break;
        }
        ident(p, "a name")?;
    }
    Ok(())
}

fn name_list(p: &mut Parser<'_>) -> PResult {
    loop {
        qualified_name(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    Ok(())
}

pub(crate) fn paren_name_list(p: &mut Parser<'_>) -> PResult {
    p.expect(SyntaxKind::LParen, "`(`")?;
    loop {
        ident(p, "a name")?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.expect(SyntaxKind::RParen, "`)`")
}

pub(crate) fn paren_expr_list(p: &mut Parser<'_>) -> PResult {
    p.expect(SyntaxKind::LParen, "`(`")?;
    if !p.at(SyntaxKind::RParen) {
        loop {
            expr(p, 0)?;
            if !p.eat(SyntaxKind::Comma) {
                break;
            }
        }
    }
    p.expect(SyntaxKind::RParen, "`)`")
}

/// Function-call style argument list; also used for table functions.
pub(crate) fn arg_list(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::ArgList);
    p.expect(SyntaxKind::LParen, "`(`")?;
    if !p.at(SyntaxKind::RParen) {
        if !p.eat_kw("distinct") {
            p.eat_kw("all");
        }
        p.eat_kw("variadic");
        loop {
            if p.at_op("*") {
                p.bump();
            } else {
                expr(p, 0)?;
            }
            // Tolerate the keyword-separated special forms: EXTRACT(x FROM
            // y), SUBSTRING(x FROM y FOR z), POSITION(a IN b),
            // TRIM(BOTH x FROM y), OVERLAY(a PLACING b FROM c FOR d).
            if p.at_any_kw(&[
                "from", "for", "in", "placing", "as", "both", "leading", "trailing",
            ]) {
                p.bump();
                continue;
            }
            if p.at_kw("order") {
                order_by_clause(p)?;
            }
            if !p.eat(SyntaxKind::Comma) {
                break;
            }
            p.eat_kw("variadic");
        }
    }
    p.expect(SyntaxKind::RParen, "`)`")?;
    p.finish();
    Ok(())
}

impl Parser<'_> {
    /// Eat a `*` operator token if present.
    pub(crate) fn eat_op_star(&mut self) -> bool {
        if self.at_op("*") {
            self.bump();
            true
        } else {
            false
        }
    }
}
