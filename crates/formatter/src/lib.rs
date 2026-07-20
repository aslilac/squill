//! Formatter: doc IR, renderer (TREE-96), and the SELECT formatting
//! rules with their per-statement safety check (TREE-97).

mod attr_order;
pub mod check;
pub mod doc;
pub mod keywords;
mod printer;
mod quoting;
mod rules;

pub use doc::Doc;
pub use doc::IdentPos;
use parser::Dialect;
use parser::lexer::LexOptions;
use parser::parser::Cst;
use parser::syntax::SyntaxKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
	#[default]
	Tab,
	Spaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordCase {
	#[default]
	Lower,
	Upper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdentQuoting {
	#[default]
	UnquotedWhenSafe,
	AlwaysQuoted,
}

/// The complete configuration surface of the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
	/// Tabs (default) or spaces.
	pub indent_style: IndentStyle,
	/// Width of one indent level: the space count in spaces mode, and the
	/// measured width of a tab in tab mode. Default 2.
	pub indent_width: u8,
	/// Target maximum line width; a group breaks when its flat layout
	/// would overrun this. Single tokens longer than the width still
	/// overrun. Default 80.
	pub max_width: u16,
	pub keyword_case: KeywordCase,
	pub quoting: IdentQuoting,
	/// Governs the identifier-quoting safety rules.
	pub dialect: Dialect,
	/// sqlc-style `@name` parameters (see [`LexOptions::at_params`]);
	/// used when re-lexing for the safety check.
	pub at_params: bool,
	/// Python DB-API `pyformat` parameters (`%s`, `%(name)s`); set by
	/// embedding for Python hosts. Not part of the CLI/config surface.
	pub pyformat_params: bool,
	/// Never collapse a statement onto one line (clause-per-line even
	/// when it would fit). Used by embedding for multi-line string
	/// literals, where the author already chose a vertical layout. Not
	/// part of the CLI/config surface.
	pub always_break_statements: bool,
}

impl Default for Options {
	fn default() -> Self {
		Options {
			indent_style: IndentStyle::default(),
			indent_width: 2,
			max_width: 80,
			keyword_case: KeywordCase::default(),
			quoting: IdentQuoting::default(),
			dialect: Dialect::default(),
			at_params: false,
			pyformat_params: false,
			always_break_statements: false,
		}
	}
}

impl Options {
	pub fn lex_options(&self) -> LexOptions {
		LexOptions {
			at_params: self.at_params,
			pyformat_params: self.pyformat_params,
		}
	}
}

/// Render a document to text.
pub fn render(doc: &Doc, options: &Options) -> String {
	printer::render(doc, options)
}

/// The result of formatting a CST.
#[derive(Debug)]
pub struct Formatted {
	pub text: String,
	/// Statements the rules could format but whose output failed the
	/// safety check and fell back to verbatim passthrough. Zero for a
	/// fully formatted file. ErrorStatements are always verbatim and are
	/// not counted here.
	pub fallback_statements: usize,
}

/// Format a CST. Statement-by-statement: each formatted statement is
/// re-lexed and compared against its input (token equivalence + comment
/// conservation); on any mismatch the original text passes through
/// verbatim, so output is never less correct than its input.
pub fn format_cst(cst: &Cst, options: &Options) -> Formatted {
	format_cst_at(cst, options, 0)
}

/// Recursion bound for procedural bodies nested inside procedural bodies.
const MAX_BODY_DEPTH: u32 = 3;

