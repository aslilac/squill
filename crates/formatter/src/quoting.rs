//! Identifier quoting: the semantics-preserving transform applied at
//! render time.
//!
//! Postgres folds unquoted identifiers to lowercase (ASCII letters only), so
//! quotes may be stripped only from all-lowercase, identifier-shaped,
//! position-appropriate non-keywords. SQLite compares identifiers
//! ASCII-case-insensitively, so any identifier-shaped non-keyword may be
//! unquoted; backtick/bracket quoting is normalized to `"` when quoting is
//! kept. No regex anywhere: shapes are checked char-by-char.

use crate::IdentQuoting;
use crate::Options;
use crate::doc::IdentPos;
use crate::keywords::PgKeywordCategory;
use crate::keywords::is_sqlite_keyword;
use crate::keywords::pg_keyword_category;
use parser::Dialect;

/// The written form of an identifier token.
enum Form {
	Bare,
	DoubleQuoted,
	Backtick,
	Bracket,
}

pub(crate) fn render_ident(
	text: &str,
	pos: IdentPos,
	options: &Options,
) -> String {
	let Some((form, inner)) = classify(text) else {
		// Malformed (unterminated quote etc.) — pass through untouched.
		return text.to_string();
	};
	match options.quoting {
		IdentQuoting::AlwaysQuoted => match form {
			// Already double-quoted: exact name, keep byte-for-byte.
			Form::DoubleQuoted => text.to_string(),
			// SQLite alternative quoting: same exact name, normalized.
			Form::Backtick | Form::Bracket => quote_double(&inner),
			// A bare identifier denotes its *folded* name; quote that.
			Form::Bare => match options.dialect {
				Dialect::Postgres => quote_double(&text.to_ascii_lowercase()),
				// SQLite compares case-insensitively, so the written case
				// still denotes the same identifier.
				Dialect::Sqlite => quote_double(text),
			},
		},
		IdentQuoting::UnquotedWhenSafe => match form {
			// A bare name in call position denotes its folded name in
			// both dialects; write it folded so COALESCE / NOW / MAX
			// normalize like keywords do. Other positions (columns,
			// tables) keep the author's case.
			Form::Bare if pos == IdentPos::TypeOrFunction => {
				text.to_ascii_lowercase()
			}
			Form::Bare => text.to_string(),
			Form::DoubleQuoted if sqlite_string_or_name(pos, options) => {
				text.to_string()
			}
			Form::DoubleQuoted | Form::Backtick | Form::Bracket => {
				if can_strip(&inner, pos, options.dialect) {
					inner
				} else if matches!(form, Form::DoubleQuoted) {
					text.to_string()
				} else {
					quote_double(&inner)
				}
			}
		},
	}
}

/// Is a double-quoted name at `pos` one SQLite might read as a string
/// (when no column matches it), and so not squill's to unquote?
fn sqlite_string_or_name(pos: IdentPos, options: &Options) -> bool {
	pos == IdentPos::Expression && options.dialect == Dialect::Sqlite
}

/// The double-quoted `text`, a lone name in a SQLite expression, that
/// squill would otherwise unquote: kept as written, and worth a word,
/// since SQLite reads it as a string if no column matches.
pub(crate) fn kept_for_sqlite(text: &str, options: &Options) -> bool {
	options.quoting == IdentQuoting::UnquotedWhenSafe
		&& sqlite_string_or_name(IdentPos::Expression, options)
		&& matches!(classify(text), Some((Form::DoubleQuoted, inner))
			if can_strip(&inner, IdentPos::Expression, options.dialect))
}

/// Split an identifier token into its form and unescaped inner name.
fn classify(text: &str) -> Option<(Form, String)> {
	let mut chars = text.chars();
	match chars.next() {
		Some('"') => {
			let body = text.strip_prefix('"')?.strip_suffix('"')?;
			Some((Form::DoubleQuoted, body.replace("\"\"", "\"")))
		}
		Some('`') => {
			let body = text.strip_prefix('`')?.strip_suffix('`')?;
			Some((Form::Backtick, body.replace("``", "`")))
		}
		Some('[') => {
			let body = text.strip_prefix('[')?.strip_suffix(']')?;
			Some((Form::Bracket, body.to_string()))
		}
		Some(_) => Some((Form::Bare, text.to_string())),
		None => None,
	}
}

fn quote_double(name: &str) -> String {
	let mut out = String::with_capacity(name.len() + 2);
	out.push('"');
	for c in name.chars() {
		if c == '"' {
			out.push('"');
		}
		out.push(c);
	}
	out.push('"');
	out
}

fn can_strip(name: &str, pos: IdentPos, dialect: Dialect) -> bool {
	match dialect {
		Dialect::Postgres => {
			// Shape: [a-z_][a-z0-9_$]* — all-lowercase so folding the
			// bare form resolves to the same name.
			let mut chars = name.chars();
			let Some(first) = chars.next() else {
				return false;
			};
			if !(first.is_ascii_lowercase() || first == '_') {
				return false;
			}
			if !chars.all(|c| {
				c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '$'
			}) {
				return false;
			}
			match pg_keyword_category(name) {
				None | Some(PgKeywordCategory::Unreserved) => true,
				Some(PgKeywordCategory::ColName) => pos != IdentPos::TypeOrFunction,
				Some(PgKeywordCategory::TypeFuncName) => {
					pos == IdentPos::TypeOrFunction
				}
				Some(PgKeywordCategory::Reserved) => false,
			}
		}
		Dialect::Sqlite => {
			// Identifier-shaped in any ASCII case (SQLite compares
			// case-insensitively), and not a keyword.
			let mut chars = name.chars();
			let Some(first) = chars.next() else {
				return false;
			};
			if !(first.is_ascii_alphabetic() || first == '_') {
				return false;
			}
			if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$') {
				return false;
			}
			!is_sqlite_keyword(name)
		}
	}
}
