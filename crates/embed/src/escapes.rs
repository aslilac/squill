//! Escapes in a captured string. The grammar says where each one is (an
//! `escape_sequence` node, or one the query captures as
//! `@squill.escape`); its query says which kinds the string's syntax
//! takes (`(#set! squill.escape "whitespace")`), and squill reads those,
//! by shape, only to read the SQL. Every escape is written back exactly
//! as it was spelled, never respelled or inserted: one between SQL
//! tokens is layout, which squill writes its own way, and one inside a
//! token comes back with the token.
//!
//! An interpolation the query captures as `@squill.parameter` is read
//! the same way: as a parameter exactly as wide as the hole (`${user.id}`
//! reads as `$000000003`), so the SQL lays out at its real width, and the
//! hole comes back as it was spelled.

use std::collections::HashMap;
use std::collections::VecDeque;
use std::ops::Range;

use parser::Dialect;
use parser::lexer::LexOptions;
use parser::syntax::SyntaxKind;

/// A kind of escape a string syntax takes, as `squill.escape` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Group {
	/// `\n`, `\r`, `\t`.
	Whitespace,
	/// `\\`, `\"`, `\'`: each the character itself.
	Punctuation,
	/// `\$`, a `$` (JS templates, Kotlin).
	Dollar,
	/// `` \` ``, a backtick (JS templates).
	Backtick,
	/// `\xHH`, two hex digits.
	Hex,
	/// `\uXXXX`, four hex digits.
	Unicode4,
	/// `\u{XXXX}`, hex digits in braces.
	UnicodeBraced,
	/// `\UXXXXXXXX`, eight hex digits.
	Unicode8,
	/// `\NNN`, one to three octal digits (so `\0` too).
	Octal,
	/// A backslash ending a line: the line break isn't in the string.
	LineContinuation,
	/// Likewise, and neither is the next line's leading whitespace (Rust).
	LineContinuationTrim,
}

impl Group {
	pub(crate) const NAMES: &[(&str, Group)] = &[
		("whitespace", Group::Whitespace),
		("punctuation", Group::Punctuation),
		("\\$", Group::Dollar),
		("\\`", Group::Backtick),
		("\\xHH", Group::Hex),
		("\\uXXXX", Group::Unicode4),
		("\\u{XXXX}", Group::UnicodeBraced),
		("\\UXXXXXXXX", Group::Unicode8),
		("\\NNN", Group::Octal),
		("line-continuation", Group::LineContinuation),
		("line-continuation-trim", Group::LineContinuationTrim),
	];

	pub(crate) fn from_name(name: &str) -> Option<Group> {
		Group::NAMES.iter().find(|(n, _)| *n == name).map(|&(_, group)| group)
	}
}

/// What one escape, its whole text as the grammar delimits it, stands
/// for, if one of `groups` covers its shape.
fn meaning(escape: &str, groups: &[Group]) -> Option<String> {
	let rest = escape.strip_prefix('\\')?;
	let takes = |group| groups.contains(&group);
	let hex = |digits: &str, count: Option<usize>| -> Option<char> {
		let valid = !digits.is_empty()
			&& count.is_none_or(|n| digits.len() == n)
			&& digits.chars().all(|c| c.is_ascii_hexdigit());
		valid
			.then(|| u32::from_str_radix(digits, 16).ok())
			.flatten()?
			.try_into()
			.ok()
	};
	let single = |c: char| Some(c.to_string());
	match rest {
		"n" if takes(Group::Whitespace) => single('\n'),
		"r" if takes(Group::Whitespace) => single('\r'),
		"t" if takes(Group::Whitespace) => single('\t'),
		"\\" | "\"" | "'" if takes(Group::Punctuation) => Some(rest.to_string()),
		"$" if takes(Group::Dollar) => single('$'),
		"`" if takes(Group::Backtick) => single('`'),
		"\n" | "\r\n"
			if takes(Group::LineContinuation)
				|| takes(Group::LineContinuationTrim) =>
		{
			Some(String::new())
		}
		_ => {
			if let Some(digits) = rest.strip_prefix('x') {
				takes(Group::Hex).then(|| hex(digits, Some(2)).map(String::from))?
			} else if let Some(braced) = rest.strip_prefix("u{") {
				let digits = braced.strip_suffix('}')?;
				takes(Group::UnicodeBraced)
					.then(|| hex(digits, None).map(String::from))?
			} else if let Some(digits) = rest.strip_prefix('u') {
				takes(Group::Unicode4)
					.then(|| hex(digits, Some(4)).map(String::from))?
			} else if let Some(digits) = rest.strip_prefix('U') {
				takes(Group::Unicode8)
					.then(|| hex(digits, Some(8)).map(String::from))?
			} else if takes(Group::Octal)
				&& (1..=3).contains(&rest.len())
				&& rest.chars().all(|c| c.is_digit(8))
			{
				let value = u32::from_str_radix(rest, 8).ok()?;
				(value <= 0o377)
					.then(|| char::from_u32(value))
					.flatten()
					.map(String::from)
			} else {
				None
			}
		}
	}
}

