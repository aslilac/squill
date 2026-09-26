//! DML statement grammar: INSERT, UPDATE, DELETE, MERGE, and
//! WITH-prefixed variants (including data-modifying CTEs).

use super::PResult;
use super::Parser;
use super::expr::expr;
use super::grammar::alias_name;
use super::grammar::at_bare_alias;
use super::grammar::at_index_hint;
use super::grammar::from_clause;
use super::grammar::from_item;
use super::grammar::index_hint;
use super::grammar::paren_expr_list;
use super::grammar::paren_name_list;
use super::grammar::qualified_name;
use super::grammar::query_body;
use super::grammar::select_list;
use super::grammar::where_clause;
use super::grammar::with_clause;
use crate::dialect::Dialect;
use crate::syntax::SyntaxKind;

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
	let kind = if p.at_kw("insert") || at_replace_into(p) {
		SyntaxKind::InsertStmt
	} else if p.at_kw("update") {
		SyntaxKind::UpdateStmt
	} else if p.at_kw("delete") {
		SyntaxKind::DeleteStmt
	} else if p.at_kw("merge") {
		SyntaxKind::MergeStmt
	} else {
		SyntaxKind::SelectStmt
	};
	p.open_at(checkpoint, kind);
	match kind {
		SyntaxKind::InsertStmt => insert_body(p)?,
		SyntaxKind::UpdateStmt => update_body(p)?,
		SyntaxKind::DeleteStmt => delete_body(p)?,
		SyntaxKind::MergeStmt => merge_body(p)?,
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

pub(crate) fn merge_stmt(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::MergeStmt);
	merge_body(p)?;
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
	} else if p.at_kw("merge") {
		p.start(SyntaxKind::MergeStmt);
		merge_body(p)?;
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

/// SQLite's `REPLACE INTO`, an `INSERT OR REPLACE` by another name.
pub(crate) fn at_replace_into(p: &Parser<'_>) -> bool {
	p.dialect() == Dialect::Sqlite && p.at_kw("replace") && p.nth_at_kw(1, "into")
}

/// SQLite's `OR ROLLBACK | ABORT | REPLACE | FAIL | IGNORE` after
/// `INSERT` or `UPDATE`: what to do when a constraint fails.
fn conflict_resolution(p: &mut Parser<'_>) -> PResult {
	if p.dialect() == Dialect::Sqlite && p.at_kw("or") {
		p.bump();
		if !p.at_any_kw(&["rollback", "abort", "replace", "fail", "ignore"]) {
			return Err(p.error(
				"expected `ROLLBACK`, `ABORT`, `REPLACE`, `FAIL`, or `IGNORE`",
			));
		}
		p.bump();
	}
	Ok(())
}

fn insert_body(p: &mut Parser<'_>) -> PResult {
	if at_replace_into(p) {
		p.bump();
	} else {
		p.expect_kw("insert")?;
		conflict_resolution(p)?;
	}
	p.expect_kw("into")?;
	dml_target(p)?;
	if p.at(SyntaxKind::LParen) {
		// Wrapped so long column lists format as an indented block.
		p.start(SyntaxKind::ElementList);
		paren_name_list(p)?;
		p.finish();
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
	conflict_resolution(p)?;
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

/// `MERGE INTO target [AS alias] USING source ON condition`, one or more
/// WHEN clauses, and (Postgres 17) RETURNING.
fn merge_body(p: &mut Parser<'_>) -> PResult {
	p.expect_kws(&["merge", "into"])?;
	p.eat_kw("only");
	dml_target(p)?;
	p.start(SyntaxKind::UsingClause);
	p.expect_kw("using")?;
	from_item(p)?;
	p.start(SyntaxKind::JoinCondition);
	p.expect_kw("on")?;
	expr(p, 0)?;
	p.finish();
	p.finish();
	if !p.at_kw("when") {
		return Err(p.error("expected `WHEN`"));
	}
	while p.at_kw("when") {
		merge_when(p)?;
	}
	if p.at_kw("returning") {
		returning_clause(p)?;
	}
	Ok(())
}

/// `WHEN [NOT] MATCHED [BY SOURCE | BY TARGET] [AND condition] THEN`
/// `UPDATE SET ... | DELETE | INSERT ... | DO NOTHING`.
fn merge_when(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::MergeWhenClause);
	p.expect_kw("when")?;
	p.eat_kw("not");
	p.expect_kw("matched")?;
	if p.eat_kw("by") && !p.eat_kw("source") {
		p.expect_kw("target")?;
	}
	if p.eat_kw("and") {
		expr(p, 0)?;
	}
	p.expect_kw("then")?;
	if p.eat_kw("update") {
		set_clause(p)?;
	} else if p.eat_kw("delete") {
	} else if p.eat_kw("do") {
		p.expect_kw("nothing")?;
	} else {
		p.expect_kw("insert")?;
		if p.at(SyntaxKind::LParen) {
			p.start(SyntaxKind::ElementList);
			paren_name_list(p)?;
			p.finish();
		}
		if p.eat_kw("overriding") {
			if !p.eat_kw("system") {
				p.expect_kw("user")?;
			}
			p.expect_kw("value")?;
		}
		if p.eat_kw("default") {
			p.expect_kw("values")?;
		} else {
			super::grammar::values_clause(p)?;
		}
	}
	p.finish();
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
	if !at_index_hint(p) && (p.eat_kw("as") || at_bare_alias(p, DML_ALIAS_STOP)) {
		alias_name(p)?;
	}
	index_hint(p)?;
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
