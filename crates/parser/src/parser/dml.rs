//! DML statement grammar: INSERT, UPDATE, DELETE, and WITH-prefixed
//! variants (including data-modifying CTEs).

use crate::syntax::SyntaxKind;

use super::expr::expr;
use super::grammar::{
    alias_name, at_bare_alias, from_clause, from_item, paren_expr_list, paren_name_list,
    qualified_name, query_body, select_list, where_clause, with_clause,
};
use super::{PResult, Parser};

/// Keywords that stop a bare alias after a DML target table.
const DML_ALIAS_STOP: &[&str] = &[
    "set",
    "using",
    "where",
    "returning",
    "from",
    "values",
    "select",
    "on",
    "default",
];

/// `WITH ...` followed by SELECT or DML: parse the with-clause, then wrap
/// the whole statement in the kind the follow keyword dictates.
pub(crate) fn with_statement(p: &mut Parser<'_>) -> PResult {
    let checkpoint = p.checkpoint();
    with_clause(p)?;
    let kind = if p.at_kw("insert") {
        SyntaxKind::InsertStmt
    } else if p.at_kw("update") {
        SyntaxKind::UpdateStmt
    } else if p.at_kw("delete") {
        SyntaxKind::DeleteStmt
    } else {
        SyntaxKind::SelectStmt
    };
    p.open_at(checkpoint, kind);
    match kind {
        SyntaxKind::InsertStmt => insert_body(p)?,
        SyntaxKind::UpdateStmt => update_body(p)?,
        SyntaxKind::DeleteStmt => delete_body(p)?,
        _ => {
            super::grammar::query_tail(p)?;
        }
    }
    end_statement(p)?;
    p.finish();
    Ok(())
}

pub(crate) fn insert_stmt(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::InsertStmt);
    insert_body(p)?;
    end_statement(p)?;
    p.finish();
    Ok(())
}

pub(crate) fn update_stmt(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::UpdateStmt);
    update_body(p)?;
    end_statement(p)?;
    p.finish();
    Ok(())
}

pub(crate) fn delete_stmt(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::DeleteStmt);
    delete_body(p)?;
    end_statement(p)?;
    p.finish();
    Ok(())
}

/// Body-only parsers for data-modifying CTE bodies: no semicolon.
pub(crate) fn dml_in_parens(p: &mut Parser<'_>) -> PResult {
    if p.at_kw("insert") {
        p.start(SyntaxKind::InsertStmt);
        insert_body(p)?;
    } else if p.at_kw("update") {
        p.start(SyntaxKind::UpdateStmt);
        update_body(p)?;
    } else {
        p.start(SyntaxKind::DeleteStmt);
        delete_body(p)?;
    }
    p.finish();
    Ok(())
}

fn end_statement(p: &mut Parser<'_>) -> PResult {
    if !p.at_eof() {
        p.expect(SyntaxKind::Semicolon, "`;` or end of statement")?;
    }
    Ok(())
}

fn insert_body(p: &mut Parser<'_>) -> PResult {
    p.expect_kws(&["insert", "into"])?;
    dml_target(p)?;
    if p.at(SyntaxKind::LParen) {
        paren_name_list(p)?;
    }
    if p.at_kw("overriding") {
        p.bump();
        if !p.eat_kw("system") {
            p.expect_kw("user")?;
        }
        p.expect_kw("value")?;
    }
    if p.at_kw("default") {
        p.bump();
        p.expect_kw("values")?;
    } else {
        query_body(p)?;
    }
    if p.at_kw("on") {
        on_conflict(p)?;
    }
    if p.at_kw("returning") {
        returning_clause(p)?;
    }
    Ok(())
}

fn update_body(p: &mut Parser<'_>) -> PResult {
    p.expect_kw("update")?;
    p.eat_kw("only");
    dml_target(p)?;
    set_clause(p)?;
    if p.at_kw("from") {
        from_clause(p)?;
    }
    if p.at_kw("where") {
        update_where(p)?;
    }
    if p.at_kw("returning") {
        returning_clause(p)?;
    }
    Ok(())
}

fn delete_body(p: &mut Parser<'_>) -> PResult {
    p.expect_kws(&["delete", "from"])?;
    p.eat_kw("only");
    dml_target(p)?;
    if p.at_kw("using") {
        p.start(SyntaxKind::UsingClause);
        p.bump();
        loop {
            from_item(p)?;
            if !p.eat(SyntaxKind::Comma) {
                break;
            }
        }
        p.finish();
    }
    if p.at_kw("where") {
        update_where(p)?;
    }
    if p.at_kw("returning") {
        returning_clause(p)?;
    }
    Ok(())
}

/// `WHERE expr` or `WHERE CURRENT OF cursor`.
fn update_where(p: &mut Parser<'_>) -> PResult {
    if p.at_kw("where") && p.nth_at_kw(1, "current") && p.nth_at_kw(2, "of") {
        p.start(SyntaxKind::WhereClause);
        p.bump();
        p.bump();
        p.bump();
        qualified_name(p)?;
        p.finish();
        Ok(())
    } else {
        where_clause(p)
    }
}

fn dml_target(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::TableRef);
    qualified_name(p)?;
    if p.eat_kw("as") || at_bare_alias(p, DML_ALIAS_STOP) {
        alias_name(p)?;
    }
    p.finish();
    Ok(())
}

fn set_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SetClause);
    p.expect_kw("set")?;
    loop {
        set_item(p)?;
        if !p.eat(SyntaxKind::Comma) {
            break;
        }
    }
    p.finish();
    Ok(())
}

fn set_item(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::SetItem);
    if p.at(SyntaxKind::LParen) {
        // `(a, b) = (expr, expr)` / `(a, b) = (subquery)`.
        paren_name_list(p)?;
    } else {
        p.start(SyntaxKind::ColumnRef);
        qualified_name(p)?;
        p.finish();
    }
    if !p.at_op("=") {
        return Err(p.error("expected `=`"));
    }
    p.bump();
    expr(p, 0)?;
    p.finish();
    Ok(())
}

fn on_conflict(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::OnConflictClause);
    p.expect_kws(&["on", "conflict"])?;
    if p.at(SyntaxKind::LParen) {
        paren_expr_list(p)?;
        if p.at_kw("where") {
            where_clause(p)?;
        }
    } else if p.at_kw("on") {
        p.bump();
        p.expect_kw("constraint")?;
        qualified_name(p)?;
    }
    p.expect_kw("do")?;
    if !p.eat_kw("nothing") {
        p.expect_kw("update")?;
        set_clause(p)?;
        if p.at_kw("where") {
            where_clause(p)?;
        }
    }
    p.finish();
    Ok(())
}

pub(crate) fn returning_clause(p: &mut Parser<'_>) -> PResult {
    p.start(SyntaxKind::ReturningClause);
    p.expect_kw("returning")?;
    select_list(p)?;
    if p.in_plpgsql() && p.at_kw("into") {
        super::grammar::pl_into(p)?;
    }
    p.finish();
    Ok(())
}
