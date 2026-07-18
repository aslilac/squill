//! Guard: the vendored corpus is present and non-empty.

use std::path::Path;

fn count_sql_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sql"))
        .count()
}

#[test]
fn coder_corpus_is_vendored() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/coder");
    assert!(
        count_sql_files(&corpus.join("migrations")) > 0,
        "corpus/coder/migrations has no .sql files"
    );
    assert!(
        count_sql_files(&corpus.join("queries")) > 0,
        "corpus/coder/queries has no .sql files"
    );
}
