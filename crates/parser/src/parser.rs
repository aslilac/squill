//! Recursive descent parser: tokens to a concrete syntax tree.
//!
//! Statements are keyword-dispatched; expressions use Pratt parsing with a
//! dialect-parameterized precedence table (see `parser::expr`). The parser
//! emits [`Event`]s over the non-trivia tokens; trivia attachment happens
//! in the tree sink (TREE-94).
//!
//! Error recovery: a statement that fails to parse becomes an
//! `ErrorStatement` node containing its raw tokens verbatim plus a
//! [`Diagnostic`], and parsing resumes at the next top-level `;`. The tree
//! never drops tokens.

mod ddl;
mod dml;
mod expr;
mod grammar;
mod plpgsql;

use crate::dialect::Dialect;
use crate::lexer::Token;
use crate::syntax::SyntaxKind;
pub use crate::tree::Cst;
use crate::tree::Event;
use crate::tree::build_tree;

/// A parse problem, with byte offsets into the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
	pub message: String,
	pub start: usize,
	pub end: usize,
}

/// The result of parsing: always a full, lossless tree, plus diagnostics
/// for every statement that landed as an `ErrorStatement`.
#[derive(Debug)]
pub struct Parse {
	pub cst: Cst,
	pub diagnostics: Vec<Diagnostic>,
}

/// Parse a lexed token stream (trivia included) into a CST.
pub fn parse(tokens: &[Token<'_>], dialect: Dialect) -> Parse {
	let mut toks = Vec::new();
	let mut offset = 0;
	for token in tokens {
		let end = offset + token.text.len();
		if !token.kind.is_trivia() {
			toks.push(Tok { kind: token.kind, text: token.text, start: offset, end });
		}
		offset = end;
	}
	let eof = offset;
	let mut parser = Parser {
		toks,
		pos: 0,
		events: Vec::new(),
		diagnostics: Vec::new(),
		dialect,
		eof,
		depth: 0,
		fuel: 0,
		in_plpgsql: false,
	};
	parser.refuel();
	parser.events.push(Event::StartNode(SyntaxKind::Root));
	while !parser.at_eof() {
		parser.statement();
	}
	parser.events.push(Event::FinishNode);
	Parse {
		cst: build_tree(tokens, &parser.events),
		diagnostics: parser.diagnostics,
	}
}

/// Parse a PL/pgSQL function body (the content of a dollar-quoted
/// `LANGUAGE plpgsql` string) into a CST of `Pl*` statement nodes.
/// Same guarantees as [`parse`]: always lossless, never fails.
pub fn parse_plpgsql_body(tokens: &[Token<'_>], dialect: Dialect) -> Parse {
	let mut toks = Vec::new();
	let mut offset = 0;
	for token in tokens {
		let end = offset + token.text.len();
		if !token.kind.is_trivia() {
			toks.push(Tok { kind: token.kind, text: token.text, start: offset, end });
		}
		offset = end;
	}
	let eof = offset;
	let mut parser = Parser {
		toks,
		pos: 0,
		events: Vec::new(),
		diagnostics: Vec::new(),
		dialect,
		eof,
		depth: 0,
		fuel: 0,
		in_plpgsql: true,
	};
	parser.refuel();
	parser.events.push(Event::StartNode(SyntaxKind::Root));
	while !parser.at_eof() {
		plpgsql::body_statement(&mut parser);
	}
	parser.events.push(Event::FinishNode);
	Parse {
		cst: build_tree(tokens, &parser.events),
		diagnostics: parser.diagnostics,
	}
}

/// A non-trivia token with its byte span.
struct Tok<'src> {
	kind: SyntaxKind,
	text: &'src str,
	start: usize,
	end: usize,
}

/// An error inside one statement; triggers rollback to `ErrorStatement`.
pub(crate) struct StmtError {
	message: String,
	/// Index into `toks` where the error occurred.
	at: usize,
}

pub(crate) type PResult = Result<(), StmtError>;

/// Recursion bound: deeper nesting turns the statement into an
/// `ErrorStatement` instead of risking stack overflow.
const MAX_DEPTH: u32 = 200;

/// Work budget per statement: generous linear headroom over the token
/// count. Exponential speculative backtracking (deep paren runs feeding
/// the `((SELECT` disambiguation) burns through it, and the statement
/// falls back to `ErrorStatement` — verbatim, lossless — instead of
/// hanging.
const FUEL_BASE: usize = 4096;
const FUEL_PER_TOKEN: usize = 64;

pub(crate) struct Parser<'src> {
	toks: Vec<Tok<'src>>,
	pos: usize,
	events: Vec<Event>,
	diagnostics: Vec<Diagnostic>,
	dialect: Dialect,
	eof: usize,
	depth: u32,
	/// Remaining work budget; see [`FUEL_BASE`].
	fuel: usize,
	/// Parsing a PL/pgSQL body: enables `INTO [STRICT]` targets in
	/// query positions.
	in_plpgsql: bool,
}