fn format_cst_at(cst: &Cst, options: &Options, depth: u32) -> Formatted {
	let lex_options = options.lex_options();
	let mut pieces: Vec<(bool, String)> = Vec::new();
	let mut fallbacks = 0;
	let mut pending_blank = false;

	for element in cst.root().children_with_tokens() {
		match element {
			parser::syntax::SyntaxElement::Node(node) => {
				let original = node.to_string();
				let blank = pending_blank || leading_blank(&original);
				pending_blank = false;
				match rules::lower_statement(node, options.always_break_statements) {
					Some(doc) => {
						let rendered = render(&doc, options);
						let safe = check::tokens_equivalent(
							&original,
							&rendered,
							options.dialect,
							lex_options,
						) && check::comments_conserved(
							&original,
							&rendered,
							options.dialect,
							lex_options,
						);
						if safe {
							// Statement assembly owns inter-statement
							// newlines; drop any the doc produced (e.g. a
							// trailing comment's fresh line).
							let piece = rendered.trim_end().to_string();
							// TREE-101: recursively format `LANGUAGE sql`
							// procedural bodies inside the statement. If a
							// body changed shape, re-lay the statement once
							// so line measurement sees the real multi-line
							// body (this is the fixed point).
							let spliced = splice_sql_bodies(&piece, options, depth);
							let piece = if spliced != piece {
								relayout_statement(&spliced, options, depth).unwrap_or(spliced)
							} else {
								spliced
							};
							pieces.push((blank, piece));
						} else {
							fallbacks += 1;
							if std::env::var_os("SQUILL_DEBUG").is_some() {
								eprintln!(
									"== fallback ==\n-- original --\n{original}\n-- rendered --\n{rendered}\n=="
								);
							}
							pieces.push((blank, trim_verbatim(&original)));
						}
					}
					None => pieces.push((blank, trim_verbatim(&original))),
				}
			}
			parser::syntax::SyntaxElement::Token(token) => match token.kind() {
				SyntaxKind::Whitespace => {
					if token.text().matches('\n').count() >= 2 {
						pending_blank = true;
					}
				}
				SyntaxKind::LineComment | SyntaxKind::BlockComment => {
					pieces.push((pending_blank, token.text().to_string()));
					pending_blank = false;
				}
				_ => {
					// Stray root-level tokens (shouldn't happen): keep.
					pieces.push((pending_blank, token.text().to_string()));
					pending_blank = false;
				}
			},
		}
	}

	let mut out = String::new();
	for (index, (blank, piece)) in pieces.iter().enumerate() {
		if index > 0 {
			out.push('\n');
			if *blank {
				out.push('\n');
			}
		}
		out.push_str(piece);
	}
	if !out.is_empty() && !out.ends_with('\n') {
		out.push('\n');
	}
	Formatted { text: out, fallback_statements: fallbacks }
}

/// Does the statement's own text begin with a blank line (before any
/// comment or code)?
fn leading_blank(original: &str) -> bool {
	let leading: String =
		original.chars().take_while(|c| c.is_whitespace()).collect();
	leading.matches('\n').count() >= 2
}

/// Verbatim passthrough of a statement, trimmed of the surrounding
/// whitespace that statement assembly regenerates.
fn trim_verbatim(original: &str) -> String {
	original.trim().to_string()
}

/// Which body grammar a statement's dollar-quoted string holds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum BodyLang {
	Sql,
	Plpgsql,
}

