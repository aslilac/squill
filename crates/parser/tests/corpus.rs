//! TREE-93 acceptance: the token stream round-trips byte-for-byte over the
//! entire coder corpus, with no error tokens.

use std::path::{Path, PathBuf};

use parser::Dialect;
use parser::lexer::lex;
use parser::syntax::SyntaxKind;

fn collect_sql_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read corpus dir") {
        let path = entry.expect("read corpus entry").path();
        if path.is_dir() {
            collect_sql_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "sql") {
            out.push(path);
        }
    }
}

#[test]
fn corpus_lexes_losslessly_with_no_error_tokens() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut files = Vec::new();
    collect_sql_files(&root, &mut files);
    assert!(!files.is_empty(), "no corpus files found");

    for path in files {
        let source = std::fs::read_to_string(&path).expect("read corpus file");
        let tokens = lex(&source, Dialect::Postgres);
        let rebuilt: String = tokens.iter().map(|t| t.text).collect();
        assert_eq!(rebuilt, source, "round-trip failed for {}", path.display());
        if let Some(err) = tokens.iter().find(|t| t.kind == SyntaxKind::Error) {
            panic!(
                "error token in {}: {:?}",
                path.display(),
                err.text.chars().take(40).collect::<String>()
            );
        }
    }
}
