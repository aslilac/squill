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

use super::PResult;
use super::Parser;
use super::expr::expr;
use super::expr::type_name;
use super::grammar::query_body;
use super::grammar::where_clause;
use crate::syntax::SyntaxKind;

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
	let ctx = Ctx::of(p);
	ddl_tokens(p, Stop::Semicolon, ctx)?;
	p.finish();
	Ok(())
}

/// Which type positions this statement has, from its opening words.
///
/// The same tolerant `ColumnDef` shape covers column definitions, table
/// constraints, and index elements, so `id DESC` in `CREATE INDEX` looks
/// exactly like `id uuid` in `CREATE TABLE`. Only the statement says
/// which, and it says it up front.
#[derive(Clone, Copy)]
struct Ctx {
	/// Element lists hold column definitions (`name type ...`).
	columns: bool,
	/// `CREATE DOMAIN name AS type`.
	domain: bool,
}

impl Ctx {
	fn of(p: &Parser<'_>) -> Self {
		// Noise between the verb and the object it acts on.
		const MODIFIERS: &[&str] = &[
			"or",
			"replace",
			"global",
			"local",
			"temp",
			"temporary",
			"unlogged",
			"foreign",
			"materialized",
			"recursive",
			"unique",
			"concurrently",
			"if",
			"not",
			"exists",
		];
		let mut at = 1;
		let object = loop {
			let Some(word) = p.nth_word(at) else {
				return Self { columns: false, domain: false };
			};
			at += 1;
			if !MODIFIERS.contains(&word.as_str()) {
				break word;
			}
		};
		Self {
			// A view's `(a, b)` names columns without typing them, and an
			// index's elements are expressions — neither takes a type.
			columns: matches!(
				object.as_str(),
				"table" | "type" | "function" | "procedure" | "domain"
			),
			domain: object == "domain",
		}
	}
}

#[derive(PartialEq, Eq, Clone, Copy)]
enum Stop {
	/// Top level: consume through the terminating `;` (or EOF).
	Semicolon,
	/// Inside parens: stop before `,` or `)`.
	CommaOrParen,
}

fn ddl_tokens(p: &mut Parser<'_>, stop: Stop, ctx: Ctx) -> PResult {
	p.enter_depth()?;
	let result = ddl_tokens_inner(p, stop, ctx);
	p.exit_depth();
	result
}

fn ddl_tokens_inner(p: &mut Parser<'_>, stop: Stop, ctx: Ctx) -> PResult {
	// SQLite `CREATE TRIGGER ... BEGIN stmt; stmt; END;` bodies contain
	// semicolons that do not terminate the statement.
	let mut begin_depth = 0u32;
	// The word before the current one, and whether this statement is a
	// `CREATE DOMAIN` — both only to tell apart the positions where a
	// type name is grammatical from the ones where the same word is not.
	let mut prev = String::new();
	loop {
		if p.dialect() == crate::dialect::Dialect::Sqlite && stop == Stop::Semicolon
		{
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
			Some(SyntaxKind::RParen | SyntaxKind::Comma)
				if stop == Stop::CommaOrParen =>
			{
				return Ok(());
			}
			Some(SyntaxKind::RParen) => {
				return Err(p.error("unexpected `)`"));
			}
			Some(SyntaxKind::LParen) => element_list(p, ctx)?,
			Some(SyntaxKind::Ident) => {
				let word = p.text().to_ascii_lowercase();
				match word.as_str() {
					"where" if stop == Stop::Semicolon => where_clause(p)?,
					// `ADD COLUMN name type` / `ALTER COLUMN name TYPE
					// type`, the two alter actions that carry a type. The
					// action verb before `column` says which.
					"column" if matches!(prev.as_str(), "add" | "alter") => {
						column_action(p, &prev);
					}
					// `CREATE DOMAIN name AS type`.
					"as" if ctx.domain => {
						p.bump();
						opt_type_name(p);
					}
					// `RETURNS type` / `RETURNS SETOF type`; `RETURNS
					// TABLE (...)` is an element list instead.
					"returns" => {
						p.bump();
						p.eat_kw("setof");
						if !p.at_kw("table") {
							opt_type_name(p);
						}
					}
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
				prev = word;
			}
			Some(_) => p.bump(),
		}
	}
}

/// `ADD COLUMN name type ...` / `ALTER COLUMN name TYPE type ...`, from
/// the `column` keyword on. Anything unexpected falls back to the
/// tolerant token run, which resumes at whatever is left.
fn column_action(p: &mut Parser<'_>, verb: &str) {
	p.bump();
	// `IF NOT EXISTS` / `IF EXISTS`.
	if p.at_kw("if") {
		p.bump();
		p.eat_kw("not");
		p.eat_kw("exists");
	}
	if !(p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent)) {
		return;
	}
	p.bump();
	if verb == "add" {
		opt_type_name(p);
		return;
	}
	// ALTER COLUMN: `TYPE t` or the spelled-out `SET DATA TYPE t`.
	if p.at_kw("set") && p.nth_at_kw(1, "data") && p.nth_at_kw(2, "type") {
		p.bump();
		p.bump();
	}
	if p.eat_kw("type") {
		opt_type_name(p);
	}
}

/// `( element, element, ... )` — each element an expression when it fully
/// parses as one, a tolerant `ColumnDef` otherwise.
fn element_list(p: &mut Parser<'_>, ctx: Ctx) -> PResult {
	p.start(SyntaxKind::ElementList);
	p.expect(SyntaxKind::LParen, "`(`")?;
	if !p.at(SyntaxKind::RParen) {
		loop {
			element(p, ctx)?;
			if !p.eat(SyntaxKind::Comma) {
				break;
			}
		}
	}
	p.expect(SyntaxKind::RParen, "`)`")?;
	p.finish();
	Ok(())
}

fn element(p: &mut Parser<'_>, ctx: Ctx) -> PResult {
	let state = p.state();
	if expr(p, 0).is_ok() && (p.at(SyntaxKind::Comma) || p.at(SyntaxKind::RParen))
	{
		return Ok(());
	}
	p.backtrack(state);
	p.start(SyntaxKind::ColumnDef);
	// `name type ...` — but the same node shape also covers table
	// constraints, which open with a keyword and have no name or type.
	if ctx.columns
		&& !p.at_any_kw(CONSTRAINT_HEADS)
		&& (p.at(SyntaxKind::Ident) || p.at(SyntaxKind::QuotedIdent))
	{
		p.bump();
		opt_type_name(p);
	}
	ddl_tokens(p, Stop::CommaOrParen, ctx)?;
	p.finish();
	Ok(())
}

/// Words that open a table constraint rather than a column definition.
const CONSTRAINT_HEADS: &[&str] =
	&["check", "constraint", "exclude", "foreign", "like", "primary", "unique"];

/// Parse a type name here if one parses cleanly; leave the position
/// untouched otherwise, so the tolerant token run still sees it.
fn opt_type_name(p: &mut Parser<'_>) -> bool {
	let state = p.state();
	if type_name(p).is_ok() {
		return true;
	}
	p.backtrack(state);
	false
}

fn can_start_expr(p: &Parser<'_>) -> bool {
	!matches!(
		p.kind(),
		None
			| Some(
				SyntaxKind::Comma
					| SyntaxKind::Semicolon
					| SyntaxKind::RParen
					| SyntaxKind::RBracket
			)
	)
}
