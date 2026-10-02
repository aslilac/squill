//! What each token of SQL source is, for editors that color it (the
//! language server's semantic tokens).
//!
//! A word is a keyword exactly when formatting would re-case it: the
//! rules lower it as a keyword, and it is in the dialect's keyword
//! tables. So a column called `name` or `type` reads as a name, the way
//! it formats. Tree context comes first, though: a word in a type is a
//! type, and a function's name is a function. A statement that doesn't
//! parse, as most don't while they're being typed, has no rules to ask,
//! so any word in the keyword tables is a keyword there.

use std::collections::HashSet;
use std::ops::Range;

use parser::syntax::SyntaxKind;
use parser::syntax::SyntaxNode;
use parser::syntax::SyntaxToken;

use crate::Options;
use crate::doc::Doc;
use crate::printer::cases_as_keyword;
use crate::rules;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
	Keyword,
	/// A table, column, alias, or any other name.
	Name,
	Function,
	Type,
	String,
	Number,
	Parameter,
	Operator,
	Comment,
}

/// One highlighted token, as a byte range into the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Highlight {
	pub range: Range<usize>,
	pub kind: HighlightKind,
}

/// Every token of `source` worth coloring, in order. Whitespace and
/// punctuation are left out.
pub fn highlight(source: &str, options: &Options) -> Vec<Highlight> {
	let tokens =
		parser::lexer::lex_with(source, options.dialect, options.lex_options());
	let parse = parser::parser::parse(&tokens, options.dialect);
	let root = parse.cst.root();
	let keywords = keyword_offsets(root, options.dialect);
	root
		.descendants_with_tokens()
		.filter_map(|element| element.into_token())
		.filter_map(|token| {
			let kind = classify(token, &keywords)?;
			let range = token.text_range();
			let range =
				u32::from(range.start()) as usize..u32::from(range.end()) as usize;
			Some(Highlight { range, kind })
		})
		.collect()
}

/// Where each word that formatting would case as a keyword starts.
fn keyword_offsets(
	root: &SyntaxNode,
	dialect: parser::Dialect,
) -> HashSet<usize> {
	let mut offsets = HashSet::new();
	for statement in root.children() {
		match rules::lower_statement(statement) {
			Some(doc) => {
				let mut stack = vec![&doc];
				while let Some(doc) = stack.pop() {
					match doc {
						Doc::Keyword { text, offset } => {
							if cases_as_keyword(text, dialect) {
								offsets.insert(*offset);
							}
						}
						Doc::Concat(items) | Doc::Fill(items) => stack.extend(items),
						Doc::Group(inner) | Doc::Indent(inner) => stack.push(inner),
						Doc::IfBreak { broken, flat } => {
							stack.push(broken);
							stack.push(flat);
						}
						_ => {}
					}
				}
			}
			None => {
				let words = statement
					.descendants_with_tokens()
					.filter_map(|element| element.into_token())
					.filter(|token| token.kind() == SyntaxKind::Ident)
					.filter(|token| cases_as_keyword(token.text(), dialect));
				offsets.extend(
					words.map(|token| u32::from(token.text_range().start()) as usize),
				);
			}
		}
	}
	offsets
}

fn classify(
	token: &SyntaxToken,
	keywords: &HashSet<usize>,
) -> Option<HighlightKind> {
	let kind = match token.kind() {
		SyntaxKind::LineComment | SyntaxKind::BlockComment => {
			HighlightKind::Comment
		}
		SyntaxKind::String
		| SyntaxKind::EscapeString
		| SyntaxKind::UnicodeString
		| SyntaxKind::BitString
		| SyntaxKind::HexString
		| SyntaxKind::DollarString => HighlightKind::String,
		SyntaxKind::Number => HighlightKind::Number,
		SyntaxKind::Param => HighlightKind::Parameter,
		SyntaxKind::Operator | SyntaxKind::ColonColon => HighlightKind::Operator,
		SyntaxKind::Ident | SyntaxKind::QuotedIdent => {
			let start = u32::from(token.text_range().start()) as usize;
			if token
				.parent()
				.ancestors()
				.any(|node| node.kind() == SyntaxKind::TypeName)
			{
				HighlightKind::Type
			} else if is_function_name(token) {
				HighlightKind::Function
			} else if token.kind() == SyntaxKind::Ident && keywords.contains(&start) {
				HighlightKind::Keyword
			} else {
				HighlightKind::Name
			}
		}
		_ => return None,
	};
	Some(kind)
}

/// Is this the called name of a function call: the last word of the
/// reference that starts it (`count` in `pg_catalog.count(*)`)?
fn is_function_name(token: &SyntaxToken) -> bool {
	let reference = token.parent();
	if reference.kind() != SyntaxKind::ColumnRef {
		return false;
	}
	let calls = reference.parent().is_some_and(|call| {
		call.kind() == SyntaxKind::FunctionCall
			&& call.first_child().is_some_and(|first| first == reference)
	});
	let last_word = reference
		.children_with_tokens()
		.filter_map(|element| element.into_token())
		.filter(|word| {
			matches!(word.kind(), SyntaxKind::Ident | SyntaxKind::QuotedIdent)
		})
		.last()
		.is_some_and(|word| word == token);
	calls && last_word
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Each highlighted token's text and kind.
	fn kinds(sql: &str, dialect: parser::Dialect) -> Vec<(&str, HighlightKind)> {
		let options = Options { dialect, ..Options::default() };
		highlight(sql, &options)
			.into_iter()
			.map(|highlight| (&sql[highlight.range], highlight.kind))
			.collect()
	}

	#[test]
	fn names_spelled_like_keywords_are_names() {
		use HighlightKind::*;
		assert_eq!(
			kinds(
				"select name, count(*)::int from t where type = $1 -- hi\n",
				parser::Dialect::Postgres,
			),
			[
				("select", Keyword),
				("name", Name),
				("count", Function),
				("*", Operator),
				("::", Operator),
				("int", Type),
				("from", Keyword),
				("t", Name),
				("where", Keyword),
				("type", Name),
				("=", Operator),
				("$1", Parameter),
				("-- hi", Comment),
			]
		);
	}

	#[test]
	fn ddl_names_are_names() {
		use HighlightKind::*;
		assert_eq!(
			kinds(
				"create table t (key text not null default 'a', value int);",
				parser::Dialect::Postgres,
			),
			[
				("create", Keyword),
				("table", Keyword),
				("t", Name),
				("key", Name),
				("text", Type),
				("not", Keyword),
				("null", Keyword),
				("default", Keyword),
				("'a'", String),
				("value", Name),
				("int", Type),
			]
		);
	}

	#[test]
	fn unparsable_statements_use_the_keyword_tables() {
		use HighlightKind::*;
		assert_eq!(
			kinds("select a from where 'x'", parser::Dialect::Sqlite),
			[
				("select", Keyword),
				("a", Name),
				("from", Keyword),
				("where", Keyword),
				("'x'", String),
			]
		);
	}
}
