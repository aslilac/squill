//! TREE-99 acceptance: the squill CLI end to end.

use std::io::Write;
use std::process::Command;
use std::process::Stdio;

fn squill() -> Command {
	Command::new(env!("CARGO_BIN_EXE_squill"))
}

fn corpus() -> std::path::PathBuf {
	std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus/coder")
}

fn temp_dir(name: &str) -> std::path::PathBuf {
	let dir = std::env::temp_dir()
		.join(format!("squill-cli-test-{name}-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(&dir).expect("create temp dir");
	// A repo-root marker, so config discovery stops here instead of
	// walking into the real temp directory: a stray squill.toml up there
	// would otherwise reconfigure every test at once.
	std::fs::create_dir_all(dir.join(".git")).expect("create .git marker");
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
	let before: Vec<_> =
		walk(&dir).into_iter().map(|p| std::fs::read(&p).expect("read")).collect();
	let status = squill()
		.args(["fmt", "--at-params"])
		.arg(&dir)
		.status()
		.expect("run squill");
	assert!(status.success());
	let after: Vec<_> =
		walk(&dir).into_iter().map(|p| std::fs::read(&p).expect("read")).collect();
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
	let output =
		squill().args(["fmt", "--check"]).arg(&file).output().expect("run squill");
	assert_eq!(output.status.code(), Some(1));
	let stdout = String::from_utf8_lossy(&output.stdout);
	assert!(stdout.contains("-SELECT   1;"), "diff missing: {stdout}");
	assert!(stdout.contains("+select 1;"), "diff missing: {stdout}");
	// The file was not modified.
	assert_eq!(std::fs::read_to_string(&file).expect("read"), "SELECT   1;\n");
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
	let status =
		squill().args(["fmt", "--strict"]).arg(&file).status().expect("run");
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
fn max_width_flag_applies() {
	// 38 chars flat: fits the default 80, breaks under --max-width 30.
	let sql = b"select aaaaaaaa, bbbbbbbb from big_table;";
	let mut child = squill()
		.args(["fmt", "--stdin", "--max-width", "30"])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.spawn()
		.expect("spawn");
	child.stdin.take().expect("stdin").write_all(sql).expect("write");
	let output = child.wait_with_output().expect("wait");
	let stdout = String::from_utf8_lossy(&output.stdout);
	assert_eq!(stdout, "select aaaaaaaa, bbbbbbbb\nfrom big_table;\n");

	let status = squill()
		.args(["fmt", "--stdin", "--max-width", "10"])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.status()
		.expect("run");
	assert_eq!(status.code(), Some(2), "out-of-range width is an error");
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
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write config");
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
	std::fs::write(dir.join("squill.toml"), "keyword_case = \"upper\"\n")
		.expect("write");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("squill.toml:1:") && stderr.contains("unknown key"),
		"got: {stderr}"
	);

	// A section that is not a known host language.
	std::fs::write(dir.join("squill.toml"), "# fine\n[section]\n")
		.expect("write");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("squill.toml:2:") && stderr.contains("unknown section"),
		"got: {stderr}"
	);

	// Wrong type.
	std::fs::write(dir.join("squill.toml"), "at-params = \"yes\"\n")
		.expect("write");
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
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write");
	let mut child = squill()
		.args(["fmt", "--stdin"])
		.current_dir(&dir)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.spawn()
		.expect("spawn");
	child.stdin.take().expect("stdin").write_all(b"select 1;").expect("write");
	let output = child.wait_with_output().expect("wait");
	assert!(output.status.success());
	assert_eq!(String::from_utf8_lossy(&output.stdout), "SELECT 1;\n");
	let _ = std::fs::remove_dir_all(&dir);
}

// ---- ignore mechanisms ----

#[test]
fn gitignore_and_hidden_files_are_skipped() {
	let dir = temp_dir("gitignore");
	std::fs::create_dir_all(dir.join("gen")).expect("mkdir");
	std::fs::write(dir.join(".gitignore"), "gen/\nscratch.sql\n").expect("write");
	std::fs::write(dir.join("gen/skip.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join("scratch.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join(".hidden.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join("keep.sql"), "SELECT   1;\n").expect("write");
	let status = Command::new("git")
		.args(["init", "-q"])
		.current_dir(&dir)
		.status()
		.expect("git init");
	assert!(status.success());

	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("keep.sql")).expect("read"),
		"select 1;\n"
	);
	for untouched in ["gen/skip.sql", "scratch.sql", ".hidden.sql"] {
		assert_eq!(
			std::fs::read_to_string(dir.join(untouched)).expect("read"),
			"SELECT   1;\n",
			"{untouched} must be skipped"
		);
	}

	// Explicit file arguments bypass ignores.
	let status =
		squill().arg("fmt").arg(dir.join("gen/skip.sql")).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("gen/skip.sql")).expect("read"),
		"select 1;\n"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ignore_patterns_from_config_and_flags() {
	let dir = temp_dir("ignorepatterns");
	std::fs::create_dir_all(dir.join("legacy")).expect("mkdir");
	std::fs::create_dir_all(dir.join("sub")).expect("mkdir");
	// Multi-line array with a comment and a trailing comma.
	std::fs::write(
		dir.join("squill.toml"),
		"ignore = [\n\t\"legacy\", # frozen migrations\n\t\"*.gen.sql\",\n\t\"sub/skip.sql\",\n]\n",
	)
	.expect("write config");
	std::fs::write(dir.join("legacy/old.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join("report.gen.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join("sub/skip.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join("sub/other.sql"), "SELECT   1;\n").expect("write");
	std::fs::write(dir.join("keep.sql"), "SELECT   1;\n").expect("write");

	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("keep.sql")).expect("read"),
		"select 1;\n"
	);
	assert_eq!(
		std::fs::read_to_string(dir.join("sub/other.sql")).expect("read"),
		"select 1;\n"
	);
	for untouched in ["legacy/old.sql", "report.gen.sql", "sub/skip.sql"] {
		assert_eq!(
			std::fs::read_to_string(dir.join(untouched)).expect("read"),
			"SELECT   1;\n",
			"{untouched} must be skipped"
		);
	}

	// Config patterns are relative to the config file: formatting the
	// subdirectory still skips sub/skip.sql.
	std::fs::write(dir.join("sub/other.sql"), "SELECT   1;\n").expect("write");
	let status = squill().arg("fmt").arg(dir.join("sub")).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("sub/other.sql")).expect("read"),
		"select 1;\n"
	);
	assert_eq!(
		std::fs::read_to_string(dir.join("sub/skip.sql")).expect("read"),
		"SELECT   1;\n"
	);

	// --ignore adds patterns (cwd-relative), on top of the config's.
	std::fs::write(dir.join("keep.sql"), "SELECT   2;\n").expect("write");
	let status = squill()
		.args(["fmt", "--ignore", "keep*", "."])
		.current_dir(&dir)
		.status()
		.expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("keep.sql")).expect("read"),
		"SELECT   2;\n",
		"--ignore pattern must skip keep.sql"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn ignore_array_errors_have_locations() {
	let dir = temp_dir("ignoreerrors");
	std::fs::write(dir.join("f.sql"), "select 1;\n").expect("write");

	// Unterminated array: a TOML syntax error with a location.
	std::fs::write(dir.join("squill.toml"), "ignore = [\n\t\"a\",\n")
		.expect("write");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("squill.toml:2:") && stderr.contains("unclosed array"),
		"got: {stderr}"
	);

	// Scalar instead of an array.
	std::fs::write(dir.join("squill.toml"), "ignore = \"legacy\"\n")
		.expect("write");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(stderr.contains("expects an array"), "got: {stderr}");

	// Invalid glob.
	std::fs::write(dir.join("squill.toml"), "ignore = [\"a[\"]\n")
		.expect("write");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(stderr.contains("invalid ignore pattern"), "got: {stderr}");

	assert_eq!(
		std::fs::read_to_string(dir.join("f.sql")).expect("read"),
		"select 1;\n",
		"no file may be touched on config errors"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn dot_config_location_and_precedence() {
	let dir = temp_dir("dotconfig");
	std::fs::create_dir_all(dir.join(".config")).expect("mkdir");
	std::fs::write(dir.join(".config/squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write");
	std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");

	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("a.sql")).expect("read"),
		"SELECT 1;\n",
		".config/squill.toml must apply"
	);

	// A bare squill.toml at the same level wins.
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"lower\"\n")
		.expect("write");
	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("a.sql")).expect("read"),
		"select 1;\n",
		"squill.toml must win over .config/squill.toml"
	);
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
	let status =
		squill().args(["fmt", "--embedded"]).arg(&dir).status().expect("run");
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
	assert!(status.success(), "--check after --embedded fmt must be clean");
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_mode_diffs_host_files() {
	let dir = temp_dir("embedcheck");
	let file = dir.join("q.rs");
	std::fs::write(&file, RS_FIXTURE).expect("write");
	let output =
		squill().args(["fmt", "--check"]).arg(&file).output().expect("run");
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

#[test]
fn check_large_diff_prints_replacement_hunk() {
	// Past the fine-diff limit, --check prints a deterministic
	// whole-file replacement instead of running Myers.
	let dir = temp_dir("bigdiff");
	let file = dir.join("big.sql");
	let mut source = String::new();
	for i in 0..1500 {
		source.push_str(&format!("SELECT   {i};\n"));
	}
	std::fs::write(&file, &source).expect("write");
	let output =
		squill().args(["fmt", "--check"]).arg(&file).output().expect("run squill");
	assert_eq!(output.status.code(), Some(1));
	let stdout = String::from_utf8_lossy(&output.stdout);
	assert!(
		stdout.contains("@@ -1,1500 +1,1500 @@"),
		"replacement hunk header missing: {}",
		&stdout[..stdout.len().min(200)]
	);
	assert!(stdout.contains("-SELECT   7;"), "old lines missing");
	assert!(stdout.contains("+select 7;"), "new lines missing");
	let _ = std::fs::remove_dir_all(&dir);
}

/// One config file, two host languages, two indent styles — and the
/// section beats the host file's own indentation, which is what makes
/// the setting worth having for a tab-indented language like Go.
#[test]
fn language_sections_configure_indent_per_host() {
	let dir = temp_dir("langsections");
	std::fs::write(
		dir.join("squill.toml"),
		"indent = \"tab\"\n\n[javascript]\nindent = \"spaces\"\nindent-width = 2\n\n[go]\nindent = \"tab\"\n",
	)
	.expect("write config");

	let js = dir.join("q.js");
	std::fs::write(
		&js,
		"function f(db) {\n  return db.query(`SELECT id,name,email,created_at,updated_at,deleted_at,organization_id,avatar_url FROM users WHERE org = $1`);\n}\n",
	)
	.expect("write js");
	// A Go file indented with tabs; the SQL keeps tabs.
	let go = dir.join("q.go");
	std::fs::write(
		&go,
		"package main\n\nfunc f(db *sql.DB) {\n\tdb.QueryRow(`SELECT id,name,email,created_at,updated_at,deleted_at,organization_id,avatar_url FROM users WHERE org = $1`)\n}\n",
	)
	.expect("write go");

	let status =
		squill().args(["fmt", "--embedded"]).arg(&dir).status().expect("run");
	assert!(status.success());

	let js_out = std::fs::read_to_string(&js).expect("read js");
	assert!(
		js_out.contains("  select\n    id,\n    name,\n"),
		"javascript section did not give two-space SQL: {js_out}"
	);
	let go_out = std::fs::read_to_string(&go).expect("read go");
	assert!(
		go_out.contains("\tselect\n\t\tid,\n\t\tname,\n"),
		"go section did not give tab SQL: {go_out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A section's indent wins over the host file's indentation even when
/// the two disagree, and unset keys still fall through to the top level.
#[test]
fn language_section_overrides_host_indent_and_inherits_rest() {
	let dir = temp_dir("langoverride");
	std::fs::write(
		dir.join("squill.toml"),
		"keyword-case = \"upper\"\n\n[go]\nindent = \"spaces\"\nindent-width = 4\n",
	)
	.expect("write config");
	let go = dir.join("q.go");
	std::fs::write(
		&go,
		"package main\n\nfunc f(db *sql.DB) {\n\tdb.QueryRow(`SELECT id,name,email,created_at,updated_at,deleted_at,organization_id,avatar_url FROM users WHERE org = $1`)\n}\n",
	)
	.expect("write go");

	let status = squill().arg("fmt").arg(&go).status().expect("run");
	assert!(status.success());
	let out = std::fs::read_to_string(&go).expect("read");
	// Four-space SQL indent inside a tab-indented host file, and the
	// top-level keyword-case still applies.
	assert!(
		out.contains("\tSELECT\n\t    id,\n\t    name,\n")
			&& out.contains("\tFROM users\n"),
		"section indent or inherited keyword-case missing: {out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Sections apply only to their own language: a `[go]` section leaves
/// plain .sql files (and other hosts) on the top-level settings.
#[test]
fn language_sections_do_not_leak_to_other_files() {
	let dir = temp_dir("langscope");
	std::fs::write(
		dir.join("squill.toml"),
		"indent = \"tab\"\n\n[go]\nindent = \"spaces\"\nindent-width = 4\nkeyword-case = \"upper\"\n",
	)
	.expect("write config");
	std::fs::write(dir.join("a.sql"), "select id from (select 1 as id) t;\n")
		.expect("write sql");
	let status =
		squill().arg("fmt").arg(dir.join("a.sql")).status().expect("run");
	assert!(status.success());
	let out = std::fs::read_to_string(dir.join("a.sql")).expect("read");
	assert_eq!(
		out, "select id from (select 1 as id) t;\n",
		"the [go] section must not touch .sql files"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Flags still beat a section, the same way they beat the top level.
#[test]
fn flags_override_language_sections() {
	let dir = temp_dir("langflags");
	std::fs::write(dir.join("squill.toml"), "[go]\nkeyword-case = \"upper\"\n")
		.expect("write config");
	let go = dir.join("q.go");
	std::fs::write(
		&go,
		"package main\n\nfunc f(db *sql.DB) {\n\tdb.QueryRow(`SELECT id,name,email,created_at,updated_at,deleted_at,organization_id,avatar_url FROM users WHERE org = $1`)\n}\n",
	)
	.expect("write go");
	let status = squill()
		.args(["fmt", "--keyword-case", "lower"])
		.arg(&go)
		.status()
		.expect("run");
	assert!(status.success());
	let out = std::fs::read_to_string(&go).expect("read");
	assert!(
		out.contains("\tselect\n\t\tid,"),
		"flag did not override section: {out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Bad sections are hard errors with a location, like any other config
/// mistake.
#[test]
fn language_section_errors_have_locations() {
	let dir = temp_dir("langbad");
	std::fs::write(dir.join("f.sql"), "select 1;\n").expect("write");

	let cases = [
		("[ruby]\nindent = \"tab\"\n", 1, "unknown section `[ruby]`"),
		("[go]\nignore = [\"x\"]\n", 2, "applies to the whole file"),
		("[go]\nindent = \"elephant\"\n", 2, "unknown indent style"),
		("[go]\nindent-width = 99\n", 2, "integer from 1 to 16"),
		("[go]\nnope = 1\n", 2, "unknown key `nope`"),
		("[go]\n[go.inner]\nindent = \"tab\"\n", 2, "do not nest"),
		// Multi-language headers are validated name by name.
		("[\"go, ruby\"]\nindent = \"tab\"\n", 1, "unknown section `[ruby]`"),
		("[\"go,\"]\nindent = \"tab\"\n", 1, "empty language name"),
		(
			"[go]\nindent = \"tab\"\n\n[\"go, rust\"]\nindent = \"spaces\"\n",
			4,
			"`go` is configured by more than one section",
		),
		(
			"[\"go, go\"]\nindent = \"tab\"\n",
			1,
			"`go` is configured by more than one section",
		),
	];
	for (config, line, needle) in cases {
		std::fs::write(dir.join("squill.toml"), config).expect("write config");
		let output = squill().arg("fmt").arg(&dir).output().expect("run");
		assert_eq!(output.status.code(), Some(2), "config accepted: {config}");
		let stderr = String::from_utf8_lossy(&output.stderr);
		assert!(
			stderr.contains(&format!("squill.toml:{line}:"))
				&& stderr.contains(needle),
			"for {config:?} got: {stderr}"
		);
	}
	let _ = std::fs::remove_dir_all(&dir);
}

/// Discovery stops at a git repository root: a config above a checkout
/// never reconfigures the code inside it.
#[test]
fn discovery_stops_at_a_git_repo_root() {
	let dir = temp_dir("gitboundary");
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write config");

	// A checkout below it, with its own repo-root marker and no config.
	std::fs::create_dir_all(dir.join("repo/.git")).expect("mkdir repo");
	std::fs::write(dir.join("repo/a.sql"), "select 1;\n").expect("write");
	// The same layout without the marker, as a control.
	std::fs::create_dir_all(dir.join("plain")).expect("mkdir plain");
	std::fs::write(dir.join("plain/b.sql"), "select 1;\n").expect("write");

	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("repo/a.sql")).expect("read"),
		"select 1;\n",
		"config leaked past the repo root"
	);
	assert_eq!(
		std::fs::read_to_string(dir.join("plain/b.sql")).expect("read"),
		"SELECT 1;\n",
		"without a repo root the walk should reach the config"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A `.git` file (worktrees and submodules) bounds the walk just like a
/// `.git` directory.
#[test]
fn discovery_stops_at_a_git_worktree_file() {
	let dir = temp_dir("gitfile");
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write config");
	std::fs::create_dir_all(dir.join("wt")).expect("mkdir");
	std::fs::write(dir.join("wt/.git"), "gitdir: /elsewhere/.git/worktrees/wt\n")
		.expect("write .git file");
	std::fs::write(dir.join("wt/a.sql"), "select 1;\n").expect("write");

	let status =
		squill().arg("fmt").arg(dir.join("wt/a.sql")).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("wt/a.sql")).expect("read"),
		"select 1;\n",
		"config leaked past a .git file"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Discovery stops at a symlinked directory, whose lexical parent is not
/// the tree it actually lives in.
#[cfg(unix)]
#[test]
fn discovery_stops_at_a_symlinked_directory() {
	let dir = temp_dir("symlinkboundary");
	// The config sits beside both the real directory and the link, so
	// the only difference between the two runs is how we got there.
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write config");
	std::fs::create_dir_all(dir.join("real")).expect("mkdir");
	std::fs::write(dir.join("real/a.sql"), "select 1;\n").expect("write");
	std::os::unix::fs::symlink(dir.join("real"), dir.join("link"))
		.expect("symlink");

	// Through the link: the walk stops at the link itself.
	let status =
		squill().arg("fmt").arg(dir.join("link/a.sql")).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("real/a.sql")).expect("read"),
		"select 1;\n",
		"config leaked through a symlinked directory"
	);

	// Through the real path: the same file, now reconfigured.
	let status =
		squill().arg("fmt").arg(dir.join("real/a.sql")).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("real/a.sql")).expect("read"),
		"SELECT 1;\n",
		"the real path should reach the config"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A relative path argument walks up to the config the same way an
/// absolute one does — `Path::parent` alone runs out at the working
/// directory.
#[test]
fn relative_paths_find_a_config_above_the_working_directory() {
	let dir = temp_dir("relativewalk");
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write config");
	std::fs::create_dir_all(dir.join("sub/deeper")).expect("mkdir");
	std::fs::write(dir.join("sub/deeper/a.sql"), "select 1;\n").expect("write");

	let status = squill()
		.current_dir(dir.join("sub"))
		.args(["fmt", "deeper/a.sql"])
		.status()
		.expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("sub/deeper/a.sql")).expect("read"),
		"SELECT 1;\n",
		"relative path did not reach the config two levels up"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// One section can name several languages at once. TOML has no bare
/// comma in a table header, so the name list is quoted.
#[test]
fn one_section_can_name_several_languages() {
	let dir = temp_dir("multilang");
	std::fs::write(
		dir.join("squill.toml"),
		"indent = \"tab\"\n\n[\"javascript, typescript\"]\nindent = \"spaces\"\nindent-width = 2\n",
	)
	.expect("write config");

	let query = "SELECT id,name,email,created_at,updated_at,deleted_at,organization_id,avatar_url FROM users WHERE org = $1";
	for name in ["q.js", "q.ts", "q.tsx"] {
		std::fs::write(
			dir.join(name),
			format!("export function f(db) {{\n\treturn db.query(`{query}`);\n}}\n"),
		)
		.expect("write host file");
	}
	// Not named by the section: stays on the top-level tab indent.
	std::fs::write(
		dir.join("q.go"),
		format!(
			"package main\n\nfunc f(db *sql.DB) {{\n\tdb.QueryRow(`{query}`)\n}}\n"
		),
	)
	.expect("write go");

	let status =
		squill().args(["fmt", "--embedded"]).arg(&dir).status().expect("run");
	assert!(status.success());
	for name in ["q.js", "q.ts", "q.tsx"] {
		let out = std::fs::read_to_string(dir.join(name)).expect("read");
		assert!(
			out.contains("\tselect\n\t  id,\n\t  name,\n"),
			"{name} did not take the shared section: {out}"
		);
	}
	let go_out = std::fs::read_to_string(dir.join("q.go")).expect("read");
	assert!(
		go_out.contains("\tselect\n\t\tid,\n\t\tname,\n"),
		"go should keep the top-level tab indent: {go_out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}
