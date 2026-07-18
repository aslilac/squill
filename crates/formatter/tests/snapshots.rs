//! TREE-97 acceptance: insta snapshot of the formatted output for every
//! file in `corpus/coder/queries/`.

use std::path::Path;

use formatter::{Options, format_cst};
use parser::lexer::lex_with;

#[test]
fn queries_formatted_snapshots() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/coder/queries");
    let mut files: Vec<_> = std::fs::read_dir(&root)
        .expect("read queries dir")
        .map(|entry| entry.expect("queries entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no query files found");

    let options = Options {
        at_params: true, // sqlc SQL
        ..Options::default()
    };

    for path in files {
        let source = std::fs::read_to_string(&path).expect("read query file");
        let tokens = lex_with(&source, options.dialect, options.lex_options());
        let parse = parser::parser::parse(&tokens, options.dialect);
        let formatted = format_cst(&parse.cst, &options).text;
        let name = path.file_stem().expect("file stem").to_string_lossy();
        insta::assert_snapshot!(format!("queries__{name}"), formatted);
    }
}
