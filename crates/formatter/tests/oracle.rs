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
use std::path::Path;
use std::path::PathBuf;

fn collect_sql_files(dir: &Path, out: &mut Vec<PathBuf>) {
	for entry in std::fs::read_dir(dir).expect("read corpus dir") {
		let path = entry.expect("corpus entry").path();
		if path.is_dir() {
			collect_sql_files(&path, out);
		} else if path.extension().is_some_and(|ext| ext == "sql") {
			out.push(path);
		}
	}
}

fn format_source(source: &str, options: &Options) -> String {
	let tokens = lex_with(source, options.dialect, options.lex_options());
	let parse = parser::parser::parse(&tokens, options.dialect);
	format_cst(&parse.cst, options).text
}

#[test]
fn safety_oracle_holds_corpus_wide() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
	let mut files = Vec::new();
	collect_sql_files(&root, &mut files);
	assert!(!files.is_empty(), "no corpus files found");
	files.sort();

	let options = Options {
		at_params: true, // the coder corpus is sqlc SQL
		..Options::default()
	};
	let lex_options = options.lex_options();

	for path in files {
		let source = std::fs::read_to_string(&path).expect("read corpus file");
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