/// A string's content read as SQL, with where each of its bytes was
/// spelled in the content.
pub(crate) struct Read {
	pub(crate) sql: String,
	/// Per byte of `sql`, the span of the content that spells it: one
	/// character, a whole escape, or a whole hole.
	spelled: Vec<Range<usize>>,
	/// The parameters the holes read as, in order, and the holes.
	placeholders: Vec<String>,
	holes: Vec<String>,
}

/// How to read a string's content: where its escapes and holes are
/// (sorted ranges within it), and what its syntax takes.
pub(crate) struct Reading<'a> {
	pub(crate) escapes: &'a [Range<usize>],
	pub(crate) holes: &'a [Range<usize>],
	pub(crate) groups: &'a [Group],
	/// No escapes: a backslash is just a backslash.
	pub(crate) raw: bool,
	/// What a hole reads as a parameter of: `$` (Postgres) or `?` (SQLite).
	pub(crate) sigil: char,
}

/// Why a string couldn't be read.
pub(crate) enum Unreadable {
	/// A backslash no escape covers: a raw backslash, in a string that
	/// takes escapes, or an escape the grammar didn't mark.
	Backslash,
	/// An escape none of the string's groups covers.
	Escape(String),
	/// An interpolation spanning lines, which squill doesn't lay out.
	HoleSpansLines,
}

/// The parameter each hole reads as: the sigil and a zero-padded number,
/// as wide as the hole where its number fits, and spelled nowhere else in
/// the content.
fn placeholders(
	content: &str,
	holes: &[Range<usize>],
	sigil: char,
) -> Vec<String> {
	let mut first = 1;
	loop {
		let made: Vec<String> = holes
			.iter()
			.enumerate()
			.map(|(index, hole)| {
				let width = content[hole.clone()].chars().count();
				let number = (first + index).to_string();
				format!("{sigil}{number:0>pad$}", pad = width.saturating_sub(1))
			})
			.collect();
		if !made.iter().any(|placeholder| content.contains(placeholder.as_str())) {
			return made;
		}
		first += holes.len();
	}
}

/// Read `content` as SQL, as `reading` says.
pub(crate) fn read(
	content: &str,
	reading: &Reading<'_>,
) -> Result<Read, Unreadable> {
	let groups = reading.groups;
	if reading.holes.iter().any(|hole| content[hole.clone()].contains('\n')) {
		return Err(Unreadable::HoleSpansLines);
	}
	let placeholders = placeholders(content, reading.holes, reading.sigil);
	let mut sql = String::with_capacity(content.len());
	let mut spelled = Vec::with_capacity(content.len());
	// An escape inside a hole is the host's, not the SQL's.
	let mut escapes = reading
		.escapes
		.iter()
		.filter(|escape| {
			!reading
				.holes
				.iter()
				.any(|hole| hole.start < escape.end && escape.start < hole.end)
		})
		.peekable();
	let mut holes = reading.holes.iter().zip(&placeholders).peekable();
	let mut at = 0;
	while at < content.len() {
		if let Some((hole, placeholder)) =
			holes.next_if(|(hole, _)| hole.start == at)
		{
			for _ in 0..placeholder.len() {
				spelled.push(hole.clone());
			}
			sql.push_str(placeholder);
			at = hole.end;
			continue;
		}
		if !reading.raw
			&& let Some(escape) = escapes.next_if(|escape| escape.start == at)
		{
			let text = &content[escape.clone()];
			let decoded = meaning(text, groups)
				.ok_or_else(|| Unreadable::Escape(text.to_string()))?;
			let mut end = escape.end;
			if groups.contains(&Group::LineContinuationTrim) && decoded.is_empty() {
				end += content[end..]
					.find(|c: char| c != ' ' && c != '\t')
					.unwrap_or(content.len() - end);
			}
			for _ in 0..decoded.len() {
				spelled.push(at..end);
			}
			sql.push_str(&decoded);
			at = end;
			continue;
		}
		let c = content[at..].chars().next().expect("in bounds");
		if c == '\\' && !reading.raw {
			return Err(Unreadable::Backslash);
		}
		for _ in 0..c.len_utf8() {
			spelled.push(at..at + c.len_utf8());
		}
		sql.push(c);
		at += c.len_utf8();
	}
	let holes = reading
		.holes
		.iter()
		.map(|hole| content[hole.clone()].to_string())
		.collect();
	Ok(Read { sql, spelled, placeholders, holes })
}

