//! PL/pgSQL body grammar (TREE-102).
//!
//! A statement-level wrapper language: PL/pgSQL tokenizes exactly like
//! SQL, and embedded SQL statements dispatch to the existing statement
//! grammar (with `INTO [STRICT]` enabled via the parser's plpgsql flag).
//! Every body statement gets the same rollback-to-ErrorStatement
//! recovery as top-level SQL.

use super::PResult;
use super::Parser;
use super::expr::expr;
use super::expr::type_name;
use super::grammar::pl_into;
use super::grammar::qualified_name;
use super::grammar::query_body;
use super::grammar::select_core_rest;
use super::grammar::trailing_clauses;
use crate::syntax::SyntaxKind;

/// Keywords that end a statement list inside a block construct.
const STMT_STOP: &[&str] = &["end", "elsif", "else", "when", "exception"];

/// Parse one body-level statement with error recovery.
pub(crate) fn body_statement(p: &mut Parser<'_>) {
	if p.at(SyntaxKind::Semicolon) {
		p.start(SyntaxKind::EmptyStmt);
		p.bump();
		p.finish();
		return;
	}
	let state = p.state();
	if let Err(error) = pl_statement(p) {
		p.backtrack(state);
		p.error_statement(error);
	}
}

/// A statement list that stops before block-closing keywords.
fn pl_statements(p: &mut Parser<'_>) -> PResult {
	while !p.at_eof() && !p.at_any_kw(STMT_STOP) {
		body_statement(p);
	}
	Ok(())
}

fn pl_statement(p: &mut Parser<'_>) -> PResult {
	p.enter_depth()?;
	let result = pl_statement_inner(p);
	p.exit_depth();
	result
}

fn pl_statement_inner(p: &mut Parser<'_>) -> PResult {
	if p.at_kw("declare") || p.at_kw("begin") || p.at_op("<<") {
		return pl_block(p);
	}
	if p.at_kw("if") {
		return pl_if(p);
	}
	if p.at_kw("case") {
		return pl_case(p);
	}
	if p.at_any_kw(&["loop", "while", "for", "foreach"]) {
		return pl_loop(p);
	}
	if p.at_kw("exit") || p.at_kw("continue") {
		return pl_exit(p);
	}
	if p.at_kw("return") {
		return pl_return(p);
	}
	if p.at_kw("raise") {
		return pl_raise(p);
	}
	if p.at_kw("perform") {
		return pl_perform(p);
	}
	if p.at_kw("execute") {
		return pl_execute(p);
	}
	if p.at_kw("get") {
		return pl_get_diagnostics(p);
	}
	if p.at_kw("null") && p.nth_at(1, SyntaxKind::Semicolon) {
		p.start(SyntaxKind::PlNull);
		p.bump();
		p.bump();
		p.finish();
		return Ok(());
	}
	// Assignment: `name[.name][[idx]] := expr;` (also plain `=`).
	if p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent) {
		let state = p.state();
		if pl_assign(p).is_ok() {
			return Ok(());
		}
		p.backtrack(state);
	}
	// Everything else: the regular SQL statement grammar (SELECT, DML,
	// DDL, ...), which carries its own recovery.
	if p.at_eof() {
		return Err(p.error("expected a statement"));
	}
	p.statement();
	Ok(())
}

/// `[<<label>>] [DECLARE decls] BEGIN stmts [EXCEPTION handlers] END
/// [label];`
fn pl_block(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlBlock);
	if p.at_op("<<") {
		p.bump();
		ident(p)?;
		if !p.at_op(">>") {
			return Err(p.error("expected `>>`"));
		}
		p.bump();
	}
	if p.eat_kw("declare") {
		while !p.at_kw("begin") && !p.at_eof() {
			pl_declare(p)?;
		}
	}
	p.expect_kw("begin")?;
	pl_statements(p)?;
	if p.at_kw("exception") {
		p.start(SyntaxKind::PlException);
		p.bump();
		while p.at_kw("when") {
			p.start(SyntaxKind::PlWhen);
			p.bump();
			expr(p, 0)?;
			p.expect_kw("then")?;
			pl_statements(p)?;
			p.finish();
		}
		p.finish();
	}
	p.expect_kw("end")?;
	if p.at(SyntaxKind::Ident) && !p.at_any_kw(&["if", "case", "loop"]) {
		p.bump(); // closing label
	}
	end_semicolon(p)?;
	p.finish();
	Ok(())
}

