//! Escapes in a captured string. The grammar says where each one is (an
//! `escape_sequence` node, or one the query captures as
//! `@squill.escape`); its query says which kinds the string's syntax
//! takes (`(#set! squill.escape "whitespace")`), and squill reads those,
//! by shape, only to read the SQL. Every escape is written back exactly
//! as it was spelled, never respelled or inserted: one between SQL
//! tokens is layout, which squill writes its own way, and one inside a
//! token comes back with the token.

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
	/// character, or a whole escape.
	spelled: Vec<Range<usize>>,
}

/// Why a string's escapes couldn't be read.
pub(crate) enum Unreadable {
	/// A backslash no escape covers: a raw backslash, in a string that
	/// takes escapes, or an escape the grammar didn't mark.
	Backslash,
	/// An escape none of the string's groups covers.
	Escape(String),
}

/// Read `content`, whose escapes are at `escapes` (sorted ranges within
/// it), taking those `groups` cover.
pub(crate) fn read(
	content: &str,
	escapes: &[Range<usize>],
	groups: &[Group],
) -> Result<Read, Unreadable> {
	let mut sql = String::with_capacity(content.len());
	let mut spelled = Vec::with_capacity(content.len());
	let mut escapes = escapes.iter().peekable();
	let mut at = 0;
	while at < content.len() {
		if let Some(escape) = escapes.next_if(|escape| escape.start == at) {
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
		if c == '\\' {
			return Err(Unreadable::Backslash);
		}
		for _ in 0..c.len_utf8() {
			spelled.push(at..at + c.len_utf8());
		}
		sql.push(c);
		at += c.len_utf8();
	}
	Ok(Read { sql, spelled })
}

impl Read {
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
		// A token spelled with an escape that the formatting changed (a
		// keyword's case) or dropped.
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

	#[test]
	fn escapes_inside_tokens_keep_their_spelling() {
		let content = "SELECT 'a\\tb',\\n\\\"Weird\\\" FROM t";
		let escapes = [9..11, 14..16, 16..18, 23..25];
		let read = read(content, &escapes, &all()).ok().expect("read");
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
		let read =
			read(content, std::slice::from_ref(&(3..7)), &all()).ok().expect("read");
		assert_eq!(read.sql, "SELECT 1");
		assert!(
			read
				.respell(content, "select 1", Dialect::Postgres, LexOptions::default())
				.is_none()
		);
	}

	#[test]
	fn a_backslash_no_escape_covers_is_unreadable() {
		assert!(matches!(read("a \\ b", &[], &all()), Err(Unreadable::Backslash)));
		assert!(matches!(
			read("a \\q b", std::slice::from_ref(&(2..4)), &all()),
			Err(Unreadable::Escape(escape)) if escape == "\\q"
		));
	}

	#[test]
	fn a_trimming_continuation_takes_the_indentation_with_it() {
		let content = "select \\\n    1";
		let read = read(
			content,
			std::slice::from_ref(&(7..9)),
			&[Group::LineContinuationTrim],
		)
		.ok()
		.expect("read");
		assert_eq!(read.sql, "select 1");
	}
}
