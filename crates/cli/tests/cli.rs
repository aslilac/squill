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
	// `-` is the conventional spelling of stdin as a path.
	for flag in ["--stdin", "-"] {
		let mut child = squill()
			.args(["fmt", flag])
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
		assert!(output.status.success(), "{flag}");
		assert_eq!(
			String::from_utf8_lossy(&output.stdout),
			"select * from t where a = 1;\n",
			"{flag}"
		);
	}
}

#[test]
fn stdin_dash_cannot_mix_with_paths() {
	let output = squill().args(["fmt", "-", "a.sql"]).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	assert!(
		String::from_utf8_lossy(&output.stderr).contains("cannot be combined")
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

	for width in ["0", "99"] {
		let status = squill()
			.args(["fmt", "--stdin", "--indent-width", width])
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.status()
			.expect("run");
		assert_eq!(status.code(), Some(2), "--indent-width {width} accepted");
	}
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

/// The rule every Rust embedding test starts from.
const RUST_RULE: &str =
	"[[embedded]]\ninclude = [\"**/*.rs\"]\ngrammar = \"rust\"\n";

/// A Go file whose one query is long enough to break.
const GO_WIDE: &str = "package main\n\nfunc f(db *sql.DB) {\n\tdb.QueryRow(`SELECT id,name,email,created_at,updated_at,deleted_at,organization_id,avatar_url FROM users WHERE org = $1`)\n}\n";

#[test]
fn explicit_rust_path_formats_under_an_embedded_rule() {
	let dir = temp_dir("embedrs");
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
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

/// A file no rule covers, and not named *.sql, is an error when named
/// explicitly — with the rule to add — rather than formatted as SQL: a
/// host file as SQL is a wall of parse errors.
#[test]
fn explicit_path_without_a_rule_explains_itself() {
	let dir = temp_dir("embednorule");
	let file = dir.join("q.rs");
	std::fs::write(&file, RS_FIXTURE).expect("write");
	// No config at all: point at the wizard.
	let output = squill().arg("fmt").arg(&file).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(stderr.contains("run `squill init`"), "{stderr}");
	// A config without the rule: show the rule to add.
	std::fs::write(dir.join("squill.toml"), "dialect = \"postgres\"\n")
		.expect("write config");
	let output = squill().arg("fmt").arg(&file).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("no rule covers it")
			&& stderr.contains("[[embedded]]")
			&& stderr.contains("grammar = \"rust\""),
		"{stderr}"
	);
	assert_eq!(std::fs::read_to_string(&file).expect("read"), RS_FIXTURE);
	// Anything else: a [[files]] rule would make it SQL.
	let notes = dir.join("notes.pgsql");
	std::fs::write(&notes, "SELECT   1;\n").expect("write");
	let output = squill().arg("fmt").arg(&notes).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("[[files]]") && stderr.contains("\"**/*.pgsql\""),
		"{stderr}"
	);
	assert_eq!(std::fs::read_to_string(&notes).expect("read"), "SELECT   1;\n");
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn embedded_rules_bring_host_files_into_recursion() {
	let dir = temp_dir("embeddir");
	std::fs::write(dir.join("q.rs"), RS_FIXTURE).expect("write");
	std::fs::write(dir.join("plain.sql"), "SELECT   1;\n").expect("write");

	// No rule: only the .sql file changes.
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

	// With a rule: the .rs file formats too, and --check is then clean.
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert!(
		std::fs::read_to_string(dir.join("q.rs"))
			.expect("read")
			.contains("select id, name\n        from users"),
	);
	let status =
		squill().args(["fmt", "--check"]).arg(&dir).status().expect("run");
	assert!(status.success(), "--check after fmt must be clean");
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn check_mode_diffs_host_files() {
	let dir = temp_dir("embedcheck");
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
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
	std::fs::write(
		dir.join("squill.toml"),
		"[[embedded]]\ninclude = [\"*.go\"]\ngrammar = \"go\"\n",
	)
	.expect("write config");
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

/// A rule's `query` replaces the grammar's default, and is found
/// relative to the config.
#[test]
fn rules_take_a_custom_query() {
	let dir = temp_dir("embedquery");
	std::fs::create_dir_all(dir.join(".config/squill")).expect("mkdir");
	// A query that only matches `my_sql!` macros.
	std::fs::write(
        dir.join(".config/squill/only_mine.scm"),
        "((macro_invocation macro: (identifier) @_name (token_tree (raw_string_literal) @sql)) (#eq? @_name \"my_sql\"))",
    )
    .expect("write");
	std::fs::write(
		dir.join(".config/squill.toml"),
		"[[embedded]]\ninclude = [\"**/*.rs\"]\ngrammar = \"rust\"\nquery = \".config/squill/only_mine.scm\"\n",
	)
	.expect("write config");
	std::fs::create_dir_all(dir.join("src")).expect("mkdir");
	let file = dir.join("src/q.rs");
	std::fs::write(
        &file,
        "fn f() {\n    my_sql!(r#\"SELECT   1\"#);\n    sqlx::query!(r#\"SELECT   2\"#);\n}\n",
    )
    .expect("write");
	let status = squill().arg("fmt").arg(&dir).status().expect("run");
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

/// Strings squill won't rewrite are diagnostics: reported with a
/// location, and failures under --strict.
#[test]
fn embedded_warnings_are_diagnostics() {
	let dir = temp_dir("embedwarn");
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
	let file = dir.join("q.rs");
	let source = "fn f() {\n    sqlx::query!(r#\"select (\n    from\"#);\n}\n";
	std::fs::write(&file, source).expect("write");
	let output = squill().arg("fmt").arg(&file).output().expect("run");
	assert!(output.status.success());
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(stderr.contains("q.rs:2:18: embedded SQL did not parse"), "{stderr}");
	assert!(stderr.contains("1 diagnostic(s)"), "{stderr}");
	assert_eq!(std::fs::read_to_string(&file).expect("read"), source);
	let status =
		squill().args(["fmt", "--strict"]).arg(&file).status().expect("run");
	assert_eq!(status.code(), Some(1));
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

/// Rules give each host language its own indent — and a rule's indent
/// beats the host file's own indentation, which is what makes the
/// setting worth having for a tab-indented language like Go.
#[test]
fn rules_configure_indent_per_host() {
	let dir = temp_dir("langsections");
	std::fs::write(
		dir.join("squill.toml"),
		"indent = \"tabs\"\n\n[[embedded]]\ninclude = [\"*.js\"]\ngrammar = \"javascript\"\nindent = \"spaces\"\nindent-width = 2\n\n[[embedded]]\ninclude = [\"*.go\"]\ngrammar = \"go\"\n",
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
	std::fs::write(&go, GO_WIDE).expect("write go");

	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());

	let js_out = std::fs::read_to_string(&js).expect("read js");
	assert!(
		js_out.contains("  select\n    id,\n    name,\n"),
		"javascript rule did not give two-space SQL: {js_out}"
	);
	let go_out = std::fs::read_to_string(&go).expect("read go");
	assert!(
		go_out.contains("\tselect\n\t\tid,\n\t\tname,\n"),
		"go rule did not give tab SQL: {go_out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A rule's indent wins over the host file's indentation even when the
/// two disagree, and unset keys still fall through to the top level.
#[test]
fn rule_overrides_host_indent_and_inherits_rest() {
	let dir = temp_dir("langoverride");
	std::fs::write(
		dir.join("squill.toml"),
		"keyword-case = \"upper\"\n\n[[embedded]]\ninclude = [\"*.go\"]\ngrammar = \"go\"\nindent = \"spaces\"\nindent-width = 4\n",
	)
	.expect("write config");
	let go = dir.join("q.go");
	std::fs::write(&go, GO_WIDE).expect("write go");

	let status = squill().arg("fmt").arg(&go).status().expect("run");
	assert!(status.success());
	let out = std::fs::read_to_string(&go).expect("read");
	// Four-space SQL indent inside a tab-indented host file, and the
	// top-level keyword-case still applies.
	assert!(
		out.contains("\tSELECT\n\t    id,\n\t    name,\n")
			&& out.contains("\tFROM users\n"),
		"rule indent or inherited keyword-case missing: {out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// An [[embedded]] rule's options apply only to its host files: plain
/// .sql files stay on the top-level settings.
#[test]
fn embedded_rules_do_not_leak_to_sql_files() {
	let dir = temp_dir("langscope");
	std::fs::write(
		dir.join("squill.toml"),
		"indent = \"tabs\"\n\n[[embedded]]\ninclude = [\"**\"]\nindent = \"spaces\"\nindent-width = 4\nkeyword-case = \"upper\"\n",
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
		"an [[embedded]] rule must not touch .sql files"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Flags still beat a rule, the same way they beat the top level.
#[test]
fn flags_override_rules() {
	let dir = temp_dir("langflags");
	std::fs::write(
		dir.join("squill.toml"),
		"[[embedded]]\ninclude = [\"*.go\"]\ngrammar = \"go\"\nkeyword-case = \"upper\"\n",
	)
	.expect("write config");
	let go = dir.join("q.go");
	std::fs::write(&go, GO_WIDE).expect("write go");
	let status = squill()
		.args(["fmt", "--keyword-case", "lower"])
		.arg(&go)
		.status()
		.expect("run");
	assert!(status.success());
	let out = std::fs::read_to_string(&go).expect("read");
	assert!(
		out.contains("\tselect\n\t\tid,"),
		"flag did not override rule: {out}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// [[files]] rules scope options to paths and bring in SQL files not
/// named *.sql — a mixed-dialect tree like Synapse's.
#[test]
fn files_rules_set_dialect_by_path() {
	let dir = temp_dir("filesrules");
	std::fs::write(
		dir.join("squill.toml"),
		"[[files]]\ninclude = [\"*.sql.sqlite\", \"sqlite/**\"]\ndialect = \"sqlite\"\n\n[[files]]\ninclude = [\"*.sql.postgres\"]\n",
	)
	.expect("write config");
	std::fs::create_dir_all(dir.join("sqlite")).expect("mkdir");
	// `?1` is a parameter in SQLite and a syntax error in Postgres.
	let sqlite = "SELECT a FROM t WHERE b = ?1;\n";
	std::fs::write(dir.join("delta.sql.sqlite"), sqlite).expect("write");
	std::fs::write(dir.join("sqlite/schema.sql"), sqlite).expect("write");
	std::fs::write(dir.join("delta.sql.postgres"), "SELECT   1;\n")
		.expect("write");
	std::fs::write(dir.join("notes.txt"), "SELECT   1;\n").expect("write");

	let output =
		squill().args(["fmt", "--strict"]).arg(&dir).output().expect("run");
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	for name in ["delta.sql.sqlite", "sqlite/schema.sql"] {
		assert_eq!(
			std::fs::read_to_string(dir.join(name)).expect("read"),
			"select a from t where b = ?1;\n",
			"{name}"
		);
	}
	assert_eq!(
		std::fs::read_to_string(dir.join("delta.sql.postgres")).expect("read"),
		"select 1;\n",
		"a [[files]] rule brings its files into recursion"
	);
	assert_eq!(
		std::fs::read_to_string(dir.join("notes.txt")).expect("read"),
		"SELECT   1;\n",
		"nothing claims notes.txt"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Rules layer in file order: a later rule without a grammar narrows
/// options for part of what an earlier rule covers.
#[test]
fn later_rules_layer_over_earlier_ones() {
	let dir = temp_dir("layering");
	std::fs::write(
		dir.join("squill.toml"),
		format!("{RUST_RULE}keyword-case = \"upper\"\n\n[[embedded]]\ninclude = [\"legacy/**\"]\nkeyword-case = \"lower\"\n"),
	)
	.expect("write config");
	std::fs::create_dir_all(dir.join("legacy")).expect("mkdir");
	let source = "fn f() {\n    sqlx::query!(r#\"select 1\"#);\n}\n";
	std::fs::write(dir.join("new.rs"), source).expect("write");
	std::fs::write(dir.join("legacy/old.rs"), source).expect("write");
	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert!(
		std::fs::read_to_string(dir.join("new.rs"))
			.expect("read")
			.contains("SELECT 1"),
	);
	assert!(
		std::fs::read_to_string(dir.join("legacy/old.rs"))
			.expect("read")
			.contains("select 1"),
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Bad rules are hard errors with a location, like any other config
/// mistake.
#[test]
fn rule_errors_have_locations() {
	let dir = temp_dir("langbad");
	std::fs::write(dir.join("f.sql"), "select 1;\n").expect("write");

	let cases = [
		// The old language sections are gone.
		("[go]\nindent = \"tabs\"\n", 1, "unknown section `[go]`"),
		("files = 1\n", 1, "write each as `[[files]]`"),
		("include = [\"x\"]\n", 1, "belongs in a [[files]] or [[embedded]] rule"),
		("[[files]]\ndialect = \"sqlite\"\n", 1, "needs an `include` list"),
		("[[files]]\ninclude = []\n", 2, "at least one pattern"),
		(
			"[[files]]\ninclude = [\"x\"]\ngrammar = \"rust\"\n",
			3,
			"belongs in an [[embedded]] rule",
		),
		(
			"[[embedded]]\ninclude = [\"x\"]\ngrammar = \"ruby\"\n",
			3,
			"unknown grammar `ruby`",
		),
		(
			"[[embedded]]\ninclude = [\"x\"]\nignore = [\"y\"]\n",
			3,
			"applies to the whole file",
		),
		(
			"[[embedded]]\ninclude = [\"x\"]\nindent = \"elephant\"\n",
			3,
			"unknown indent style",
		),
		(
			"[[embedded]]\ninclude = [\"x\"]\nindent-width = 99\n",
			3,
			"integer from 1 to 16",
		),
		("[[embedded]]\ninclude = [\"x\"]\nnope = 1\n", 3, "unknown key `nope`"),
		("[[embedded]]\ninclude = [\"x\"]\n[embedded.inner]\n", 3, "do not nest"),
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

/// A grammar squill does not bundle, loaded from wasm: the Lua grammar's
/// own release artifact (MIT, see the LICENSE beside it). Long strings
/// (`[[ ... ]]`) are raw, so their content is SQL as written.
#[cfg(feature = "external-grammars")]
#[test]
fn wasm_grammars_load_from_config() {
	let dir = temp_dir("wasmgrammar");
	let grammar = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/grammars/tree-sitter-lua.wasm");
	std::fs::write(
		dir.join("lua.scm"),
		"((function_call name: (method_index_expression method: (identifier) @_m) arguments: (arguments . (string) @sql)) (#eq? @_m \"exec\"))",
	)
	.expect("write query");
	std::fs::write(
		dir.join("squill.toml"),
		format!(
			"[[embedded]]\ninclude = [\"*.lua\"]\ngrammar = \"{}\"\nquery = \"lua.scm\"\ndialect = \"sqlite\"\n",
			grammar.display()
		),
	)
	.expect("write config");
	let file = dir.join("db.lua");
	std::fs::write(
		&file,
		"local function migrate(db)\n  db:exec([[\n    CREATE TABLE t (id INTEGER PRIMARY KEY, a TEXT)\n  ]])\n  db:exec(\"SELECT   1\")\nend\n",
	)
	.expect("write lua");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(
		std::fs::read_to_string(&file).expect("read"),
		"local function migrate(db)\n  db:exec([[\n  create table t (\n    id integer primary key,\n    a text\n  )\n  ]])\n  db:exec(\"SELECT   1\")\nend\n"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A wasm grammar has no default query to fall back on.
#[cfg(feature = "external-grammars")]
#[test]
fn wasm_grammars_need_a_query() {
	let dir = temp_dir("wasmnoquery");
	let grammar = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
		.join("tests/fixtures/grammars/tree-sitter-lua.wasm");
	std::fs::write(
		dir.join("squill.toml"),
		format!(
			"[[embedded]]\ninclude = [\"*.lua\"]\ngrammar = \"{}\"\n",
			grammar.display()
		),
	)
	.expect("write config");
	std::fs::write(dir.join("db.lua"), "print(1)\n").expect("write lua");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	assert!(
		String::from_utf8_lossy(&output.stderr)
			.contains("give its [[embedded]] rule a `query`"),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// JDBC's `?` is opt-in, by key or flag, in Java as anywhere else.
#[test]
fn question_params_are_configured() {
	let dir = temp_dir("questionparams");
	let java = "class A {\n    void m() {\n        conn.prepareStatement(\"\"\"\n            SELECT id FROM users WHERE org = ?\n            \"\"\");\n    }\n}\n";
	std::fs::write(dir.join("A.java"), java).expect("write");
	std::fs::write(dir.join("q.sql"), "SELECT id FROM users WHERE org = ?;\n")
		.expect("write");
	let rule = "[[embedded]]\ninclude = [\"*.java\"]\ngrammar = \"java\"\n";

	// Off by default: the `?` doesn't lex as a parameter, so the string
	// is left alone and reported.
	std::fs::write(dir.join("squill.toml"), rule).expect("write config");
	let output =
		squill().arg("fmt").arg(dir.join("A.java")).output().expect("run");
	assert!(String::from_utf8_lossy(&output.stderr).contains("did not parse"));
	assert_eq!(std::fs::read_to_string(dir.join("A.java")).expect("read"), java);

	// On by the rule.
	std::fs::write(
		dir.join("squill.toml"),
		format!("{rule}question-params = true\n"),
	)
	.expect("write config");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert!(output.status.success());
	assert!(
		std::fs::read_to_string(dir.join("A.java"))
			.expect("read")
			.contains("        where org = ?\n"),
	);
	// Not for plain SQL, which the rule doesn't cover: it takes the flag.
	assert_eq!(
		std::fs::read_to_string(dir.join("q.sql")).expect("read"),
		"SELECT id FROM users WHERE org = ?;\n"
	);
	let status = squill()
		.args(["fmt", "--question-params"])
		.arg(dir.join("q.sql"))
		.status()
		.expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("q.sql")).expect("read"),
		"select id from users where org = ?;\n"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Run `squill init` in `dir` (no terminal, so no questions), returning
/// its exit code and stderr.
fn init_in(dir: &std::path::Path, extra: &[&str]) -> (Option<i32>, String) {
	let output = squill()
		.current_dir(dir)
		.arg("init")
		.args(extra)
		.stdin(Stdio::null())
		.output()
		.expect("run");
	(output.status.code(), String::from_utf8_lossy(&output.stderr).into_owned())
}

/// Without a terminal, init takes the defaults: every language whose
/// default query finds SQL, in the `--dialect` dialect.
#[test]
fn init_defaults_to_the_languages_that_hold_sql() {
	let dir = temp_dir("init");
	std::fs::write(dir.join("q.rs"), RS_FIXTURE).expect("write");
	std::fs::write(
		dir.join("q.go"),
		"package main\n\nfunc f(db *sql.DB) {\n\tdb.Exec(`DELETE FROM t`)\n}\n",
	)
	.expect("write");
	// Python without any SQL: not chosen.
	std::fs::write(dir.join("app.py"), "print('hello')\n").expect("write");

	let (code, stderr) = init_in(&dir, &["--dialect", "sqlite"]);
	assert_eq!(code, Some(0), "{stderr}");
	assert!(
		stderr.contains("Formatting SQL in Rust: found 1 SQL string in 1 file."),
		"{stderr}"
	);
	assert!(!stderr.contains("Python"), "{stderr}");
	assert_eq!(
		std::fs::read_to_string(dir.join("squill.toml")).expect("read"),
		"# squill configuration: https://mckayla.dev/squill/docs/configuration/\n\n\
		 dialect = \"sqlite\"\n\n\
		 [[embedded]]\ninclude = [\"**/*.rs\"]\ngrammar = \"rust\"\n\n\
		 [[embedded]]\ninclude = [\"**/*.go\"]\ngrammar = \"go\"\n"
	);

	// The config it wrote is one `fmt` accepts, and it formats the Go.
	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	assert!(
		std::fs::read_to_string(dir.join("q.go"))
			.expect("read")
			.contains("delete from t"),
	);

	// A second run refuses to overwrite it.
	let (code, stderr) = init_in(&dir, &[]);
	assert_eq!(code, Some(2));
	assert!(stderr.contains("already exists"), "{stderr}");
	let _ = std::fs::remove_dir_all(&dir);
}

/// A rule's `include` names the extensions the project uses, not every
/// one the language might: here `.cc` sources with `.h` headers.
#[cfg(feature = "cxx")]
#[test]
fn init_includes_the_extensions_it_finds() {
	let dir = temp_dir("initcxx");
	std::fs::write(
		dir.join("db.cc"),
		"void f(sqlite3 *db) {\n  sqlite3_exec(db, R\"(\n    DELETE FROM t\n  )\", 0, 0, 0);\n}\n",
	)
	.expect("write");
	std::fs::write(dir.join("db.h"), "void f(sqlite3 *db);\n").expect("write");
	let (code, stderr) = init_in(&dir, &["--yes"]);
	assert_eq!(code, Some(0), "{stderr}");
	let config = std::fs::read_to_string(dir.join("squill.toml")).expect("read");
	assert!(
		config.contains("include = [\"**/*.cc\", \"**/*.h\"]\ngrammar = \"c++\""),
		"{config}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A new project: nothing to find, so just the dialect.
#[test]
fn init_in_an_empty_project() {
	let dir = temp_dir("initempty");
	let (code, stderr) = init_in(&dir, &["--yes"]);
	assert_eq!(code, Some(0), "{stderr}");
	assert_eq!(
		std::fs::read_to_string(dir.join("squill.toml")).expect("read"),
		"# squill configuration: https://mckayla.dev/squill/docs/configuration/\n\ndialect = \"postgres\"\n"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// psycopg's `%s` is opt-in, by key or flag, in Python as anywhere else.
#[test]
fn pyformat_params_are_configured() {
	let dir = temp_dir("pyformatparams");
	let python = "def f(cur):\n    cur.execute(\"\"\"SELECT id FROM users WHERE org = %(org)s\"\"\")\n";
	std::fs::write(dir.join("q.py"), python).expect("write");
	std::fs::write(dir.join("q.sql"), "SELECT id FROM users WHERE org = %s;\n")
		.expect("write");
	let rule = "[[embedded]]\ninclude = [\"*.py\"]\ngrammar = \"python\"\n";

	// Off by default: the placeholder doesn't lex as one.
	std::fs::write(dir.join("squill.toml"), rule).expect("write config");
	let output = squill().arg("fmt").arg(dir.join("q.py")).output().expect("run");
	assert!(
		String::from_utf8_lossy(&output.stderr).contains("did not parse"),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(std::fs::read_to_string(dir.join("q.py")).expect("read"), python);

	// On by the rule.
	std::fs::write(
		dir.join("squill.toml"),
		format!("{rule}pyformat-params = true\n"),
	)
	.expect("write config");
	let status = squill().arg("fmt").arg(dir.join("q.py")).status().expect("run");
	assert!(status.success());
	assert!(
		std::fs::read_to_string(dir.join("q.py"))
			.expect("read")
			.contains("    where org = %(org)s\n"),
	);
	// Plain SQL takes the flag.
	let status = squill()
		.args(["fmt", "--pyformat-params"])
		.arg(dir.join("q.sql"))
		.status()
		.expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("q.sql")).expect("read"),
		"select id from users where org = %s;\n"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Pipe `input` through `squill fmt --stdin-filepath <path>` in `dir`,
/// returning exit code, stdout, and stderr.
fn stdin_as(
	dir: &std::path::Path,
	path: &str,
	input: &str,
	extra: &[&str],
) -> (Option<i32>, String, String) {
	let mut child = squill()
		.current_dir(dir)
		.args(["fmt", "--stdin-filepath", path])
		.args(extra)
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.stderr(Stdio::piped())
		.spawn()
		.expect("spawn");
	child
		.stdin
		.take()
		.expect("stdin")
		.write_all(input.as_bytes())
		.expect("write");
	let output = child.wait_with_output().expect("wait");
	(
		output.status.code(),
		String::from_utf8_lossy(&output.stdout).into_owned(),
		String::from_utf8_lossy(&output.stderr).into_owned(),
	)
}

/// `--stdin-filepath` formats the stream as the file at that path would
/// be — no file needed on disk, as for an unsaved editor buffer.
#[test]
fn stdin_filepath_resolves_rules_against_the_path() {
	let dir = temp_dir("stdinpath");
	std::fs::write(
		dir.join("squill.toml"),
		format!(
			"{RUST_RULE}\n[[files]]\ninclude = [\"*.sql.sqlite\"]\ndialect = \"sqlite\"\n"
		),
	)
	.expect("write config");

	// A host file: its embedded SQL formats.
	let (code, out, _) = stdin_as(&dir, "src/q.rs", RS_FIXTURE, &[]);
	assert_eq!(code, Some(0));
	assert!(out.contains("select id, name\n        from users"), "{out}");

	// A [[files]] rule applies: `?1` only lexes in SQLite.
	let (code, out, err) = stdin_as(
		&dir,
		"delta.sql.sqlite",
		"SELECT a FROM t WHERE b = ?1",
		&["--strict"],
	);
	assert_eq!(code, Some(0), "{err}");
	assert_eq!(out, "select a from t where b = ?1;\n");

	// Diagnostics carry the path.
	let (_, _, err) = stdin_as(
		&dir,
		"src/bad.rs",
		"fn f() { sqlx::query!(r#\"select (\n\"#); }\n",
		&[],
	);
	assert!(err.contains("src/bad.rs:1:23: embedded SQL did not parse"), "{err}");
	let _ = std::fs::remove_dir_all(&dir);
}

/// What squill wouldn't format as a file, it hands back unchanged from
/// stdin: an ignored path, or a file neither named *.sql nor covered by
/// a rule.
#[test]
fn stdin_filepath_passes_through_what_it_would_not_format() {
	let dir = temp_dir("stdinpassthrough");
	std::fs::write(dir.join("squill.toml"), "ignore = [\"generated/**\"]\n")
		.expect("write config");
	let messy = "SELECT   1;";
	let (code, out, _) = stdin_as(&dir, "generated/q.sql", messy, &[]);
	assert_eq!((code, out.as_str()), (Some(0), messy));
	let (code, out, _) = stdin_as(&dir, "src/q.rs", RS_FIXTURE, &[]);
	assert_eq!((code, out.as_str()), (Some(0), RS_FIXTURE));
	// Nor is anything else not named *.sql: a C header, say.
	let (code, out, _) = stdin_as(&dir, "row.h", messy, &[]);
	assert_eq!((code, out.as_str()), (Some(0), messy));
	// A [[files]] rule makes it SQL.
	std::fs::write(
		dir.join("squill.toml"),
		"ignore = [\"generated/**\"]\n\n[[files]]\ninclude = [\"*.pgsql\"]\n",
	)
	.expect("write config");
	let (code, out, _) = stdin_as(&dir, "notes.pgsql", messy, &[]);
	assert_eq!((code, out.as_str()), (Some(0), "select 1;\n"));
	let _ = std::fs::remove_dir_all(&dir);
}

/// Talk to `squill language-server start` over stdio: send `messages` (each framed with a
/// Content-Length header), then read everything it writes back.
#[cfg(feature = "lsp")]
fn lsp_session(
	dir: &std::path::Path,
	messages: &[serde_json::Value],
) -> Vec<serde_json::Value> {
	let mut child = squill()
		.current_dir(dir)
		.args(["language-server", "start"])
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.spawn()
		.expect("spawn");
	let mut stdin = child.stdin.take().expect("stdin");
	for message in messages {
		let body = message.to_string();
		write!(stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
			.expect("write");
	}
	drop(stdin);
	let output = child.wait_with_output().expect("wait");
	assert!(
		output.status.success(),
		"the language server exited with {}",
		output.status
	);
	let mut out = &output.stdout[..];
	let mut replies = Vec::new();
	while let Some(at) = out.windows(4).position(|w| w == b"\r\n\r\n") {
		let header = std::str::from_utf8(&out[..at]).expect("header");
		let length: usize = header
			.lines()
			.find_map(|line| line.strip_prefix("Content-Length: "))
			.expect("content length")
			.trim()
			.parse()
			.expect("length");
		let body = &out[at + 4..at + 4 + length];
		replies.push(serde_json::from_slice(body).expect("json"));
		out = &out[at + 4 + length..];
	}
	replies
}

/// The language server formats a document by its path, the way
/// --stdin-filepath would, and publishes squill's diagnostics.
#[cfg(feature = "lsp")]
#[test]
fn language_server_formats_and_reports() {
	use serde_json::json;
	let dir = temp_dir("lsp");
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
	let uri = |name: &str| format!("file://{}", dir.join(name).display());
	let open = |name: &str, language: &str, text: &str| {
		json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
			"textDocument": {"uri": uri(name), "languageId": language, "version": 1, "text": text}
		}})
	};
	let replies = lsp_session(
		&dir,
		&[
			json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}),
			json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
			open("q.sql", "sql", "SELECT   1"),
			json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/formatting", "params": {
				"textDocument": {"uri": uri("q.sql")},
				"options": {"tabSize": 4, "insertSpaces": false}
			}}),
			// Embedded SQL that doesn't parse: a published warning.
			open(
				"bad.rs",
				"rust",
				"fn f() {\n    sqlx::query!(r#\"select (\n\"#);\n}\n",
			),
			// A C header: an extension no grammar claims, in a language
			// that isn't SQL. Left alone, not read as SQL.
			open("row.h", "cpp", "struct row { int id; };\n"),
			json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/formatting", "params": {
				"textDocument": {"uri": uri("row.h")},
				"options": {"tabSize": 4, "insertSpaces": false}
			}}),
			json!({"jsonrpc": "2.0", "id": 4, "method": "shutdown"}),
			json!({"jsonrpc": "2.0", "method": "exit"}),
		],
	);
	let reply = |id: i64| {
		replies.iter().find(|reply| reply["id"] == id).expect("reply").clone()
	};
	assert_eq!(
		reply(1)["result"]["capabilities"]["documentFormattingProvider"],
		true
	);
	assert_eq!(reply(1)["result"]["serverInfo"]["name"], "squill");
	let edits = &reply(2)["result"];
	assert_eq!(edits[0]["newText"], "select 1;\n", "{edits}");
	assert_eq!(edits[0]["range"]["end"], json!({"line": 0, "character": 10}));

	let published: Vec<_> = replies
		.iter()
		.filter(|reply| reply["method"] == "textDocument/publishDiagnostics")
		.filter(|reply| reply["params"]["uri"] == uri("bad.rs"))
		.collect();
	let diagnostics =
		&published.last().expect("diagnostics for bad.rs")["params"]["diagnostics"];
	assert_eq!(diagnostics[0]["severity"], 2, "{diagnostics}");
	assert_eq!(
		diagnostics[0]["range"]["start"],
		json!({"line": 1, "character": 17})
	);
	assert!(
		diagnostics[0]["message"]
			.as_str()
			.is_some_and(|message| message.contains("did not parse")),
		"{diagnostics}"
	);

	assert_eq!(reply(3)["result"], json!([]));
	let header = replies
		.iter()
		.filter(|reply| reply["method"] == "textDocument/publishDiagnostics")
		.find(|reply| reply["params"]["uri"] == uri("row.h"))
		.expect("diagnostics for row.h");
	assert_eq!(header["params"]["diagnostics"], json!([]), "{header}");
	let _ = std::fs::remove_dir_all(&dir);
}

/// For clients that run one formatter per file: formatting (and
/// highlighting) for just the documents asked about, and a
/// `source.formatSql` code action to run after another language's
/// formatter.
#[cfg(feature = "lsp")]
#[test]
fn language_server_serves_one_formatter_editors() {
	use serde_json::json;
	let dir = temp_dir("lspselector");
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
	let uri = format!("file://{}", dir.join("q.rs").display());
	let replies = lsp_session(
		&dir,
		&[
			json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
				"capabilities": {"textDocument": {"formatting": {"dynamicRegistration": true}}},
				"initializationOptions": {
					"formattingSelector": [{"language": "sql"}],
					"semanticTokensSelector": []
				}
			}}),
			json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
			json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
				"textDocument": {"uri": uri, "languageId": "rust", "version": 1, "text": RS_FIXTURE}
			}}),
			json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction", "params": {
				"textDocument": {"uri": uri},
				"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
				"context": {"diagnostics": [], "only": ["source.formatSql"]}
			}}),
			// Asked only for other kinds: nothing.
			json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/codeAction", "params": {
				"textDocument": {"uri": uri},
				"range": {"start": {"line": 0, "character": 0}, "end": {"line": 0, "character": 0}},
				"context": {"diagnostics": [], "only": ["quickfix"]}
			}}),
			json!({"jsonrpc": "2.0", "id": 4, "method": "shutdown"}),
			json!({"jsonrpc": "2.0", "method": "exit"}),
		],
	);
	let reply = |id: i64| {
		replies.iter().find(|reply| reply["id"] == id).expect("reply").clone()
	};
	let capabilities = &reply(1)["result"]["capabilities"];
	assert!(
		capabilities["documentFormattingProvider"].is_null(),
		"{capabilities}"
	);
	// Highlighting for no documents, but the legend is still there for a
	// client that asks for tokens itself.
	let tokens = &capabilities["semanticTokensProvider"];
	assert_eq!(tokens["documentSelector"], json!([]), "{capabilities}");
	assert!(tokens["legend"]["tokenTypes"].is_array(), "{capabilities}");
	let registration = replies
		.iter()
		.find(|reply| reply["method"] == "client/registerCapability")
		.expect("a registration");
	assert_eq!(
		registration["params"]["registrations"][0]["registerOptions"]["documentSelector"],
		json!([{"language": "sql"}])
	);

	let action = &reply(2)["result"][0];
	assert_eq!(action["kind"], "source.formatSql");
	let edit = &action["edit"]["changes"][uri.as_str()][0]["newText"];
	assert!(
		edit
			.as_str()
			.is_some_and(|text| text.contains("select id, name\n        from users")),
		"{action}"
	);
	assert_eq!(reply(3)["result"], json!([]));
	let _ = std::fs::remove_dir_all(&dir);
}

/// The language server highlights the SQL a host file's rule finds, and
/// nothing else in the file. SQL files it leaves to the editor.
#[cfg(feature = "lsp")]
#[test]
fn language_server_highlights_embedded_sql() {
	use serde_json::json;
	let dir = temp_dir("lsptokens");
	std::fs::write(dir.join("squill.toml"), RUST_RULE).expect("write config");
	let uri = format!("file://{}", dir.join("q.rs").display());
	let sql_uri = format!("file://{}", dir.join("q.sql").display());
	let replies = lsp_session(
		&dir,
		&[
			json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {"capabilities": {}}}),
			json!({"jsonrpc": "2.0", "method": "initialized", "params": {}}),
			json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
				"textDocument": {"uri": uri, "languageId": "rust", "version": 1, "text": RS_FIXTURE}
			}}),
			json!({"jsonrpc": "2.0", "id": 2, "method": "textDocument/semanticTokens/full", "params": {
				"textDocument": {"uri": uri}
			}}),
			json!({"jsonrpc": "2.0", "method": "textDocument/didOpen", "params": {
				"textDocument": {"uri": sql_uri, "languageId": "sql", "version": 1, "text": "select 1;"}
			}}),
			json!({"jsonrpc": "2.0", "id": 3, "method": "textDocument/semanticTokens/full", "params": {
				"textDocument": {"uri": sql_uri}
			}}),
			json!({"jsonrpc": "2.0", "id": 4, "method": "shutdown"}),
			json!({"jsonrpc": "2.0", "method": "exit"}),
		],
	);
	let reply = |id: i64| {
		replies.iter().find(|reply| reply["id"] == id).expect("reply").clone()
	};
	let legend = reply(1)["result"]["capabilities"]["semanticTokensProvider"]
		["legend"]["tokenTypes"]
		.clone();
	let data: Vec<u64> = reply(2)["result"]["data"]
		.as_array()
		.expect("tokens")
		.iter()
		.map(|n| n.as_u64().expect("a number"))
		.collect();
	// Undo the relative encoding: each token's text, and its type's name.
	let lines: Vec<&str> = RS_FIXTURE.lines().collect();
	let (mut line, mut column) = (0, 0);
	let mut tokens = Vec::new();
	for token in data.chunks(5) {
		if token[0] > 0 {
			column = 0;
		}
		line += token[0] as usize;
		column += token[1] as usize;
		let text = &lines[line][column..column + token[2] as usize];
		let kind = legend[token[3] as usize].as_str().expect("a type name");
		tokens.push((text, kind));
	}
	assert_eq!(
		tokens,
		[
			("SELECT", "keyword"),
			("id", "variable"),
			("name", "variable"),
			("FROM", "keyword"),
			("users", "variable"),
			("WHERE", "keyword"),
			("org", "variable"),
			("=", "operator"),
			("$1", "parameter"),
			("ORDER", "keyword"),
			("BY", "keyword"),
			("name", "variable"),
		]
	);
	assert!(reply(3)["result"].is_null(), "{}", reply(3));
	let _ = std::fs::remove_dir_all(&dir);
}

/// A bare file name, run from its own directory, finds the config there:
/// its parent is the working directory, not an empty path.
#[test]
fn bare_file_names_find_their_config() {
	let dir = temp_dir("barename");
	std::fs::write(dir.join("squill.toml"), "keyword-case = \"upper\"\n")
		.expect("write config");
	std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");
	let status =
		squill().current_dir(&dir).args(["fmt", "a.sql"]).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("a.sql")).expect("read"),
		"SELECT 1;\n"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A grammar URL locked in squill.lock and already cached never touches
/// the network (the URL here doesn't resolve); one the lockfile doesn't
/// have is refused under --locked.
#[cfg(feature = "external-grammars")]
#[test]
fn grammar_urls_use_the_lockfile_and_cache() {
	use sha2::Digest;
	let dir = temp_dir("grammarurl");
	let wasm = std::fs::read(
		std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
			.join("tests/fixtures/grammars/tree-sitter-lua.wasm"),
	)
	.expect("read grammar");
	let sha: String = sha2::Sha256::digest(&wasm)
		.iter()
		.map(|byte| format!("{byte:02x}"))
		.collect();
	let url = "https://grammars.example.invalid/tree-sitter-lua.wasm";
	let cache = dir.join("cache/squill/grammars");
	std::fs::create_dir_all(&cache).expect("mkdir cache");
	std::fs::write(cache.join(format!("{sha}.wasm")), &wasm)
		.expect("write cache");
	std::fs::write(
		dir.join("lua.scm"),
		"((function_call name: (method_index_expression method: (identifier) @_m) arguments: (arguments . (string) @sql)) (#eq? @_m \"exec\"))",
	)
	.expect("write query");
	std::fs::write(
		dir.join("squill.toml"),
		format!("[[embedded]]\ninclude = [\"*.lua\"]\ngrammar = \"{url}\"\nquery = \"lua.scm\"\n"),
	)
	.expect("write config");
	std::fs::write(
		dir.join("squill.lock"),
		format!("[grammars]\n\"{url}\" = \"{sha}\"\n"),
	)
	.expect("write lock");
	std::fs::write(dir.join("db.lua"), "db:exec([[\n  SELECT   1\n]])\n")
		.expect("write lua");

	let run = |args: &[&str]| {
		squill()
			.current_dir(&dir)
			.env("XDG_CACHE_HOME", dir.join("cache"))
			.args(args)
			.output()
			.expect("run")
	};
	let output = run(&["fmt", "--locked", "db.lua"]);
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(
		std::fs::read_to_string(dir.join("db.lua")).expect("read"),
		"db:exec([[\nselect 1\n]])\n"
	);

	std::fs::remove_file(dir.join("squill.lock")).expect("remove lock");
	let output = run(&["fmt", "--locked", "db.lua"]);
	assert_eq!(output.status.code(), Some(2));
	assert!(
		String::from_utf8_lossy(&output.stderr).contains("--locked won't add it"),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// A function body squill can't parse is a diagnostic, not a silent
/// pass-through, and fails --strict.
#[test]
fn unparsable_function_bodies_are_diagnostics() {
	let dir = temp_dir("badbody");
	let file = dir.join("f.sql");
	std::fs::write(
		&file,
		"create function f() returns int language plpgsql as $$\nbegin\n  frobnicate 1;\nend\n$$;\n",
	)
	.expect("write");
	let output =
		squill().args(["fmt", "--check"]).arg(&file).output().expect("run");
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("f.sql:3:3: expected a statement, found `frobnicate` (PL/pgSQL body left as written)"),
		"{stderr}"
	);
	let status =
		squill().args(["fmt", "--strict"]).arg(&file).status().expect("run");
	assert_eq!(status.code(), Some(1));
	let _ = std::fs::remove_dir_all(&dir);
}

/// squill.yaml (or .yml) works like squill.toml: the same keys and
/// rules, and errors with line numbers.
#[test]
fn yaml_configs() {
	let dir = temp_dir("yaml");
	std::fs::write(
		dir.join("squill.yaml"),
		"keyword-case: upper\nfiles:\n  - include: [\"*.sql.sqlite\"]\n    dialect: sqlite\n",
	)
	.expect("write config");
	std::fs::write(dir.join("q.sql.sqlite"), "select a from t where b = ?1;\n")
		.expect("write");
	let status =
		squill().args(["fmt", "--strict"]).arg(&dir).status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("q.sql.sqlite")).expect("read"),
		"SELECT a FROM t WHERE b = ?1;\n"
	);

	std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");
	let cases = [
		("nope: 1\n", 1, "unknown key `nope`"),
		("indent-width: 99\n", 1, "integer from 1 to 16"),
		(
			"files:\n  - dialect: sqlite\n",
			2,
			"a `files` rule needs an `include` list",
		),
		("files:\n  include: [\"x\"]\n", 1, "write them as a list under `files:`"),
		("- a\n- b\n", 1, "the top level must be a mapping"),
		("dialect: postgres\n  oops: 1\n", 2, "mapping values are not allowed"),
	];
	for (config, line, needle) in cases {
		std::fs::write(dir.join("squill.yaml"), config).expect("write config");
		let output =
			squill().arg("fmt").arg(dir.join("a.sql")).output().expect("run");
		assert_eq!(output.status.code(), Some(2), "config accepted: {config}");
		let stderr = String::from_utf8_lossy(&output.stderr);
		assert!(
			stderr.contains(&format!("squill.yaml:{line}:"))
				&& stderr.contains(needle),
			"for {config:?} got: {stderr}"
		);
	}
	let _ = std::fs::remove_dir_all(&dir);
}

/// Two configs in one place is an error, not a guess.
#[test]
fn two_configs_in_one_directory_are_refused() {
	let dir = temp_dir("twoconfigs");
	std::fs::write(dir.join("squill.toml"), "dialect = \"postgres\"\n")
		.expect("write");
	std::fs::write(dir.join("squill.yml"), "dialect: sqlite\n").expect("write");
	std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	let stderr = String::from_utf8_lossy(&output.stderr);
	assert!(
		stderr.contains("squill.toml and")
			&& stderr.contains("squill.yml are both configs"),
		"{stderr}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn init_writes_yaml_on_request() {
	let dir = temp_dir("inityaml");
	std::fs::write(dir.join("q.rs"), RS_FIXTURE).expect("write");
	let (code, stderr) = init_in(&dir, &["--yaml", "--dialect", "sqlite"]);
	assert_eq!(code, Some(0), "{stderr}");
	assert!(!dir.join("squill.toml").exists());
	assert_eq!(
		std::fs::read_to_string(dir.join("squill.yaml")).expect("read"),
		"# squill configuration: https://mckayla.dev/squill/docs/configuration/\n\n\
		 dialect: sqlite\n\nembedded:\n  - include: [\"**/*.rs\"]\n    grammar: rust\n"
	);
	// fmt reads it back.
	let status = squill().arg("fmt").arg(&dir).status().expect("run");
	assert!(status.success());
	// And init won't write a second config beside it.
	let (code, stderr) = init_in(&dir, &[]);
	assert_eq!(code, Some(2));
	assert!(stderr.contains("squill.yaml already exists"), "{stderr}");
	let _ = std::fs::remove_dir_all(&dir);
}

/// No grammar turns a `*-params` option on by itself: Dapper's `@name`
/// in C# needs `at-params`, like anywhere else.
#[test]
fn params_are_off_until_configured() {
	let dir = temp_dir("csharpat");
	let source = "class A {\n    void M() {\n        conn.Execute(\"\"\"\n            DELETE FROM t WHERE id=@id OR id IN @ids\n            \"\"\");\n    }\n}\n";
	std::fs::write(dir.join("a.cs"), source).expect("write");
	let rule = "[[embedded]]\ninclude = [\"*.cs\"]\ngrammar = \"c#\"\n";
	std::fs::write(dir.join("squill.toml"), rule).expect("write config");
	let run = || {
		squill()
			.args(["fmt", "--strict", "--stdout"])
			.arg(dir.join("a.cs"))
			.output()
			.expect("run")
	};
	let output = run();
	assert_eq!(output.status.code(), Some(1));
	assert_eq!(String::from_utf8_lossy(&output.stdout), source);

	std::fs::write(dir.join("squill.toml"), format!("{rule}at-params = true\n"))
		.expect("write config");
	let output = run();
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert!(
		String::from_utf8_lossy(&output.stdout)
			.contains("where id = @id or id in @ids"),
		"{}",
		String::from_utf8_lossy(&output.stdout)
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// `squill locate` lists what a rule's query captures, formattable or
/// not, and a plain SQL file whole — as lines, or as JSON.
#[test]
fn locate_lists_the_sql() {
	let dir = temp_dir("locate");
	std::fs::write(
		dir.join("squill.toml"),
		"[[embedded]]\ninclude = [\"*.rs\"]\ngrammar = \"rust\"\n",
	)
	.expect("write config");
	std::fs::write(
		dir.join("db.rs"),
		"fn f() {\n    sqlx::query(r#\"SELECT 1\"#);\n    sqlx::query(\"frobnicate\");\n}\n",
	)
	.expect("write");
	std::fs::write(dir.join("a.sql"), "select 1;\n").expect("write");
	let run = |args: &[&str]| {
		let output = squill().current_dir(&dir).args(args).output().expect("run");
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		String::from_utf8_lossy(&output.stdout).into_owned()
	};
	assert_eq!(
		run(&["locate", "db.rs", "a.sql"]),
		"a.sql:1:1-2:1 postgres  select 1;\n\
		 db.rs:2:20-2:28 postgres  SELECT 1\n\
		 db.rs:3:18-3:28 postgres  frobnicate\n"
	);
	assert_eq!(
		run(&["locate", "--json", "db.rs"]),
		"{\"path\":\"db.rs\",\"start\":28,\"end\":36,\"line\":2,\"column\":20,\"end_line\":2,\"end_column\":28,\"dialect\":\"postgres\",\"pinned_dialect\":false}\n\
		 {\"path\":\"db.rs\",\"start\":58,\"end\":68,\"line\":3,\"column\":18,\"end_line\":3,\"end_column\":28,\"dialect\":\"postgres\",\"pinned_dialect\":false}\n"
	);
	// Nothing is written.
	assert!(
		std::fs::read_to_string(dir.join("db.rs"))
			.expect("read")
			.contains("SELECT 1")
	);
	// Formatting flags aren't for locate.
	let output = squill()
		.current_dir(&dir)
		.args(["locate", "--check", "db.rs"])
		.output()
		.expect("run");
	assert_eq!(output.status.code(), Some(2));
	let _ = std::fs::remove_dir_all(&dir);
}

/// A SQL file's last statement gains its `;`, embedded SQL's loses it,
/// and a rule (or the flag) can choose otherwise.
#[test]
fn trailing_semicolons_by_kind() {
	let dir = temp_dir("semicolons");
	std::fs::write(
		dir.join("squill.toml"),
		"[[embedded]]\ninclude = [\"*.rs\"]\ngrammar = \"rust\"\n\n\
		 [[embedded]]\ninclude = [\"keep.rs\"]\ntrailing-semicolons = \"always\"\n",
	)
	.expect("write config");
	let rust = "fn f() {\n    sqlx::query(r\"select 1;\");\n}\n";
	std::fs::write(dir.join("db.rs"), rust).expect("write");
	std::fs::write(dir.join("keep.rs"), rust).expect("write");
	std::fs::write(dir.join("a.sql"), "select 1; select 2\n").expect("write");
	let run = |args: &[&str]| {
		let output = squill().current_dir(&dir).args(args).output().expect("run");
		assert!(
			output.status.success(),
			"{}",
			String::from_utf8_lossy(&output.stderr)
		);
		String::from_utf8_lossy(&output.stdout).into_owned()
	};
	assert_eq!(run(&["fmt", "--stdout", "a.sql"]), "select 1;\nselect 2;\n");
	assert_eq!(
		run(&["fmt", "--stdout", "--trailing-semicolons", "none", "a.sql"]),
		"select 1;\nselect 2\n"
	);
	assert!(
		run(&["fmt", "--stdout", "db.rs"]).contains("r\"\n    select 1\n    \"")
	);
	assert!(
		run(&["fmt", "--stdout", "keep.rs"]).contains("r\"\n    select 1;\n    \"")
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// LF unless `line-ending` (or the flag) asks for CRLF, whatever the
/// input had; not an `[[embedded]]` rule's to set.
#[test]
fn line_ending_is_lf_unless_configured() {
	let dir = temp_dir("lineending");
	std::fs::write(dir.join("a.sql"), "select 1;\r\nselect 2;\r\n")
		.expect("write");
	let run = |args: &[&str]| {
		let output = squill().current_dir(&dir).args(args).output().expect("run");
		(
			output.status.success(),
			String::from_utf8_lossy(&output.stdout).into_owned(),
			String::from_utf8_lossy(&output.stderr).into_owned(),
		)
	};
	assert_eq!(run(&["fmt", "--stdout", "a.sql"]).1, "select 1;\nselect 2;\n");
	assert_eq!(
		run(&["fmt", "--stdout", "--line-ending", "crlf", "a.sql"]).1,
		"select 1;\r\nselect 2;\r\n"
	);
	std::fs::write(dir.join("squill.toml"), "line-ending = \"crlf\"\n")
		.expect("write config");
	assert_eq!(
		run(&["fmt", "--stdout", "a.sql"]).1,
		"select 1;\r\nselect 2;\r\n"
	);
	assert_eq!(
		run(&["fmt", "--stdout", "--line-ending", "lf", "a.sql"]).1,
		"select 1;\nselect 2;\n"
	);
	std::fs::write(
		dir.join("squill.toml"),
		"[[embedded]]\ninclude = [\"*.rs\"]\ngrammar = \"rust\"\nline-ending = \"crlf\"\n",
	)
	.expect("write config");
	let (ok, _, stderr) = run(&["fmt", "--stdout", "a.sql"]);
	assert!(
		!ok && stderr.contains("`line-ending` applies to SQL files"),
		"{stderr}"
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// SQLAlchemy's, Spring's, and JPA's `:name` placeholders, by key.
#[test]
fn colon_params_are_configured() {
	let dir = temp_dir("colonparams");
	let python = "def f(session):\n    session.execute(text(\"\"\"SELECT id FROM users WHERE org = :org AND tags[1:2] = :tags\"\"\"))\n";
	std::fs::write(dir.join("q.py"), python).expect("write");
	std::fs::write(
		dir.join("squill.toml"),
		"[[embedded]]\ninclude = [\"*.py\"]\ngrammar = \"python\"\ncolon-params = true\n",
	)
	.expect("write config");
	let output = squill()
		.args(["fmt", "--strict", "--stdout"])
		.arg(dir.join("q.py"))
		.output()
		.expect("run");
	assert!(
		output.status.success(),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert!(
		String::from_utf8_lossy(&output.stdout)
			.contains("    where org = :org and tags[1:2] = :tags\n"),
		"{}",
		String::from_utf8_lossy(&output.stdout)
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// Embedded SQL indents by the host file's step unless `indent` or
/// `indent-width` is configured, at the top level or in the rule.
#[test]
fn configured_indent_beats_the_hosts() {
	let dir = temp_dir("indentprecedence");
	std::fs::write(
		dir.join("a.rs"),
		"fn f() {\n    if x {\n        sqlx::query!(r#\"select a from t where alpha = 1 and beta = 2 or alpha = 3 and beta = 4 or alpha = 5 and gamma = 6\"#);\n    }\n}\n",
	)
	.expect("write");
	let rule = "[[embedded]]\ninclude = [\"*.rs\"]\ngrammar = \"rust\"\n";
	for (config, expected) in [
		(rule.to_string(), "\n        where\n            alpha = 1"),
		(
			format!("indent-width = 2\n{rule}"),
			"\n        where\n          alpha = 1",
		),
		(
			format!("{rule}indent-width = 2\n"),
			"\n        where\n          alpha = 1",
		),
		(
			format!("{rule}indent = \"tabs\"\n"),
			"\n        where\n        \talpha = 1",
		),
	] {
		std::fs::write(dir.join("squill.toml"), &config).expect("write config");
		let output = squill()
			.args(["fmt", "--stdout"])
			.arg(dir.join("a.rs"))
			.output()
			.expect("run");
		let stdout = String::from_utf8_lossy(&output.stdout);
		assert!(stdout.contains(expected), "{config}\n{stdout}");
	}
	let _ = std::fs::remove_dir_all(&dir);
}

/// `fmt` is the default command: a path, `-`, or a flag first means fmt.
#[test]
fn fmt_is_the_default_command() {
	let dir = temp_dir("implicitfmt");
	std::fs::write(dir.join("q.sql"), "SELECT   1;\n").expect("write");

	// `squill --check .`, from inside the directory: the file would change.
	let output =
		squill().current_dir(&dir).args(["--check", "."]).output().expect("run");
	assert_eq!(output.status.code(), Some(1));
	// `squill .` formats it.
	let status = squill().current_dir(&dir).arg(".").status().expect("run");
	assert!(status.success());
	assert_eq!(
		std::fs::read_to_string(dir.join("q.sql")).expect("read"),
		"select 1;\n"
	);
	// `squill -` reads stdin.
	let mut child = squill()
		.arg("-")
		.stdin(Stdio::piped())
		.stdout(Stdio::piped())
		.spawn()
		.expect("spawn");
	child
		.stdin
		.take()
		.expect("stdin")
		.write_all(b"SELECT   2;\n")
		.expect("write");
	let output = child.wait_with_output().expect("wait");
	assert_eq!(String::from_utf8_lossy(&output.stdout), "select 2;\n");
	// A bare word that isn't a command or a file is a mistyped command.
	let output = squill().current_dir(&dir).arg("fmtt").output().expect("run");
	assert_eq!(output.status.code(), Some(2));
	assert!(
		String::from_utf8_lossy(&output.stderr).contains("unknown command `fmtt`"),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	let _ = std::fs::remove_dir_all(&dir);
}

/// `squill help` and `squill version` answer like `--help` and
/// `--version`; `squill help <command>` like `<command> --help`.
#[test]
fn help_and_version_commands() {
	let stdout = |args: &[&str]| {
		let output = squill().args(args).output().expect("run");
		assert!(output.status.success(), "{args:?}");
		String::from_utf8_lossy(&output.stdout).into_owned()
	};
	assert_eq!(stdout(&["version"]), stdout(&["--version"]));
	assert_eq!(stdout(&["help"]), stdout(&["--help"]));
	assert_eq!(stdout(&["help", "locate"]), stdout(&["locate", "--help"]));
	assert_eq!(stdout(&["help", "init"]), stdout(&["init", "--help"]));
	let output = squill().args(["help", "frobnicate"]).output().expect("run");
	assert_eq!(output.status.code(), Some(2));
}