/// `name [CONSTANT] type [NOT NULL] [{:= | = | DEFAULT} expr];` or
/// `name ALIAS FOR $n;`
fn pl_declare(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlDeclare);
	ident(p)?;
	if p.eat_kw("alias") {
		p.expect_kw("for")?;
		if !p.eat(SyntaxKind::Param) {
			ident(p)?;
		}
		p.expect(SyntaxKind::Semicolon, "`;`")?;
		p.finish();
		return Ok(());
	}
	p.eat_kw("constant");
	type_name(p)?;
	// `%TYPE` / `%ROWTYPE` suffix.
	if p.at_op("%") {
		p.bump();
		ident(p)?;
	}
	if p.eat_kw("not") {
		p.expect_kw("null")?;
	}
	if assignment_operator(p) || p.eat_kw("default") {
		expr(p, 0)?;
	}
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

fn pl_if(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlIf);
	p.expect_kw("if")?;
	expr(p, 0)?;
	p.expect_kw("then")?;
	pl_statements(p)?;
	while p.at_kw("elsif") {
		p.start(SyntaxKind::PlElsif);
		p.bump();
		expr(p, 0)?;
		p.expect_kw("then")?;
		pl_statements(p)?;
		p.finish();
	}
	if p.at_kw("else") {
		p.start(SyntaxKind::PlElse);
		p.bump();
		pl_statements(p)?;
		p.finish();
	}
	p.expect_kws(&["end", "if"])?;
	end_semicolon(p)?;
	p.finish();
	Ok(())
}

fn pl_case(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlCase);
	p.expect_kw("case")?;
	if !p.at_kw("when") {
		expr(p, 0)?;
	}
	while p.at_kw("when") {
		p.start(SyntaxKind::PlWhen);
		p.bump();
		loop {
			expr(p, 0)?;
			if !p.eat(SyntaxKind::Comma) {
				break;
			}
		}
		p.expect_kw("then")?;
		pl_statements(p)?;
		p.finish();
	}
	if p.at_kw("else") {
		p.start(SyntaxKind::PlElse);
		p.bump();
		pl_statements(p)?;
		p.finish();
	}
	p.expect_kws(&["end", "case"])?;
	end_semicolon(p)?;
	p.finish();
	Ok(())
}

fn pl_loop(p: &mut Parser<'_>) -> PResult {
	let kind = if p.at_kw("loop") {
		SyntaxKind::PlLoop
	} else if p.at_kw("while") {
		SyntaxKind::PlWhile
	} else if p.at_kw("for") {
		SyntaxKind::PlFor
	} else {
		SyntaxKind::PlForeach
	};
	p.start(kind);
	match kind {
		SyntaxKind::PlLoop => {
			p.bump();
		}
		SyntaxKind::PlWhile => {
			p.bump();
			expr(p, 0)?;
			p.expect_kw("loop")?;
		}
		SyntaxKind::PlFor => {
			p.bump();
			loop {
				ident(p)?;
				if !p.eat(SyntaxKind::Comma) {
					break;
				}
			}
			p.expect_kw("in")?;
			p.eat_kw("reverse");
			if p.at_any_kw(&["select", "with", "values"]) || p.at(SyntaxKind::LParen)
			{
				query_body(p)?;
			} else if p.at_kw("execute") {
				pl_execute_tail(p)?;
			} else {
				// Integer range: `expr .. expr [BY expr]`. The lexer may
				// fold `..10` into a single `.10`-style number; accept
				// both shapes.
				expr(p, 0)?;
				if p.at(SyntaxKind::Dot) {
					p.bump();
					p.eat(SyntaxKind::Dot);
					if !p.at_kw("loop") && !p.at_kw("by") {
						expr(p, 0)?;
					}
				}
				if p.eat_kw("by") {
					expr(p, 0)?;
				}
			}
			p.expect_kw("loop")?;
		}
		_ => {
			// FOREACH target [SLICE n] IN ARRAY expr
			p.bump();
			qualified_name(p)?;
			if p.eat_kw("slice") {
				expr(p, 0)?;
			}
			p.expect_kws(&["in", "array"])?;
			expr(p, 0)?;
			p.expect_kw("loop")?;
		}
	}
	pl_statements(p)?;
	p.expect_kws(&["end", "loop"])?;
	if p.at(SyntaxKind::Ident) {
		p.bump(); // closing label
	}
	end_semicolon(p)?;
	p.finish();
	Ok(())
}

