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

/// A host file no rule covers is an error when named explicitly — with
/// the rule to add — rather than a wall of SQL parse errors.
#[test]
fn explicit_host_path_without_a_rule_explains_itself() {
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
		stderr.contains("no [[embedded]] rule covers this file")
			&& stderr.contains("grammar = \"rust\""),
		"{stderr}"
	);
	assert_eq!(std::fs::read_to_string(&file).expect("read"), RS_FIXTURE);
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

/// JDBC's `?` is on for Java and Kotlin unless configured; elsewhere
/// it is opt-in, by key or flag.
#[test]
fn question_params_default_by_grammar_and_configure() {
	let dir = temp_dir("questionparams");
	let java = "class A {\n    void m() {\n        conn.prepareStatement(\"\"\"\n            SELECT id FROM users WHERE org = ?\n            \"\"\");\n    }\n}\n";
	std::fs::write(dir.join("A.java"), java).expect("write");
	std::fs::write(dir.join("q.sql"), "SELECT id FROM users WHERE org = ?;\n")
		.expect("write");
	let rule = "[[embedded]]\ninclude = [\"*.java\"]\ngrammar = \"java\"\n";

	// Off by the rule: the `?` no longer lexes, so the string is left
	// alone and reported.
	std::fs::write(
		dir.join("squill.toml"),
		format!("{rule}question-params = false\n"),
	)
	.expect("write config");
	let output =
		squill().arg("fmt").arg(dir.join("A.java")).output().expect("run");
	assert!(String::from_utf8_lossy(&output.stderr).contains("did not parse"));
	assert_eq!(std::fs::read_to_string(dir.join("A.java")).expect("read"), java);

	// The grammar's default.
	std::fs::write(dir.join("squill.toml"), rule).expect("write config");
	let output = squill().arg("fmt").arg(&dir).output().expect("run");
	assert!(output.status.success());
	assert!(
		std::fs::read_to_string(dir.join("A.java"))
			.expect("read")
			.contains("        where org = ?\n"),
	);
	// Not for plain SQL, where it takes the key or the flag.
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

/// psycopg's `%s` is on for Python unless configured; elsewhere it is
/// opt-in, by key or flag.
#[test]
fn pyformat_params_default_by_grammar_and_configure() {
	let dir = temp_dir("pyformatparams");
	let python = "def f(cur):\n    cur.execute(\"\"\"SELECT id FROM users WHERE org = %(org)s\"\"\")\n";
	std::fs::write(dir.join("q.py"), python).expect("write");
	std::fs::write(dir.join("q.sql"), "SELECT id FROM users WHERE org = %s;\n")
		.expect("write");
	let rule = "[[embedded]]\ninclude = [\"*.py\"]\ngrammar = \"python\"\n";

	// Off by the rule: the placeholder no longer lexes as one.
	std::fs::write(
		dir.join("squill.toml"),
		format!("{rule}pyformat-params = false\n"),
	)
	.expect("write config");
	let output = squill().arg("fmt").arg(dir.join("q.py")).output().expect("run");
	assert!(
		String::from_utf8_lossy(&output.stderr).contains("did not parse"),
		"{}",
		String::from_utf8_lossy(&output.stderr)
	);
	assert_eq!(std::fs::read_to_string(dir.join("q.py")).expect("read"), python);

	// The grammar's default.
	std::fs::write(dir.join("squill.toml"), rule).expect("write config");
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
	assert_eq!(out, "select a from t where b = ?1\n");

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
/// stdin: an ignored path, or a host file no rule covers.
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
	// Anything else is SQL, as a named file would be.
	let (code, out, _) = stdin_as(&dir, "notes.txt", messy, &[]);
	assert_eq!((code, out.as_str()), (Some(0), "select 1;\n"));
	let _ = std::fs::remove_dir_all(&dir);
}