impl Parser<'_> {
	// ---- cursor ----

	pub(crate) fn at_eof(&self) -> bool {
		self.pos >= self.toks.len()
	}

	pub(crate) fn kind(&self) -> Option<SyntaxKind> {
		self.toks.get(self.pos).map(|t| t.kind)
	}

	pub(crate) fn nth_kind(&self, n: usize) -> Option<SyntaxKind> {
		self.toks.get(self.pos + n).map(|t| t.kind)
	}

	pub(crate) fn text(&self) -> &str {
		self.toks.get(self.pos).map_or("", |t| t.text)
	}

	pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
		self.kind() == Some(kind)
	}

	pub(crate) fn nth_at(&self, n: usize, kind: SyntaxKind) -> bool {
		self.nth_kind(n) == Some(kind)
	}

	/// Is the current token the given keyword? Keywords are bare `Ident`
	/// tokens compared case-insensitively; quoted identifiers never match.
	pub(crate) fn at_kw(&self, kw: &str) -> bool {
		self.nth_at_kw(0, kw)
	}

	pub(crate) fn nth_at_kw(&self, n: usize, kw: &str) -> bool {
		self.toks.get(self.pos + n).is_some_and(|t| {
			t.kind == SyntaxKind::Ident && t.text.eq_ignore_ascii_case(kw)
		})
	}

	/// The nth bare word from here, lowercased; `None` at anything else.
	pub(crate) fn nth_word(&self, n: usize) -> Option<String> {
		self
			.toks
			.get(self.pos + n)
			.filter(|t| t.kind == SyntaxKind::Ident)
			.map(|t| t.text.to_ascii_lowercase())
	}

	pub(crate) fn at_any_kw(&self, kws: &[&str]) -> bool {
		kws.iter().any(|kw| self.at_kw(kw))
	}

	/// Is the current token an operator with exactly this text?
	pub(crate) fn at_op(&self, op: &str) -> bool {
		self.nth_at_op(0, op)
	}

	pub(crate) fn nth_at_op(&self, n: usize, op: &str) -> bool {
		self
			.toks
			.get(self.pos + n)
			.is_some_and(|t| t.kind == SyntaxKind::Operator && t.text == op)
	}

	// ---- events ----

	pub(crate) fn bump(&mut self) {
		debug_assert!(!self.at_eof());
		self.events.push(Event::Token);
		self.pos += 1;
	}

	pub(crate) fn start(&mut self, kind: SyntaxKind) {
		self.events.push(Event::StartNode(kind));
	}

	pub(crate) fn finish(&mut self) {
		self.events.push(Event::FinishNode);
	}

	/// A position in the event stream that a later `open_at` can
	/// retroactively enclose in a new node (Pratt-style left recursion).
	pub(crate) fn checkpoint(&self) -> usize {
		self.events.len()
	}

	/// Retroactively open a node at `checkpoint`, so everything emitted
	/// since becomes its first children. The caller parses the rest of the
	/// node and closes it with `finish`. Wrapping repeatedly at the same
	/// checkpoint nests left-associatively.
	pub(crate) fn open_at(&mut self, checkpoint: usize, kind: SyntaxKind) {
		self.events.insert(checkpoint, Event::StartNode(kind));
	}

	/// Change the kind of a `StartNode` event already emitted at
	/// `checkpoint` (e.g. a `ParenExpr` that turned out to be a `RowExpr`).
	pub(crate) fn rewrite_start(&mut self, checkpoint: usize, kind: SyntaxKind) {
		debug_assert!(matches!(self.events[checkpoint], Event::StartNode(_)));
		self.events[checkpoint] = Event::StartNode(kind);
	}

	/// Snapshot for speculative parsing; pair with `backtrack` on failure.
	/// Restores the depth counter too, since an `Err` bail skips
	/// `exit_depth` calls.
	pub(crate) fn state(&self) -> (usize, usize, u32) {
		(self.events.len(), self.pos, self.depth)
	}

	pub(crate) fn backtrack(&mut self, state: (usize, usize, u32)) {
		// Rewound events are re-done work: charge them, so exponential
		// speculation exhausts the budget instead of the clock.
		let rewound = self.events.len() - state.0;
		self.fuel = self.fuel.saturating_sub(rewound + 1);
		self.events.truncate(state.0);
		self.pos = state.1;
		self.depth = state.2;
	}

	// ---- eating ----

	pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
		if self.at(kind) {
			self.bump();
			true
		} else {
			false
		}
	}

	pub(crate) fn eat_kw(&mut self, kw: &str) -> bool {
		if self.at_kw(kw) {
			self.bump();
			true
		} else {
			false
		}
	}

	/// Eat each keyword in order; all-or-nothing is not checked — callers
	/// use this for fixed keyword runs after peeking the first word.
	pub(crate) fn expect_kws(&mut self, kws: &[&str]) -> PResult {
		for kw in kws {
			self.expect_kw(kw)?;
		}
		Ok(())
	}

	pub(crate) fn expect(&mut self, kind: SyntaxKind, what: &str) -> PResult {
		if self.eat(kind) {
			Ok(())
		} else {
			Err(self.error(&format!("expected {what}")))
		}
	}

	pub(crate) fn expect_kw(&mut self, kw: &str) -> PResult {
		if self.eat_kw(kw) {
			Ok(())
		} else {
			Err(self.error(&format!("expected `{}`", kw.to_uppercase())))
		}
	}

	/// Guard a recursive descent; pair with `exit_depth` on all Ok paths.
	/// Unpaired exits after an `Err` are fine: `statement` resets depth.
	pub(crate) fn enter_depth(&mut self) -> PResult {
		self.depth += 1;
		if self.depth > MAX_DEPTH {
			Err(self.error("nesting too deep"))
		} else if self.fuel == 0 {
			Err(self.error("statement too complex"))
		} else {
			Ok(())
		}
	}

	/// Reset the work budget (per statement, and once at parse start so
	/// the PL/pgSQL body path is covered too).
	fn refuel(&mut self) {
		let remaining = self.toks.len().saturating_sub(self.pos);
		self.fuel = FUEL_BASE + remaining * FUEL_PER_TOKEN;
	}

	pub(crate) fn exit_depth(&mut self) {
		self.depth = self.depth.saturating_sub(1);
	}

	pub(crate) fn error(&self, message: &str) -> StmtError {
		let found = match self.toks.get(self.pos) {
			Some(t) => format!("`{}`", t.text),
			None => "end of input".to_string(),
		};
		StmtError { message: format!("{message}, found {found}"), at: self.pos }
	}

	// ---- statements & recovery ----

	pub(crate) fn statement(&mut self) {
		if self.at(SyntaxKind::Semicolon) {
			self.start(SyntaxKind::EmptyStmt);
			self.bump();
			self.finish();
			return;
		}
		let events_checkpoint = self.events.len();
		let pos_checkpoint = self.pos;
		self.refuel();
		let result = self.statement_inner();
		self.depth = 0;
		if let Err(error) = result {
			self.events.truncate(events_checkpoint);
			self.pos = pos_checkpoint;
			self.error_statement(error);
		}
	}

	fn statement_inner(&mut self) -> PResult {
		if self.at_any_kw(&["select", "values", "table"])
			|| self.at(SyntaxKind::LParen)
		{
			grammar::select_stmt(self)
		} else if self.at_kw("with") {
			dml::with_statement(self)
		} else if self.at_kw("insert") {
			dml::insert_stmt(self)
		} else if self.at_kw("update") {
			dml::update_stmt(self)
		} else if self.at_kw("delete") {
			dml::delete_stmt(self)
		} else if self
			.toks
			.get(self.pos)
			.is_some_and(|t| t.kind == SyntaxKind::Ident)
			&& ddl::DDL_STARTERS.iter().any(|kw| self.at_kw(kw))
		{
			ddl::ddl_stmt(self)
		} else {
			Err(self.error("expected a statement"))
		}
	}

	/// Emit an `ErrorStatement` holding every token up to and including the
	/// next top-level `;`, and record the diagnostic. Top-level means:
	/// dollar-quoted bodies are already single tokens, and in SQLite a
	/// `BEGIN ... END` trigger body does not end the statement.
	pub(crate) fn error_statement(&mut self, error: StmtError) {
		let (start, end) = match self.toks.get(error.at) {
			Some(t) => (t.start, t.end),
			None => (self.eof, self.eof),
		};
		self.diagnostics.push(Diagnostic { message: error.message, start, end });

		self.start(SyntaxKind::ErrorStatement);
		let mut begin_depth = 0u32;
		while !self.at_eof() {
			if self.dialect == Dialect::Sqlite {
				if self.at_kw("begin") && !self.begin_is_transaction() {
					begin_depth += 1;
				} else if self.at_kw("end") {
					begin_depth = begin_depth.saturating_sub(1);
				}
			}
			let at_semicolon = self.at(SyntaxKind::Semicolon);
			self.bump();
			if at_semicolon && begin_depth == 0 {
				break;
			}
		}
		self.finish();
	}

	/// Distinguish SQLite `BEGIN [DEFERRED|IMMEDIATE|EXCLUSIVE]
	/// [TRANSACTION]` from a trigger body's `BEGIN stmt; ... END`.
	pub(crate) fn begin_is_transaction(&self) -> bool {
		self.nth_at(1, SyntaxKind::Semicolon)
			|| self.nth_at_kw(1, "transaction")
			|| self.nth_at_kw(1, "deferred")
			|| self.nth_at_kw(1, "immediate")
			|| self.nth_at_kw(1, "exclusive")
			|| self.nth_kind(1).is_none()
	}

	pub(crate) fn in_plpgsql(&self) -> bool {
		self.in_plpgsql
	}

	pub(crate) fn dialect(&self) -> Dialect {
		self.dialect
	}
}
