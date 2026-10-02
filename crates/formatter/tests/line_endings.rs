//! The `line_ending` option: LF unless CRLF is asked for, never taken
//! from the input, and never applied to lines inside a value.

use formatter::LineEnding;
use formatter::Options;
use formatter::check;
use formatter::format_cst;
use parser::Dialect;
use parser::lexer::lex_with;

fn format(source: &str, line_ending: LineEnding) -> String {
	let options = Options { line_ending, ..Options::default() };
	let tokens = lex_with(source, Dialect::Postgres, options.lex_options());
	let parse = parser::parser::parse(&tokens, Dialect::Postgres);
	let formatted = format_cst(&parse.cst, &options);
	assert_eq!(formatted.fallback_statements, 0, "unexpected fallback");
	let text = formatted.text;
	assert!(
		check::tokens_equivalent(
			source,
			&text,
			Dialect::Postgres,
			options.lex_options()
		),
		"tokens changed: {text:?}"
	);
	assert_eq!(
		check::comment_texts(source, Dialect::Postgres, options.lex_options()),
		check::comment_texts(&text, Dialect::Postgres, options.lex_options()),
		"comments changed: {text:?}"
	);
	text
}

#[test]
fn crlf_input_comes_out_lf() {
	let source = "select a,\r\n b -- hi\r\nfrom t;\r\n/* x\r\ny */ select 1;\r\n";
	assert_eq!(
		format(source, LineEnding::Lf),
		"select\n\ta,\n\tb -- hi\nfrom t;\n/* x\ny */\nselect 1;\n"
	);
}

#[test]
fn crlf_is_opt_in() {
	let source = "select a,\n b -- hi\nfrom t;\n/* x\ny */ select 1;\n";
	let out = format(source, LineEnding::Crlf);
	assert_eq!(
		out,
		"select\r\n\ta,\r\n\tb -- hi\r\nfrom t;\r\n/* x\r\ny */\r\nselect 1;\r\n"
	);
	assert_eq!(format(&out, LineEnding::Crlf), out);
}

#[test]
fn unparsable_statements_follow_too() {
	assert_eq!(
		format("frobnicate\r\nthe widgets;\r\n", LineEnding::Lf),
		"frobnicate\nthe widgets;\n"
	);
	assert_eq!(
		format("frobnicate\nthe widgets;\n", LineEnding::Crlf),
		"frobnicate\r\nthe widgets;\r\n"
	);
}

#[test]
fn strings_keep_their_line_endings() {
	assert_eq!(
		format("select 'a\r\nb', $$c\r\nd$$\r\n", LineEnding::Lf),
		"select\n\t'a\r\nb',\n\t$$c\r\nd$$;\n"
	);
	assert_eq!(
		format("select 'a\nb', \"e\nf\"\n", LineEnding::Crlf),
		"select\r\n\t'a\nb',\r\n\t\"e\nf\";\r\n"
	);
}

#[test]
fn procedural_bodies_follow() {
	let source = "create function f() returns text language sql as $$\r\n\
		select 'a';\r\n$$;\r\n";
	assert_eq!(
		format(source, LineEnding::Lf),
		"create function f()\nreturns text\nlanguage sql\nas $$\n\tselect 'a';\n$$;\n"
	);
}
