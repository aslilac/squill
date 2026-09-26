//! TREE-97 safety oracle, corpus-wide:
//!
//! 1. Idempotence: `format(format(x)) == format(x)`.
//! 2. Token equivalence: input and output re-lex to the same non-trivia
//!    stream modulo keyword case and sanctioned quote changes.
//! 3. Comment conservation: same comment texts, same order.
//!
//! ErrorStatements pass through verbatim, so 1–2 hold for them trivially;
//! 3 covers them regardless.

use formatter::Options;
use formatter::check;
use formatter::format_cst;
use parser::lexer::lex_with;

fn format_source(source: &str, options: &Options) -> String {
	let tokens = lex_with(source, options.dialect, options.lex_options());
	let parse = parser::parser::parse(&tokens, options.dialect);
	format_cst(&parse.cst, options).text
}

#[test]
fn safety_oracle_holds_corpus_wide() {
	let files = corpus::files(&corpus::root()).expect("read corpus");
	assert!(!files.is_empty(), "no corpus files found");

	for file in files {
		let options = Options {
			dialect: file.dialect,
			at_params: file.lex_options.at_params,
			..Options::default()
		};
		let lex_options = options.lex_options();
		let path = &file.path;
		let source = std::fs::read_to_string(path).expect("read corpus file");
		let once = format_source(&source, &options);

		// 1. Idempotence.
		let twice = format_source(&once, &options);
		assert_eq!(twice, once, "format is not idempotent for {}", path.display());

		// 2. Token equivalence.
		assert!(
			check::tokens_equivalent(&source, &once, options.dialect, lex_options),
			"token stream changed for {}",
			path.display()
		);

		// 3. Comment conservation.
		assert_eq!(
			check::comment_texts(&source, options.dialect, lex_options),
			check::comment_texts(&once, options.dialect, lex_options),
			"comments changed for {}",
			path.display()
		);
	}
}