impl Read {
	/// `message`, about the SQL as read, with each hole as it was written
	/// rather than the parameter it read as.
	pub(crate) fn unread(&self, message: &str) -> String {
		self.placeholders.iter().zip(&self.holes).fold(
			message.to_string(),
			|message, (placeholder, hole)| {
				message.replace(placeholder.as_str(), hole)
			},
		)
	}

	/// Does a hole read as a parameter of its own sit right against SQL
	/// text (`${a}${b}`, `${x}abc`)? Then where it ends is the host's
	/// business, and squill can't move what's around it. A hole inside a
	/// token (`'${x}'`, `t_${x}`) comes back with the token, so is fine.
	pub(crate) fn glued(
		&self,
		dialect: Dialect,
		lex_options: LexOptions,
	) -> bool {
		let word = |c: Option<char>| {
			c.is_some_and(|c| c.is_alphanumeric() || "_$?'\"`".contains(c))
		};
		let mut at = 0;
		for token in parser::lexer::lex_with(&self.sql, dialect, lex_options) {
			let range = at..at + token.text.len();
			at = range.end;
			if self.placeholders.iter().any(|placeholder| placeholder == token.text)
				&& (word(self.sql[..range.start].chars().next_back())
					|| word(self.sql[range.end..].chars().next()))
			{
				return true;
			}
		}
		false
	}