fn pl_exit(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlExit);
	p.bump(); // EXIT | CONTINUE
	if p.at(SyntaxKind::Ident) && !p.at_kw("when") {
		p.bump(); // label
	}
	if p.eat_kw("when") {
		expr(p, 0)?;
	}
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

fn pl_return(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlReturn);
	p.expect_kw("return")?;
	if p.eat(SyntaxKind::Semicolon) {
		p.finish();
		return Ok(());
	}
	if p.eat_kw("next") {
		expr(p, 0)?;
	} else if p.eat_kw("query") {
		if p.at_kw("execute") {
			pl_execute_tail(p)?;
		} else {
			query_body(p)?;
		}
	} else {
		expr(p, 0)?;
	}
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

const RAISE_LEVELS: &[&str] =
	&["debug", "log", "info", "notice", "warning", "exception"];

fn pl_raise(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlRaise);
	p.expect_kw("raise")?;
	if p.at_any_kw(RAISE_LEVELS) {
		p.bump();
	}
	if !p.at(SyntaxKind::Semicolon) && !p.at_kw("using") {
		// Format string / condition name / SQLSTATE, plus arguments.
		expr(p, 0)?;
		while p.eat(SyntaxKind::Comma) {
			expr(p, 0)?;
		}
	}
	if p.eat_kw("using") {
		loop {
			ident(p)?;
			if !p.at_op("=") {
				return Err(p.error("expected `=`"));
			}
			p.bump();
			expr(p, 0)?;
			if !p.eat(SyntaxKind::Comma) {
				break;
			}
		}
	}
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

fn pl_perform(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlPerform);
	p.expect_kw("perform")?;
	// PERFORM is SELECT under another name, trailing clauses included
	// (`PERFORM 1 FROM t FOR UPDATE`, `... LIMIT 1`).
	select_core_rest(p)?;
	trailing_clauses(p)?;
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

fn pl_execute(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlExecute);
	pl_execute_tail(p)?;
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

/// `EXECUTE expr [INTO [STRICT] targets] [USING exprs]` without the `;`
/// (also used by FOR ... IN EXECUTE and RETURN QUERY EXECUTE).
fn pl_execute_tail(p: &mut Parser<'_>) -> PResult {
	p.expect_kw("execute")?;
	expr(p, 0)?;
	if p.at_kw("into") {
		pl_into(p)?;
	}
	if p.eat_kw("using") {
		loop {
			expr(p, 0)?;
			if !p.eat(SyntaxKind::Comma) {
				break;
			}
		}
	}
	Ok(())
}

fn pl_get_diagnostics(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlGetDiag);
	p.expect_kw("get")?;
	p.eat_kw("stacked");
	p.expect_kw("diagnostics")?;
	loop {
		qualified_name(p)?;
		if !assignment_operator(p) {
			return Err(p.error("expected `:=` or `=`"));
		}
		ident(p)?;
		if !p.eat(SyntaxKind::Comma) {
			break;
		}
	}
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

/// `target := expr;` — target may be qualified and subscripted.
fn pl_assign(p: &mut Parser<'_>) -> PResult {
	p.start(SyntaxKind::PlAssign);
	qualified_name(p)?;
	while p.at(SyntaxKind::LBracket) {
		p.bump();
		expr(p, 0)?;
		p.expect(SyntaxKind::RBracket, "`]`")?;
	}
	if !assignment_operator(p) {
		return Err(p.error("expected `:=` or `=`"));
	}
	expr(p, 0)?;
	p.expect(SyntaxKind::Semicolon, "`;`")?;
	p.finish();
	Ok(())
}

/// Eat `:=` (a single operator token) or plain `=`; false if neither.
fn assignment_operator(p: &mut Parser<'_>) -> bool {
	if p.at_op(":=") || p.at_op("=") {
		p.bump();
		true
	} else {
		false
	}
}

/// The body's outermost END often has no trailing `;` before EOF.
fn end_semicolon(p: &mut Parser<'_>) -> PResult {
	if !p.at_eof() {
		p.expect(SyntaxKind::Semicolon, "`;`")?;
	}
	Ok(())
}

fn ident(p: &mut Parser<'_>) -> PResult {
	if p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent) {
		p.bump();
		Ok(())
	} else {
		Err(p.error("expected a name"))
	}
}
