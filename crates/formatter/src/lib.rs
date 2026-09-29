//! Formatter: doc IR, renderer (TREE-96), and the SELECT formatting
//! rules with their per-statement safety check (TREE-97).

mod attr_order;
pub mod check;
mod col_order;
pub mod doc;
pub mod highlight;
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

/// Whether the last statement ends in a `;`. Only the last: the ones
/// between statements are what separate them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrailingSemicolons {
	/// Add one where it's missing: the norm for SQL files.
	#[default]
	Always,
	/// Drop it: the norm for SQL embedded in host code.
	None,
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
	/// Whether the last statement ends in a `;`.
	pub trailing_semicolons: TrailingSemicolons,
	/// Governs the identifier-quoting safety rules.
	pub dialect: Dialect,
	/// sqlc-style `@name` parameters (see [`LexOptions::at_params`]);
	/// used when re-lexing for the safety check.
	pub at_params: bool,
	/// Python DB-API `pyformat` parameters (`%s`, `%(name)s`; see
	/// [`LexOptions::pyformat_params`]); used when re-lexing for the
	/// safety check.
	pub pyformat_params: bool,
	/// JDBC-style `?` placeholders in Postgres (see
	/// [`LexOptions::question_params`]); used when re-lexing for the
	/// safety check.
	pub question_params: bool,
	/// `:name` placeholders in Postgres (see [`LexOptions::colon_params`]);
	/// used when re-lexing for the safety check.
	pub colon_params: bool,
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
			trailing_semicolons: TrailingSemicolons::default(),
			dialect: Dialect::default(),
			at_params: false,
			pyformat_params: false,
			question_params: false,
			colon_params: false,
			always_break_statements: false,
		}
	}
}

