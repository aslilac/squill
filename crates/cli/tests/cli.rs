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

// ---- TREE-105: squill.toml config discovery ----

#[test]
fn config_discovery_nested_and_precedence() {
    let dir = temp_dir("config");
    std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n").expect("write config");
    std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
    std::fs::write(
        dir.join("sub/squill.toml"),
        "# nearest config wins\nkeyword-case = \"lower\"\nindent = \"spaces\" # with a comment\n",
    )
    .expect("write sub config");
    std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");
    std::fs::write(dir.join("sub/b.sql"), "select 1;\n").expect("write");

    let status = squill().arg("fmt").arg(&dir).status().expect("run");
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(dir.join("a.sql")).expect("read"),
        "SELECT 1;\n",
        "root file must use the root config"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("sub/b.sql")).expect("read"),
        "select 1;\n",
        "nested file must use the nearest config"
    );

    // Explicit flags override config.
    std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");
    let status = squill()
        .args(["fmt", "--keyword-case", "lower"])
        .arg(dir.join("a.sql"))
        .status()
        .expect("run");
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(dir.join("a.sql")).expect("read"),
        "select 1;\n"
    );

    // --no-config ignores the config entirely.
    let status = squill()
        .args(["fmt", "--no-config"])
        .arg(dir.join("a.sql"))
        .status()
        .expect("run");
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(dir.join("a.sql")).expect("read"),
        "select 1;\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn config_errors_are_hard_errors_with_location() {
    let dir = temp_dir("badconfig");
    std::fs::write(dir.join("f.sql"), "select 1;\n").expect("write");

    // Unknown key.
    std::fs::write(dir.join("squill.toml"), "keyword_case = \"upper\"\n").expect("write");
    let output = squill().arg("fmt").arg(&dir).output().expect("run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("squill.toml:1:") && stderr.contains("unknown key"),
        "got: {stderr}"
    );

    // Syntax error with line number.
    std::fs::write(dir.join("squill.toml"), "# fine\n[section]\n").expect("write");
    let output = squill().arg("fmt").arg(&dir).output().expect("run");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("squill.toml:2:") && stderr.contains("sections"),
        "got: {stderr}"
    );

    // Wrong type.
    std::fs::write(dir.join("squill.toml"), "at-params = \"yes\"\n").expect("write");
    let output = squill().arg("fmt").arg(&dir).output().expect("run");
    assert_eq!(output.status.code(), Some(2));

    // The file was never touched.
    assert_eq!(
        std::fs::read_to_string(dir.join("f.sql")).expect("read"),
        "select 1;\n"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stdin_uses_cwd_config() {
    let dir = temp_dir("stdinconfig");
    std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n").expect("write");
    let mut child = squill()
        .args(["fmt", "--stdin"])
        .current_dir(&dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"select 1;")
        .expect("write");
    let output = child.wait_with_output().expect("wait");
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout), "SELECT 1;\n");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---- TREE-108: embedded SQL through the CLI ----

const RS_FIXTURE: &str = r####"fn q(pool: &PgPool) {
    let _ = sqlx::query!(
        r#"SELECT id,name FROM users
        WHERE org=$1 ORDER BY name"#,
        org
    );
}
"####;

#[test]
fn explicit_rust_path_formats_sqlx_macros() {
    let dir = temp_dir("embedrs");
    let file = dir.join("q.rs");
    std::fs::write(&file, RS_FIXTURE).expect("write");
    let status = squill().arg("fmt").arg(&file).status().expect("run");
    assert!(status.success());
    let out = std::fs::read_to_string(&file).expect("read");
    assert!(
        out.contains(
            "r#\"\n        select id, name\n        from users\n        where org = $1\n        order by name\n        \"#"
        ),
        "sqlx macro not formatted: {out}"
    );
    // Idempotent second pass.
    let before = out.clone();
    let status = squill().arg("fmt").arg(&file).status().expect("run");
    assert!(status.success());
    assert_eq!(std::fs::read_to_string(&file).expect("read"), before);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn directory_recursion_needs_embed_flag() {
    let dir = temp_dir("embeddir");
    std::fs::write(dir.join("q.rs"), RS_FIXTURE).expect("write");
    std::fs::write(dir.join("plain.sql"), "SELECT   1;\n").expect("write");

    // Without --embedded: only the .sql file changes.
    let status = squill().arg("fmt").arg(&dir).status().expect("run");
    assert!(status.success());
    assert_eq!(
        std::fs::read_to_string(dir.join("q.rs")).expect("read"),
        RS_FIXTURE
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("plain.sql")).expect("read"),
        "select 1;\n"
    );

    // With --embedded: the .rs file formats too, and --check is then clean.
    let status = squill()
        .args(["fmt", "--embedded"])
        .arg(&dir)
        .status()
        .expect("run");
    assert!(status.success());
    assert!(
        std::fs::read_to_string(dir.join("q.rs"))
            .expect("read")
            .contains("select id, name\n        from users"),
    );
    let status = squill()
        .args(["fmt", "--embedded", "--check"])
        .arg(&dir)
        .status()
        .expect("run");
    assert!(
        status.success(),
        "--check after --embedded fmt must be clean"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_mode_diffs_host_files() {
    let dir = temp_dir("embedcheck");
    let file = dir.join("q.rs");
    std::fs::write(&file, RS_FIXTURE).expect("write");
    let output = squill()
        .args(["fmt", "--check"])
        .arg(&file)
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("+"), "diff expected: {stdout}");
    assert_eq!(std::fs::read_to_string(&file).expect("read"), RS_FIXTURE);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn go_host_files_format() {
    let dir = temp_dir("embedgo");
    let file = dir.join("q.go");
    std::fs::write(
        &file,
        "package main\n\nfunc f(db *sql.DB) {\n\tdb.QueryRow(`SELECT count(*)\n\tFROM t WHERE  a=1`)\n}\n",
    )
    .expect("write");
    let status = squill().arg("fmt").arg(&file).status().expect("run");
    assert!(status.success());
    assert!(
        std::fs::read_to_string(&file)
            .expect("read")
            .contains("`\n\tselect count(*)\n\tfrom t\n\twhere a = 1\n\t`"),
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn custom_embed_query_file() {
    let dir = temp_dir("embedquery");
    // A query that only matches `my_sql!` macros.
    std::fs::write(
        dir.join("only_mine.scm"),
        "((macro_invocation macro: (identifier) @_name (token_tree (raw_string_literal) @sql.postgres)) (#eq? @_name \"my_sql\"))",
    )
    .expect("write");
    let file = dir.join("q.rs");
    std::fs::write(
        &file,
        "fn f() {\n    my_sql!(r#\"SELECT   1\"#);\n    sqlx::query!(r#\"SELECT   2\"#);\n}\n",
    )
    .expect("write");
    let status = squill()
        .args(["fmt", "--embedded-query"])
        .arg(dir.join("only_mine.scm"))
        .arg(&file)
        .status()
        .expect("run");
    assert!(status.success());
    let out = std::fs::read_to_string(&file).expect("read");
    assert!(
        out.contains("my_sql!(r#\"\n    select 1\n    \"#)"),
        "custom query missed: {out}"
    );
    assert!(
        out.contains("r#\"SELECT   2\"#"),
        "default macro must be untouched: {out}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
