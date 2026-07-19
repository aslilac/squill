//! TREE-97/98 acceptance: insta snapshots of the formatted output for
//! every file in `corpus/coder/queries/` and `corpus/coder/migrations/`.

use formatter::Options;
use formatter::format_cst;
use parser::lexer::lex_with;
use std::path::Path;

fn snapshot_dir(dir: &str) {
	let root = Path::new(env!("CARGO_MANIFEST_DIR"))
		.join(format!("../../corpus/coder/{dir}"));
	let mut files: Vec<_> = std::fs::read_dir(&root)
		.expect("read corpus dir")
		.map(|entry| entry.expect("corpus entry").path())
		.filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
		.collect();
	files.sort();
	assert!(!files.is_empty(), "no files found in {dir}");

	let options = Options {
		at_params: true, // sqlc SQL
		..Options::default()
	};

	for path in files {
		let source = std::fs::read_to_string(&path).expect("read corpus file");
		let tokens = lex_with(&source, options.dialect, options.lex_options());
		let parse = parser::parser::parse(&tokens, options.dialect);
		let formatted = format_cst(&parse.cst, &options).text;
		let name = path.file_stem().expect("file stem").to_string_lossy();
		insta::assert_snapshot!(format!("{dir}__{name}"), formatted);
	}
}

#[test]
fn queries_formatted_snapshots() {
	snapshot_dir("queries");
}

#[test]
fn migrations_formatted_snapshots() {
	snapshot_dir("migrations");
}

#[test]
fn fixtures_formatted_snapshots() {
	snapshot_dir("fixtures");
}
