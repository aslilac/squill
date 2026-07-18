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

/// TREE-95: parsing is lossless corpus-wide, including ErrorStatements.
#[test]
fn corpus_parses_losslessly() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let mut files = Vec::new();
    collect_sql_files(&root, &mut files);

    for path in files {
        let source = std::fs::read_to_string(&path).expect("read corpus file");
        let tokens = lex(&source, Dialect::Postgres);
        let parse = parser::parser::parse(&tokens, Dialect::Postgres);
        assert_eq!(
            parse.cst.text(),
            source,
            "parse round-trip failed for {}",
            path.display()
        );
    }
}

/// TREE-95: every statement in `corpus/coder/queries/` that does not
/// involve DML parses cleanly. (DML lands with TREE-98.)
#[test]
fn queries_select_statements_all_parse() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/coder/queries");
    let mut files = Vec::new();
    collect_sql_files(&root, &mut files);
    assert!(!files.is_empty(), "no query files found");

    for path in files {
        let source = std::fs::read_to_string(&path).expect("read corpus file");
        let tokens = lex(&source, Dialect::Postgres);
        let parse = parser::parser::parse(&tokens, Dialect::Postgres);
        for node in parse.cst.root().children() {
            if node.kind() == SyntaxKind::ErrorStatement {
                let text = node.to_string();
                assert!(
                    !starts_selectish(&text) || involves_dml(&text),
                    "SELECT statement failed to parse in {}: {}",
                    path.display(),
                    text.split_whitespace().collect::<Vec<_>>().join(" ")
                );
            }
        }
    }
}

/// Does the statement start like a query (`SELECT`/`WITH`/`VALUES`/
/// `TABLE`/`(`)?
fn starts_selectish(sql: &str) -> bool {
    let tokens = lex(sql, Dialect::Postgres);
    tokens
        .iter()
        .find(|t| !t.kind.is_trivia())
        .is_some_and(|t| {
            t.kind == SyntaxKind::LParen
                || ["select", "with", "values", "table"]
                    .iter()
                    .any(|kw| t.text.eq_ignore_ascii_case(kw))
        })
}

/// Does the failed statement involve DML — a top-level
/// INSERT/UPDATE/DELETE/MERGE or a data-modifying CTE? Mirrors the
/// corpus-report classifier; `FOR [NO KEY] UPDATE` does not count.
fn involves_dml(sql: &str) -> bool {
    let tokens = lex(sql, Dialect::Postgres);
    let mut depth = 0i32;
    let mut prev = String::new();
    for token in tokens.iter().filter(|t| !t.kind.is_trivia()) {
        match token.kind {
            SyntaxKind::LParen => depth += 1,
            SyntaxKind::RParen => depth -= 1,
            SyntaxKind::Ident => {
                let text = token.text.to_ascii_lowercase();
                let is_dml = ["insert", "update", "delete", "merge"].contains(&text.as_str());
                let after_lock = ["for", "key", "no"].contains(&prev.as_str());
                if is_dml && (prev == "(" || (depth == 0 && !after_lock)) {
                    return true;
                }
            }
            _ => {}
        }
        prev = token.text.to_ascii_lowercase();
    }
    false
}