impl Options {
	pub fn lex_options(&self) -> LexOptions {
		LexOptions {
			at_params: self.at_params,
			pyformat_params: self.pyformat_params,
			question_params: self.question_params,
			colon_params: self.colon_params,
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
	/// Procedural bodies (`LANGUAGE plpgsql` / `LANGUAGE sql` / `DO`)
	/// that didn't parse, so were left as written.
	pub body_diagnostics: Vec<BodyDiagnostic>,
}

/// Where a procedural body failed to parse, in the source's bytes.
#[derive(Debug, Clone)]
pub struct BodyDiagnostic {
	pub start: usize,
	pub end: usize,
	pub message: String,
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
	// Each piece carries the number of blank lines that precede it.
	let mut pieces: Vec<(usize, String)> = Vec::new();
	let mut fallbacks = 0;
	let mut pending_blank = 0usize;
	// The last statement, whose `;` is `trailing_semicolons`' to settle.
	let last = cst.root().children().last();

	for element in cst.root().children_with_tokens() {
		match element {
			parser::syntax::SyntaxElement::Node(node) => {
				// Not inside procedural bodies, and never an unparsable one.
				let (settled, original) =
					if depth == 0 && node.kind() != SyntaxKind::ErrorStatement {
						settle_semicolon(node.clone(), options, last == Some(node))
					} else {
						(node.clone(), node.to_string())
					};
				let node = &settled;
				let blank = pending_blank.max(leading_blanks(&original));
				pending_blank = 0;
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
					pending_blank = pending_blank.max(blank_lines(token.text()));
				}
				SyntaxKind::LineComment | SyntaxKind::BlockComment => {
					pieces.push((pending_blank, token.text().to_string()));
					pending_blank = 0;
				}
				_ => {
					// Stray root-level tokens (shouldn't happen): keep.
					pieces.push((pending_blank, token.text().to_string()));
					pending_blank = 0;
				}
			},
		}
	}

	let mut out = String::new();
	for (index, (blank, piece)) in pieces.iter().enumerate() {
		if index > 0 {
			out.push('\n');
			for _ in 0..*blank {
				out.push('\n');
			}
		}
		out.push_str(piece);
	}
	if !out.is_empty() && !out.ends_with('\n') {
		out.push('\n');
	}
	// Bodies that don't parse are left as written by the splice; say so,
	// pointing into the source. Only at the top: nested bodies are
	// reported with the one that holds them.
	let mut body_diagnostics = Vec::new();
	if depth == 0 {
		for node in cst.root().children() {
			if node.kind() != SyntaxKind::ErrorStatement {
				collect_body_diagnostics(node, options, &mut body_diagnostics);
			}
		}
	}
	Formatted { text: out, fallback_statements: fallbacks, body_diagnostics }
}

/// A statement with its `;` where it belongs: moved up past any line
/// comment that pushed it onto a line of its own, and — for the last
/// statement — added (before any trailing comment) or dropped, per
/// `trailing_semicolons`. Re-parsed, so layout measures the statement as
/// it will be printed. Unchanged if it already agrees, or if the adjusted
/// text doesn't parse as one statement of the same kind.
fn settle_semicolon(
	node: parser::syntax::SyntaxNode,
	options: &Options,
	last: bool,
) -> (parser::syntax::SyntaxNode, String) {
	let original = node.to_string();
	let tokens =
		parser::lexer::lex_with(&original, options.dialect, options.lex_options());
	let mut offset = 0;
	let mut last_code = None;
	let mut before_last = None;
	let mut comment_since = false;
	for token in &tokens {
		if !token.kind.is_trivia() {
			before_last = last_code.map(|(at, token, _)| (at, token));
			last_code = Some((offset, token, comment_since));
			comment_since = false;
		} else if token.kind == SyntaxKind::LineComment {
			comment_since = true;
		}
		offset += token.text.len();
	}
	let Some((at, token, after_comment)) = last_code else {
		return (node, original);
	};
	let is_semicolon = token.kind == SyntaxKind::Semicolon;
	let adjusted = match (last, options.trailing_semicolons) {
		// A `;` a line comment pushed onto a line of its own moves back up
		// to the code it ends: `select 1 -- one` then `;` on its own line
		// becomes `select 1; -- one`.
		(_, TrailingSemicolons::Always) | (false, _)
			if is_semicolon && after_comment =>
		{
			let Some((prev_at, prev)) = before_last else {
				return (node, original);
			};
			let end = prev_at + prev.text.len();
			format!(
				"{};{}{}",
				&original[..end],
				&original[end..at],
				&original[at + 1..]
			)
		}
		(true, TrailingSemicolons::Always) if !is_semicolon => {
			let end = at + token.text.len();
			format!("{};{}", &original[..end], &original[end..])
		}
		(true, TrailingSemicolons::None) if is_semicolon => {
			format!("{}{}", &original[..at], &original[at + 1..])
		}
		_ => return (node, original),
	};
	let tokens =
		parser::lexer::lex_with(&adjusted, options.dialect, options.lex_options());
	let parse = parser::parser::parse(&tokens, options.dialect);
	let mut nodes = parse.cst.root().children();
	match (nodes.next(), nodes.next()) {
		// The whole text must stay in the one statement: a comment the
		// move leaves on a line of its own would parse outside it.
		(Some(settled), None)
			if settled.kind() == node.kind()
				&& parse.diagnostics.is_empty()
				&& settled.to_string().trim_end() == adjusted.trim_end() =>
		{
			(settled.clone(), adjusted)
		}
		_ => (node, original),
	}
}

/// The most blank lines squill will keep between two top-level pieces.
/// One is the ordinary paragraph break; a second is a section divider
/// authors use deliberately. Past that it is just drift, and collapses.
const MAX_BLANK_LINES: usize = 2;

/// How many blank lines a run of whitespace holds, capped at
/// [`MAX_BLANK_LINES`]. A single newline ends a line without leaving one
/// blank, so the count is one less than the newlines.
fn blank_lines(whitespace: &str) -> usize {
	whitespace.matches('\n').count().saturating_sub(1).min(MAX_BLANK_LINES)
}

/// How many blank lines the statement's own text begins with (before any
/// comment or code).
fn leading_blanks(original: &str) -> usize {
	let leading: String =
		original.chars().take_while(|c| c.is_whitespace()).collect();
	blank_lines(&leading)
}

/// Verbatim passthrough of a statement, trimmed of the surrounding
/// whitespace that statement assembly regenerates.
fn trim_verbatim(original: &str) -> String {
	original.trim().to_string()
}

/// Parse each procedural body in `statement` (as the splice would) and
/// record where any fails to.
fn collect_body_diagnostics(
	statement: &parser::syntax::SyntaxNode,
	options: &Options,
	out: &mut Vec<BodyDiagnostic>,
) {
	let tokens: Vec<(usize, parser::lexer::Token<'_>)> = statement
		.descendants_with_tokens()
		.filter_map(|element| element.into_token())
		.filter(|token| !token.kind().is_trivia())
		.map(|token| {
			let start = u32::from(token.text_range().start()) as usize;
			(start, parser::lexer::Token { kind: token.kind(), text: token.text() })
		})
		.collect();
	let non_trivia: Vec<&parser::lexer::Token<'_>> =
		tokens.iter().map(|(_, token)| token).collect();
	let Some(lang) = body_lang(&non_trivia) else {
		return;
	};
	let lex_options = options.lex_options();
	for (start, token) in &tokens {
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
		let (parse, what) = match lang {
			BodyLang::Sql => (
				parser::parser::parse(&body_tokens, options.dialect),
				"SQL function body",
			),
			BodyLang::Plpgsql => (
				parser::parser::parse_plpgsql_body(&body_tokens, options.dialect),
				"PL/pgSQL body",
			),
		};
		let base = start + tag.len();
		for diagnostic in &parse.diagnostics {
			out.push(BodyDiagnostic {
				start: base + diagnostic.start,
				end: base + diagnostic.end,
				message: format!("{} ({what} left as written)", diagnostic.message),
			});
		}
	}
}

/// Which body grammar a statement's dollar-quoted string holds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum BodyLang {
	Sql,
	Plpgsql,
}

/// The body grammar of a statement's dollar-quoted strings, if it has
/// procedural bodies at all: its `LANGUAGE sql` / `LANGUAGE plpgsql`
/// marker (position-independent; LANGUAGE may precede or follow AS), or
/// plpgsql for a `DO` statement without one.
pub(crate) fn body_lang(
	non_trivia: &[&parser::lexer::Token<'_>],
) -> Option<BodyLang> {
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
	marker.or_else(|| {
		non_trivia
			.first()
			.is_some_and(|t| t.text.eq_ignore_ascii_case("do"))
			.then_some(BodyLang::Plpgsql)
	})
}

/// Byte offsets of the lines in formatted `sql` whose leading whitespace
/// is part of a value: lines that begin inside a string, quoted
/// identifier, or dollar-quoted string spanning lines. Re-indenting
/// those (to anchor SQL inside a host file, say) would change the data.
/// Procedural bodies are the exception — their layout is the
/// formatter's own — though a value spanning lines inside one is not.
pub fn verbatim_line_starts(
	sql: &str,
	options: &Options,
) -> std::collections::HashSet<usize> {
	let mut out = std::collections::HashSet::new();
	collect_verbatim_lines(sql, 0, options, 0, &mut out);
	out
}

fn collect_verbatim_lines(
	sql: &str,
	base: usize,
	options: &Options,
	depth: u32,
	out: &mut std::collections::HashSet<usize>,
) {
	let tokens =
		parser::lexer::lex_with(sql, options.dialect, options.lex_options());
	let mut offsets = Vec::with_capacity(tokens.len());
	let mut offset = 0;
	for token in &tokens {
		offsets.push(offset);
		offset += token.text.len();
	}
	// Statement by statement, since the body marker is per statement.
	let mut start = 0;
	while start < tokens.len() {
		let end = tokens[start..]
			.iter()
			.position(|t| t.kind == SyntaxKind::Semicolon)
			.map_or(tokens.len(), |at| start + at + 1);
		let statement = &tokens[start..end];
		let non_trivia: Vec<_> =
			statement.iter().filter(|t| !t.kind.is_trivia()).collect();
		let lang = body_lang(&non_trivia);
		for (index, token) in statement.iter().enumerate() {
			if token.kind.is_trivia() || !token.text.contains('\n') {
				continue;
			}
			let at = base + offsets[start + index];
			if token.kind == SyntaxKind::DollarString
				&& lang.is_some()
				&& depth < MAX_BODY_DEPTH
				&& let Some((tag, content)) = check::split_dollar(token.text)
			{
				collect_verbatim_lines(
					content,
					at + tag.len(),
					options,
					depth + 1,
					out,
				);
				continue;
			}
			for (newline, _) in token.text.match_indices('\n') {
				out.insert(at + newline + 1);
			}
		}
		start = end;
	}
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

	let non_trivia: Vec<_> =
		tokens.iter().filter(|t| !t.kind.is_trivia()).collect();
	let Some(lang) = body_lang(&non_trivia) else {
		return statement.to_string();
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
		// Re-anchor the body against the line holding the opening tag,
		// with the closing tag on its own line at that indent. A
		// PL/pgSQL body carries its own `begin`/`end` bracketing, so it
		// sits flush with the tags; a bare SQL body has none, and gets
		// one indent unit to mark it off.
		let anchor = line_indent(statement, range.start);
		let unit = match lang {
			BodyLang::Plpgsql => String::new(),
			BodyLang::Sql => match options.indent_style {
				IndentStyle::Tab => "\t".to_string(),
				IndentStyle::Spaces => " ".repeat(usize::from(options.indent_width)),
			},
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
