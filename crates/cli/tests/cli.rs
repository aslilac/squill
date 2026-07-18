//! TREE-99 acceptance: the squill CLI end to end.

use std::io::Write;
use std::process::{Command, Stdio};

fn squill() -> Command {
    Command::new(env!("CARGO_BIN_EXE_squill"))
}

fn corpus() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/coder")
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("squill-cli-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// Acceptance: formatting the corpus and re-checking it runs clean, and a
/// second write pass is byte-identical.
#[test]
fn corpus_roundtrip_through_the_cli() {
    let dir = temp_dir("corpus");
    // Copy a slice of the corpus (a full copy is slow in a test): every
    // 20th migration plus all queries under 10KB.
    let mut copied = 0;
    for (index, entry) in walk(&corpus()).into_iter().enumerate() {
        let meta = std::fs::metadata(&entry).expect("meta");
        let is_query = entry.to_string_lossy().contains("queries");
        if (is_query && meta.len() < 10_000) || (!is_query && index % 20 == 0) {
            let dest = dir.join(entry.file_name().expect("file name"));
            std::fs::copy(&entry, &dest).expect("copy corpus file");
            copied += 1;
        }
    }
    assert!(copied > 20, "not enough corpus files copied");

    // First pass: write in place.
    let status = squill()
        .args(["fmt", "--at-params"])
        .arg(&dir)
        .status()
        .expect("run squill");
    assert!(status.success(), "initial fmt failed");

    // Check pass: clean.
    let output = squill()
        .args(["fmt", "--check", "--at-params"])
        .arg(&dir)
        .output()
        .expect("run squill");
    assert!(
        output.status.success(),
        "--check after fmt found changes:\n{}",
        String::from_utf8_lossy(&output.stdout)
    );

    // Second write pass: byte-identical no-op.
    let before: Vec<_> = walk(&dir)
        .into_iter()
        .map(|p| std::fs::read(&p).expect("read"))
        .collect();
    let status = squill()
        .args(["fmt", "--at-params"])
        .arg(&dir)
        .status()
        .expect("run squill");
    assert!(status.success());
    let after: Vec<_> = walk(&dir)
        .into_iter()
        .map(|p| std::fs::read(&p).expect("read"))
        .collect();
    assert_eq!(before, after, "second fmt pass must be a no-op");

    let _ = std::fs::remove_dir_all(&dir);
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "sql") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn check_reports_diff_and_exits_1() {
    let dir = temp_dir("diff");
    let file = dir.join("q.sql");
    std::fs::write(&file, "SELECT   1;\n").expect("write");
    let output = squill()
        .args(["fmt", "--check"])
        .arg(&file)
        .output()
        .expect("run squill");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("-SELECT   1;"), "diff missing: {stdout}");
    assert!(stdout.contains("+select 1;"), "diff missing: {stdout}");
    // The file was not modified.
    assert_eq!(
        std::fs::read_to_string(&file).expect("read"),
        "SELECT   1;\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stdin_formats_to_stdout() {
    let mut child = squill()
        .args(["fmt", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"SELECT * FROM t WHERE  a=1;")
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "select * from t where a = 1;\n"
    );
}

#[test]
fn strict_fails_on_error_statements() {
    let dir = temp_dir("strict");
    let file = dir.join("bad.sql");
    std::fs::write(&file, "FROBNICATE the database;\n").expect("write");

    // Default: diagnostics reported, exit 0.
    let output = squill().arg("fmt").arg(&file).output().expect("run");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("bad.sql:1:1:"),
        "diagnostic with line:col missing: {stderr}"
    );

    // Strict: exit 1.
    let status = squill()
        .args(["fmt", "--strict"])
        .arg(&file)
        .status()
        .expect("run");
    assert_eq!(status.code(), Some(1));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn option_flags_apply() {
    let mut child = squill()
        .args([
            "fmt",
            "--stdin",
            "--keyword-case",
            "upper",
            "--indent",
            "spaces",
            "--indent-width",
            "4",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"select aaaaaaaaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbbbbbbb, cccccccccccccccccccccc from t;")
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("SELECT\n    aaaa"), "got: {stdout}");
    assert!(stdout.contains("\nFROM t;"), "got: {stdout}");
}

#[test]
fn sqlite_dialect_flag() {
    let mut child = squill()
        .args(["fmt", "--stdin", "--dialect", "sqlite"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"select `a b`,[c] from t where x = :param;")
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Backtick/bracket idents normalize; :param survives.
    assert_eq!(stdout, "select \"a b\", c from t where x = :param;\n");
}