	/// `formatted`, SQL that differs from what was read only in layout
	/// (and keyword case, quoting, a final `;`), with every token spelled
	/// the way the content spelled it. `None` when a token holding an
	/// escape didn't come through unchanged, so its escape can't be.
	pub(crate) fn respell(
		&self,
		content: &str,
		formatted: &str,
		dialect: Dialect,
		lex_options: LexOptions,
	) -> Option<String> {
		// Each token's spellings, in order, by its text.
		let mut spellings: HashMap<&str, VecDeque<&str>> = HashMap::new();
		let mut at = 0;
		for token in parser::lexer::lex_with(&self.sql, dialect, lex_options) {
			let range = at..at + token.text.len();
			at = range.end;
			if token.kind == SyntaxKind::Whitespace || range.is_empty() {
				continue;
			}
			let spelling = &content
				[self.spelled[range.start].start..self.spelled[range.end - 1].end];
			spellings.entry(token.text).or_default().push_back(spelling);
		}
		let mut out = String::with_capacity(formatted.len());
		for token in parser::lexer::lex_with(formatted, dialect, lex_options) {
			match spellings.get_mut(token.text).and_then(VecDeque::pop_front) {
				Some(spelling) if token.kind != SyntaxKind::Whitespace => {
					out.push_str(spelling);
				}
				_ => out.push_str(token.text),
			}
		}
		// A token spelled with an escape or a hole that the formatting
		// changed (a keyword's case) or dropped.
		let lost = spellings
			.iter()
			.any(|(text, left)| left.iter().any(|spelling| spelling != text));
		(!lost).then_some(out)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn all() -> Vec<Group> {
		Group::NAMES.iter().map(|&(_, group)| group).collect()
	}

	#[test]
	fn shapes_mean_what_they_say() {
		let groups = all();
		for (escape, meant) in [
			("\\n", "\n"),
			("\\t", "\t"),
			("\\\"", "\""),
			("\\\\", "\\"),
			("\\x41", "A"),
			("\\u00e9", "é"),
			("\\u{1F600}", "😀"),
			("\\U0001F600", "😀"),
			("\\0", "\0"),
			("\\101", "A"),
			("\\\n", ""),
		] {
			assert_eq!(meaning(escape, &groups).as_deref(), Some(meant), "{escape}");
		}
		assert_eq!(meaning("\\q", &groups), None);
		assert_eq!(meaning("\\x4", &groups), None);
		assert_eq!(meaning("\\n", &[Group::Hex]), None);
	}

	/// `content`'s SQL, its escapes at `escapes`, taking `groups`.
	fn read_escapes(
		content: &str,
		escapes: &[Range<usize>],
		groups: &[Group],
	) -> Result<Read, Unreadable> {
		let reading =
			Reading { escapes, holes: &[], groups, raw: false, sigil: '$' };
		read(content, &reading)
	}

	#[test]
	fn escapes_inside_tokens_keep_their_spelling() {
		let content = "SELECT 'a\\tb',\\n\\\"Weird\\\" FROM t";
		let escapes = [9..11, 14..16, 16..18, 23..25];
		let read = read_escapes(content, &escapes, &all()).ok().expect("read");
		assert_eq!(read.sql, "SELECT 'a\tb',\n\"Weird\" FROM t");
		let formatted = "select 'a\tb', \"Weird\"\nfrom t";
		assert_eq!(
			read
				.respell(content, formatted, Dialect::Postgres, LexOptions::default())
				.as_deref(),
			Some("select 'a\\tb', \\\"Weird\\\"\nfrom t")
		);
	}

	#[test]
	fn a_token_whose_escape_cant_come_through_declines() {
		let content = "SEL\\x45CT 1";
		let read = read_escapes(content, std::slice::from_ref(&(3..7)), &all())
			.ok()
			.expect("read");
		assert_eq!(read.sql, "SELECT 1");
		assert!(
			read
				.respell(content, "select 1", Dialect::Postgres, LexOptions::default())
				.is_none()
		);
	}

	#[test]
	fn a_backslash_no_escape_covers_is_unreadable() {
		assert!(matches!(
			read_escapes("a \\ b", &[], &all()),
			Err(Unreadable::Backslash)
		));
		assert!(matches!(
			read_escapes("a \\q b", std::slice::from_ref(&(2..4)), &all()),
			Err(Unreadable::Escape(escape)) if escape == "\\q"
		));
	}

	#[test]
	fn a_trimming_continuation_takes_the_indentation_with_it() {
		let content = "select \\\n    1";
		let read = read_escapes(
			content,
			std::slice::from_ref(&(7..9)),
			&[Group::LineContinuationTrim],
		)
		.ok()
		.expect("read");
		assert_eq!(read.sql, "select 1");
	}

	fn read_holes(content: &str, holes: &[Range<usize>], sigil: char) -> Read {
		let reading =
			Reading { escapes: &[], holes, groups: &[], raw: true, sigil };
		read(content, &reading).ok().expect("read")
	}

	#[test]
	fn holes_read_as_parameters_as_wide_as_they_are() {
		let content = "SELECT ${a} FROM t WHERE id = ${user.id} AND x = '${x}'";
		let read = read_holes(content, &[7..11, 30..40, 50..54], '$');
		assert_eq!(
			read.sql,
			"SELECT $001 FROM t WHERE id = $000000002 AND x = '$003'"
		);
		assert!(!read.glued(Dialect::Postgres, LexOptions::default()));
		let formatted = "select $001\nfrom t\nwhere id = $000000002 and x = '$003'";
		assert_eq!(
			read
				.respell(content, formatted, Dialect::Postgres, LexOptions::default())
				.as_deref(),
			Some("select ${a}\nfrom t\nwhere id = ${user.id} and x = '${x}'")
		);
		// SQLite's parameters, and numbers no other parameter has.
		let read =
			read_holes("SELECT ?01, {a}", std::slice::from_ref(&(12..15)), '?');
		assert_eq!(read.sql, "SELECT ?01, ?02");
	}

	#[test]
	fn holes_against_sql_text_are_glued() {
		let content = "SELECT ${a}${b}, ${c}d FROM t";
		let read = read_holes(content, &[7..11, 11..15, 17..21], '$');
		assert!(read.glued(Dialect::Postgres, LexOptions::default()));
		let read =
			read_holes("SELECT t_${x} FROM t", std::slice::from_ref(&(9..13)), '$');
		assert!(!read.glued(Dialect::Postgres, LexOptions::default()));
	}
}