/// Recursively format procedural dollar-quoted bodies inside a rendered
/// statement, re-anchoring them to the statement's indentation.
/// `LANGUAGE sql` bodies use the SQL grammar (TREE-101); `LANGUAGE
/// plpgsql` bodies and `DO` blocks (plpgsql by default) use the PL/pgSQL
/// grammar (TREE-103). Every splice is individually guarded: a body that
/// fails to parse, trips the self-check, or would collide with its own
/// tag is left untouched.
fn splice_sql_bodies(statement: &str, options: &Options, depth: u32) -> String {
	if depth >= MAX_BODY_DEPTH {
		return statement.to_string();
	}
	let lex_options = options.lex_options();
	let tokens = parser::lexer::lex_with(statement, options.dialect, lex_options);

	// The statement's language marker: `LANGUAGE sql` / `LANGUAGE
	// plpgsql` (position-independent; LANGUAGE may precede or follow AS).
	// A `DO` statement without a marker defaults to plpgsql.
	let non_trivia: Vec<_> =
		tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
	let marker =
		non_trivia.iter().zip(non_trivia.iter().skip(1)).find_map(|(a, b)| {
			if a.kind == SyntaxKind::Ident
				&& a.text.eq_ignore_ascii_case("language")
				&& b.kind == SyntaxKind::Ident
			{
				if b.text.eq_ignore_ascii_case("sql") {
					Some(BodyLang::Sql)
				} else if b.text.eq_ignore_ascii_case("plpgsql") {
					Some(BodyLang::Plpgsql)
				} else {
					None
				}
			} else {
				None
			}
		});
	let lang = match marker {
		Some(lang) => lang,
		None
			if non_trivia
				.first()
				.is_some_and(|t| t.text.eq_ignore_ascii_case("do")) =>
		{
			BodyLang::Plpgsql
		}
		None => return statement.to_string(),
	};

	// Collect (offset, token) for dollar-quoted bodies.
	let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
	let mut offset = 0;
	for token in &tokens {
		let range = offset..offset + token.text.len();
		offset = range.end;
		if token.kind != SyntaxKind::DollarString {
			continue;
		}
		let Some((tag, content)) = check::split_dollar(token.text) else {
			continue;
		};
		if content.trim().is_empty() {
			continue;
		}
		let body_tokens =
			parser::lexer::lex_with(content, options.dialect, lex_options);
		let parse = match lang {
			BodyLang::Sql => parser::parser::parse(&body_tokens, options.dialect),
			BodyLang::Plpgsql => {
				parser::parser::parse_plpgsql_body(&body_tokens, options.dialect)
			}
		};
		if !parse.diagnostics.is_empty() {
			continue; // does not parse cleanly: leave byte-identical
		}
		let formatted = format_cst_at(&parse.cst, options, depth + 1);
		if formatted.fallback_statements > 0 {
			continue;
		}
		let body = formatted.text.trim_end();
		// Belt-and-suspenders: the reformatted body must still be the
		// same SQL, and must not collide with its own closing tag.
		if body.contains(tag)
			|| !check::tokens_equivalent(content, body, options.dialect, lex_options)
			|| !check::comments_conserved(content, body, options.dialect, lex_options)
		{
			continue;
		}
		// Re-anchor: body lines one indent unit under the line holding
		// the opening tag; closing tag on its own line at that indent.
		let anchor = line_indent(statement, range.start);
		let unit = match options.indent_style {
			IndentStyle::Tab => "\t".to_string(),
			IndentStyle::Spaces => " ".repeat(usize::from(options.indent_width)),
		};
		let mut replacement = String::from(tag);
		for line in body.split('\n') {
			replacement.push('\n');
			if !line.is_empty() {
				replacement.push_str(&anchor);
				replacement.push_str(&unit);
				replacement.push_str(line);
			}
		}
		replacement.push('\n');
		replacement.push_str(&anchor);
		replacement.push_str(tag);
		if replacement != token.text {
			edits.push((range, replacement));
		}
	}

	let mut out = statement.to_string();
	for (range, replacement) in edits.into_iter().rev() {
		out.replace_range(range, &replacement);
	}
	out
}

/// Re-parse and re-render a single spliced statement so group layout
/// accounts for the (now multi-line) body token. Falls back to the input
/// on any surprise, and re-checks safety on the result.
fn relayout_statement(
	statement: &str,
	options: &Options,
	depth: u32,
) -> Option<String> {
	let lex_options = options.lex_options();
	let tokens = parser::lexer::lex_with(statement, options.dialect, lex_options);
	let parse = parser::parser::parse(&tokens, options.dialect);
	if !parse.diagnostics.is_empty() {
		return None;
	}
	let mut nodes = parse.cst.root().children();
	let node = nodes.next()?;
	if nodes.next().is_some() {
		return None; // expected exactly one statement
	}
	let doc = rules::lower_statement(node, options.always_break_statements)?;
	let rendered = render(&doc, options);
	let piece = rendered.trim_end().to_string();
	if !check::tokens_equivalent(statement, &piece, options.dialect, lex_options)
		|| !check::comments_conserved(
			statement,
			&piece,
			options.dialect,
			lex_options,
		) {
		return None;
	}
	// Bodies are already formatted; this splice only re-anchors and is
	// expected to be a no-op or a stable rewrite.
	Some(splice_sql_bodies(&piece, options, depth))
}

/// Leading whitespace of the line containing `offset`.
fn line_indent(source: &str, offset: usize) -> String {
	let line_start = source[..offset].rfind('\n').map_or(0, |pos| pos + 1);
	source[line_start..].chars().take_while(|&c| c == ' ' || c == '\t').collect()
}

/// Dev-tool access to the statement lowering (see examples/).
#[doc(hidden)]
pub fn debug_lower(node: &parser::syntax::SyntaxNode) -> Option<Doc> {
	rules::lower_statement(node, false)
}
