//! TREE-97/98 acceptance: insta snapshots of the formatted output for
//! every file of every corpus under `corpus/`, one per file, named
//! `<corpus>__<path>`.

use formatter::Options;
use formatter::format_cst;
use parser::lexer::lex_with;

/// Snapshot every file of one corpus.
fn snapshot_corpus(name: &str) {
	let files: Vec<_> = corpus::files(&corpus::root())
		.expect("read corpus")
		.into_iter()
		.filter(|file| file.corpus == name)
		.collect();
	assert!(!files.is_empty(), "no files found in corpus {name}");

	for file in files {
		let options = Options {
			dialect: file.dialect,
			at_params: file.lex_options.at_params,
			..Options::default()
		};
		let source = std::fs::read_to_string(&file.path).expect("read corpus file");
		let tokens = lex_with(&source, options.dialect, options.lex_options());
		let parse = parser::parser::parse(&tokens, options.dialect);
		let formatted = format_cst(&parse.cst, &options).text;
		insta::assert_snapshot!(file.name(), formatted);
	}
}

#[test]
fn coder_snapshots() {
	snapshot_corpus("coder");
}

#[test]
fn anki_snapshots() {
	snapshot_corpus("anki");
}

#[test]
fn synapse_snapshots() {
	snapshot_corpus("synapse");
}

#[test]
fn vaultwarden_snapshots() {
	snapshot_corpus("vaultwarden");
}
