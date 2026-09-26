//! TREE-100 acceptance: embedded-SQL formatting through tree-sitter
//! extraction queries. Only multiline string *syntaxes* (Rust raw
//! strings, Go backticks, Python triple quotes, JS/TS templates, Gleam
//! strings, and the raw strings / text blocks of C++, C#, Java, and
//! Kotlin) are reformatted, quotes on their own lines. Plain Rust and Go
//! strings join them once they already hold a line break, rewritten as
//! raw strings; single-line ones stay byte-identical, as do f-strings
//! and `${}` templates (SQL with holes).

// Exercises every built-in grammar: only with all of them compiled in.
#![cfg(all(
	feature = "rust",
	feature = "go",
	feature = "python",
	feature = "javascript",
	feature = "typescript",
	feature = "gleam",
	feature = "cpp",
	feature = "csharp",
	feature = "java",
	feature = "kotlin"
))]

use embed::CPP_SQL_QUERY;
use embed::CSHARP_SQL_QUERY;
use embed::GLEAM_SQL_QUERY;
use embed::GO_DB_QUERY;
use embed::Host;
use embed::Indent;
use embed::JAVA_SQL_QUERY;
use embed::JS_SQL_QUERY;
use embed::KOTLIN_SQL_QUERY;
use embed::PYTHON_DB_QUERY;
use embed::RUST_SQLX_QUERY;
use embed::format_embedded;
use formatter::Options;
use parser::lexer::LexOptions;
use std::path::Path;

fn options() -> Options {
	Options::default()
}

fn fixture_dir() -> std::path::PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sqlx_app")
}

#[test]
fn rust_sqlx_fixture_formats_and_still_compiles() {
	let fixture = fixture_dir();
	let source =
		std::fs::read_to_string(fixture.join("src/lib.rs")).expect("read fixture");
	let formatted = format_embedded(
		&source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;

	// The multi-line query formats, quotes on their own lines.
	assert!(
		formatted.contains("r#\"\n        select u.id, count(*) as n\n"),
		"multi-line query not formatted: {formatted}"
	);
	assert!(
		formatted.contains("group by u.id\n        \"#"),
		"closing quote must sit on its own line: {formatted}"
	);
	// Single-line literals stay byte-identical, formatted or not.
	assert!(formatted.contains(
		"\"SELECT id, name FROM users WHERE org_id = $1 AND deleted = false\""
	));
	assert!(
		formatted
			.contains("\"SELECT   count(*) FROM api_keys WHERE user_id = $1\"")
	);
	// Non-SQL and decoys stay byte-identical.
	assert!(formatted.contains("{not sql at all}"));
	assert!(formatted.contains("SELECT   * FROM decoy"));

	// Embedded SQL is token-equivalent before and after (the oracle
	// stands in for `cargo sqlx prepare` hash stability, per ticket).
	let before = extract_rust_strings(&source);
	let after = extract_rust_strings(&formatted);
	assert_eq!(before.len(), after.len());
	assert!(!before.is_empty(), "no raw strings found in fixture");
	for (before, after) in before.into_iter().zip(after) {
		assert!(
			formatter::check::tokens_equivalent(
				&before,
				&after,
				parser::Dialect::Postgres,
				LexOptions::default(),
			),
			"token stream changed:\n before: {before}\n after: {after}"
		);
	}

	// Idempotence at the host-file level.
	let twice = format_embedded(
		&formatted,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted, "host-file formatting must be idempotent");

	// The formatted fixture still compiles: copy the crate, substitute
	// the formatted lib.rs, and cargo check it offline.
	let dir = std::env::temp_dir()
		.join(format!("squill-embed-fixture-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	std::fs::create_dir_all(dir.join("src")).expect("mkdir");
	std::fs::copy(fixture.join("Cargo.toml"), dir.join("Cargo.toml"))
		.expect("copy manifest");
	std::fs::write(dir.join("src/lib.rs"), &formatted).expect("write lib");
	let status = std::process::Command::new(env!("CARGO"))
		.args(["check", "--offline", "--quiet"])
		.current_dir(&dir)
		.env("CARGO_TARGET_DIR", dir.join("target"))
		.status()
		.expect("run cargo check");
	assert!(status.success(), "formatted fixture does not compile");
	let _ = std::fs::remove_dir_all(&dir);
}

/// Pull raw-string contents back out (crudely) for the equivalence
/// check.
fn extract_rust_strings(source: &str) -> Vec<String> {
	let mut out = Vec::new();
	let mut rest = source;
	while let Some(start) = rest.find("r#\"") {
		let body = &rest[start + 3..];
		let end = body.find("\"#").expect("raw string end");
		out.push(body[..end].to_string());
		rest = &body[end..];
	}
	out
}

#[test]
fn single_line_plain_strings_stay_untouched() {
	// Plain quoted strings never reformat.
	let source = "fn main() {\n    let q = sqlx::query!(\"SELECT   id FROM t WHERE x = $1\");\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(formatted, source);
}

#[test]
fn single_line_raw_strings_reformat_vertically() {
	// Raw strings are multiline-capable, so they always take the
	// vertical shape — even when currently single-line.
	let source = "fn main() {\n    let q = sqlx::query!(\n        r#\"delete from team_auto_add_rules where team_id = $1 and id = $2\"#\n    );\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(
		formatted,
		"fn main() {\n    let q = sqlx::query!(\n        r#\"\n        delete from team_auto_add_rules\n        where team_id = $1 and id = $2\n        \"#\n    );\n}\n"
	);
	// Idempotent.
	let twice = format_embedded(
		&formatted,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

#[test]
fn multiline_literal_gets_quotes_on_own_lines() {
	let source = "fn main() {\n    let q = sqlx::query!(\n        r#\"SELECT id,name FROM users\n        WHERE org = $1 ORDER BY name\"#\n    );\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	// Multi-line literals stay clause-per-line even when the SQL would
	// fit on one line.
	assert_eq!(
		formatted,
		"fn main() {\n    let q = sqlx::query!(\n        r#\"\n        select id, name\n        from users\n        where org = $1\n        order by name\n        \"#\n    );\n}\n"
	);
	assert!(!formatted.contains('\t'), "no tabs in a spaces-indented file");
	// Idempotent.
	let twice = format_embedded(
		&formatted,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

#[test]
fn multiline_plain_strings_become_raw_strings() {
	// A plain `"..."` that already spans lines is rewritten as a raw
	// string, so quotes and backslashes need no escaping.
	let source = "fn f() {\n    sqlx::query!(\"SELECT 'it''s' AS s, \\\"Weird\\\" FROM t\n    WHERE x = $1\");\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(
		formatted,
		"fn f() {\n    sqlx::query!(r#\"\n    select 'it''s' as s, \"Weird\"\n    from t\n    where x = $1\n    \"#);\n}\n"
	);
}

#[test]
fn escaped_newlines_count_as_multiline() {
	// `\n` escapes are a line break too; so is a continuation.
	let source = "fn f() {\n    sqlx::query!(\"select a\\nfrom t\");\n    sqlx::query!(\"select b \\\n        from t\");\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(
		formatted,
		"fn f() {\n    sqlx::query!(r#\"\n    select a\n    from t\n    \"#);\n    sqlx::query!(r#\"\n    select b\n    from t\n    \"#);\n}\n"
	);
}

#[test]
fn rust_query_functions_use_the_session_dialect() {
	// `db::query(...)` (atuin's wrapper) and `sqlx::query_as::<_, T>(...)`
	// are function calls, not macros; the default query finds both, and
	// bare `@sql` captures follow the configured dialect.
	let source = "fn f() {\n    db::query(\"select id from t\n        where x = ?1 limit 1\");\n    sqlx::query_as::<_, Row>(r#\"select 1\"#);\n}\n";
	let mut options = options();
	options.dialect = parser::Dialect::Sqlite;
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options,
		Indent::FromHost,
	)
	.expect("format");
	assert!(
		formatted.text.contains(
			"db::query(r#\"\n    select id\n    from t\n    where x = ?1\n"
		),
		"{}",
		formatted.text
	);
	assert!(
		formatted.text.contains("query_as::<_, Row>(r#\"\n    select 1\n    \"#)"),
		"{}",
		formatted.text
	);
}

#[test]
fn unparsable_sql_is_reported_not_rewritten() {
	let source = "fn f() {\n    sqlx::query!(r#\"select (\n    from\"#);\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format");
	assert_eq!(formatted.text, source);
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
	let warning = &formatted.warnings[0];
	assert_eq!(&source[warning.offset..warning.offset + 3], "r#\"");
	assert!(warning.message.contains("did not parse"), "{}", warning.message);
}

#[test]
fn edits_the_query_cannot_read_back_are_dropped() {
	// This query only knows plain strings. Formatting turns one into a
	// raw string the query no longer captures, so the re-parse check
	// refuses the edit and says so.
	let query = r#"((macro_invocation (token_tree (string_literal) @sql)))"#;
	let source = "fn f() {\n    sqlx::query!(\"select 1\n    from t\");\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Rust.into(),
		query,
		&options(),
		Indent::FromHost,
	)
	.expect("format");
	assert_eq!(formatted.text, source);
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
	assert!(
		formatted.warnings[0].message.contains("change how the rust file parses"),
		"{}",
		formatted.warnings[0].message
	);
}

#[test]
fn go_multiline_interpreted_strings_become_raw_strings() {
	let source = "package main\n\nfunc f(db *sql.DB) {\n\tdb.Exec(\"DELETE FROM t\\nWHERE id = $1\")\n\tdb.Exec(\"DELETE FROM `t`\\nWHERE id = $1\")\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Go.into(),
		GO_DB_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format");
	assert!(
		formatted
			.text
			.contains("db.Exec(`\n\tdelete from t\n\twhere id = $1\n\t`)"),
		"{}",
		formatted.text
	);
	// A backtick has no raw-string spelling: left alone, with a warning.
	assert!(formatted.text.contains("\"DELETE FROM `t`\\nWHERE id = $1\""));
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
}

#[test]
fn match_predicate_is_rejected() {
	let query = r#"((identifier) @sql (#match? @sql "q.*"))"#;
	let err = format_embedded(
		"fn main() {}",
		&Host::Rust.into(),
		query,
		&options(),
		Indent::FromHost,
	)
	.unwrap_err();
	assert!(err.to_string().contains("match"), "got: {err}");
}

#[test]
fn go_smoke_test() {
	let source = "package main\n\nfunc list(db *sql.DB) {\n\trows, _ := db.Query(`SELECT id,name FROM users\n\tWHERE active ORDER BY name`)\n\t_, _ = db.Exec(\"DELETE   FROM sessions WHERE expires_at < now()\")\n\tfmt.Println(\"SELECT   not touched\")\n}\n"
        .to_string();
	let formatted = format_embedded(
		&source,
		&Host::Go.into(),
		GO_DB_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert!(
		formatted.contains(
			"`\n\tselect id, name\n\tfrom users\n\twhere active\n\torder by name\n\t`"
		),
		"multi-line raw string not formatted: {formatted}"
	);
	// Single-line strings stay byte-identical, even unformatted SQL.
	assert!(
		formatted.contains("\"DELETE   FROM sessions WHERE expires_at < now()\"")
	);
	// Non-query call untouched.
	assert!(formatted.contains("SELECT   not touched"));
	// Idempotent.
	let twice = format_embedded(
		&formatted,
		&Host::Go.into(),
		GO_DB_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

/// Options as the CLI resolves them for Python: pyformat params on.
fn python_options() -> Options {
	Options { pyformat_params: true, ..options() }
}

#[test]
fn python_smoke_test() {
	let source = "def load(cur, uid):\n    cur.execute(\"\"\"SELECT id,name FROM users WHERE org=%s AND status=%(status)s ORDER BY name\"\"\", args)\n    cur.execute(\"SELECT   1\")\n    cur.execute(f\"SELECT {tbl}\")\n    cur.execute(b\"SELECT 2\")\n";
	let formatted = format_embedded(
		source,
		&Host::Python.into(),
		PYTHON_DB_QUERY,
		&python_options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	// Triple-quoted strings take the vertical shape; pyformat params
	// survive byte-exact.
	assert!(
        formatted.contains(
            "\"\"\"\n    select id, name\n    from users\n    where org = %s and status = %(status)s\n    order by name\n    \"\"\""
        ),
        "triple-quoted not formatted: {formatted}"
    );
	// Single-quoted, f-, and b-strings stay byte-identical.
	assert!(formatted.contains("cur.execute(\"SELECT   1\")"));
	assert!(formatted.contains("f\"SELECT {tbl}\""));
	assert!(formatted.contains("b\"SELECT 2\""));
	// Idempotent.
	let twice = format_embedded(
		&formatted,
		&Host::Python.into(),
		PYTHON_DB_QUERY,
		&python_options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

#[test]
fn js_smoke_test() {
	let source = "async function f(db, id) {\n  await db.query(`SELECT id,name FROM users WHERE org = $1 ORDER BY name`, [id]);\n  const r = await sql`SELECT count(*) FROM api_keys WHERE user_id = ${id}`;\n  const t = sql`SELECT   3`;\n  db.query('SELECT   2');\n}\n";
	let formatted = format_embedded(
		source,
		&Host::JavaScript.into(),
		JS_SQL_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	// Template literals take the vertical shape.
	assert!(
		formatted.contains(
			"`\n  select id, name\n  from users\n  where org = $1\n  order by name\n  `"
		),
		"template not formatted: {formatted}"
	);
	// Tagged templates format too.
	assert!(formatted.contains("sql`\n  select 3\n  `"), "{formatted}");
	// `${}` substitutions and plain quoted strings stay byte-identical.
	assert!(
		formatted
			.contains("sql`SELECT count(*) FROM api_keys WHERE user_id = ${id}`")
	);
	assert!(formatted.contains("'SELECT   2'"));
	// Idempotent.
	let twice = format_embedded(
		&formatted,
		&Host::JavaScript.into(),
		JS_SQL_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

#[test]
fn typescript_smoke_test() {
	let source = "const f = async (db: Db): Promise<Row[]> =>\n  db.query(`SELECT id FROM t WHERE  x = $1`);\n";
	let formatted = format_embedded(
		source,
		&Host::TypeScript.into(),
		JS_SQL_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert!(
		formatted.contains("`\n  select id\n  from t\n  where x = $1\n  `"),
		"ts template not formatted: {formatted}"
	);
	let tsx = "export const List = () => {\n  const rows = db.query(`SELECT id,name FROM t`);\n  return <ul>{rows.map((r) => <li key={r.id}>{r.name}</li>)}</ul>;\n};\n";
	let formatted = format_embedded(
		tsx,
		&Host::Tsx.into(),
		JS_SQL_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert!(
		formatted.contains("`\n  select id, name\n  from t\n  `"),
		"tsx template not formatted: {formatted}"
	);
	assert!(formatted.contains("<li key={r.id}>"), "jsx mangled: {formatted}");
}

#[test]
fn gleam_smoke_test() {
	let source = "pub fn list(db) {\n  sqlight.query(\"select id,name from users where org = ? order by name\", on: db, with: [])\n  pog.query(\"SELECT   1\")\n}\n";
	let formatted = format_embedded(
		source,
		&Host::Gleam.into(),
		GLEAM_SQL_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	// sqlight is SQLite: `?` params lex; strings take the vertical shape.
	assert!(
		formatted.contains(
			"\"\n  select id, name\n  from users\n  where org = ?\n  order by name\n  \""
		),
		"sqlight string not formatted: {formatted}"
	);
	// pog goes through the session dialect (postgres by default).
	assert!(formatted.contains("pog.query(\"\n  select 1\n  \")"), "{formatted}");
	// Idempotent.
	let twice = format_embedded(
		&formatted,
		&Host::Gleam.into(),
		GLEAM_SQL_QUERY,
		&options(),
		Indent::FromHost,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

/// Format with a host's default query, returning text and warnings.
fn format_host(host: Host, query: &str, source: &str) -> embed::Embedded {
	// The CLI's defaults for the host's placeholders.
	let mut options = options();
	options.question_params = host.uses_question_params();
	options.pyformat_params = host.uses_pyformat_params();
	let formatted =
		format_embedded(source, &host.into(), query, &options, Indent::FromHost)
			.expect("format");
	let twice = format_embedded(
		&formatted.text,
		&host.into(),
		query,
		&options,
		Indent::FromHost,
	)
	.expect("format");
	assert_eq!(twice.text, formatted.text, "not idempotent");
	formatted
}

#[test]
fn csharp_raw_strings() {
	let source = r#"class M {
    void Up(MigrationBuilder b, DbCommand cmd) {
        b.Sql("""
            UPDATE BaseItems SET OwnerId = NULL
              WHERE OwnerId NOT IN (SELECT Id FROM BaseItems);
            """);
        cmd.CommandText = """
            select 1
        """;
        b.Sql($"""
            select {x}
            """);
        b.Sql(@"select   1
            from t");
    }
}
"#;
	let formatted = format_host(Host::CSharp, CSHARP_SQL_QUERY, source);
	assert!(
		formatted.text.contains(
			"b.Sql(\"\"\"\n        update BaseItems\n        set OwnerId = null\n        where OwnerId not in (select Id from BaseItems);\n        \"\"\");"
		),
		"{}",
		formatted.text
	);
	assert!(
		formatted
			.text
			.contains("cmd.CommandText = \"\"\"\n        select 1\n        \"\"\";"),
		"{}",
		formatted.text
	);
	// Interpolated and verbatim strings are not raw strings: untouched.
	assert!(formatted.text.contains("$\"\"\"\n            select {x}"));
	assert!(formatted.text.contains("@\"select   1"));
	assert!(formatted.warnings.is_empty(), "{:?}", formatted.warnings);
}

#[test]
fn cpp_raw_strings() {
	let source = "void f() {\n  sqlite3_prepare_v2(db, R\"sql(\n    SELECT a FROM t LIMIT 1\n  )sql\", -1, &s, 0);\n  txn.exec(R\"(select   1)\");\n}\n";
	let formatted = format_host(Host::Cpp, CPP_SQL_QUERY, source);
	// sqlite3_* calls are SQLite; single-line raw strings stay put.
	assert_eq!(
		formatted.text,
		"void f() {\n  sqlite3_prepare_v2(db, R\"sql(\n  select a\n  from t\n  limit 1\n  )sql\", -1, &s, 0);\n  txn.exec(R\"(select   1)\");\n}\n"
	);
}

#[test]
fn java_text_blocks() {
	let source = "class A {\n    void m() {\n        conn.prepareStatement(\"\"\"\n            SELECT id FROM users WHERE org = ?\n            \"\"\");\n        conn.prepareStatement(\"\"\"\n            SELECT id \\\n            FROM users\n            \"\"\");\n    }\n}\n";
	let formatted = format_host(Host::Java, JAVA_SQL_QUERY, source);
	assert!(
		formatted.text.contains(
			"prepareStatement(\"\"\"\n        select id\n        from users\n        where org = ?\n        \"\"\");"
		),
		"{}",
		formatted.text
	);
	// Text blocks process escapes: one holding a backslash is reported
	// and left alone.
	assert!(formatted.text.contains("SELECT id \\\n"));
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
	assert!(formatted.warnings[0].message.contains("backslash"));
}

#[test]
fn kotlin_raw_strings() {
	let source = "fun m() {\n    db.query(\"\"\"\n        SELECT id FROM users\n    \"\"\", mapper)\n    db.query(\"\"\"\n        SELECT id FROM $table\n    \"\"\")\n}\n";
	let formatted = format_host(Host::Kotlin, KOTLIN_SQL_QUERY, source);
	assert!(
		formatted.text.contains(
			"db.query(\"\"\"\n    select id\n    from users\n    \"\"\", mapper)"
		),
		"{}",
		formatted.text
	);
	// `$table` is a template: never matched.
	assert!(formatted.text.contains("SELECT id FROM $table"));
}

#[test]
fn every_default_query_compiles() {
	for &host in Host::ALL {
		let formatted = format_embedded(
			"",
			&host.into(),
			host.default_query(),
			&options(),
			Indent::FromHost,
		);
		assert!(formatted.is_ok(), "{}: {:?}", host.name(), formatted.err());
	}
}

#[test]
fn multiline_sql_strings_keep_their_value() {
	// Anchoring indents every SQL line to the host's indentation — except
	// lines inside a string, whose text is data.
	let source = "fn f() {\n    sqlx::query!(r#\"insert into t values ('line one\nline two')\"#);\n}\n";
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, source);
	assert!(
		formatted.text.contains("'line one\nline two'"),
		"{}",
		formatted.text
	);
}

#[test]
fn procedural_bodies_anchor_but_their_strings_do_not() {
	// A function body's layout is the formatter's own, so it follows the
	// host's indentation; a string spanning lines inside it is data.
	let source = "fn f() {\n    sqlx::query!(r#\"create function f() returns void language plpgsql as $$ begin raise notice 'one\ntwo'; end $$;\"#);\n    sqlx::query!(r#\"insert into notes values ($$one\ntwo$$)\"#);\n}\n";
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, source);
	assert!(
		formatted.text.contains(
			"    as $$\n    begin\n      raise notice 'one\ntwo';\n    end\n    $$;\n"
		),
		"{}",
		formatted.text
	);
	assert!(formatted.text.contains("$$one\ntwo$$"), "{}", formatted.text);
}

#[test]
fn column_zero_strings_take_the_files_indent_character() {
	// No anchor indentation to copy: the file's own indentation decides,
	// so a spaces-indented file never gains tabs.
	let source = "class A {\n    void M() {\n        c.Query(\n\"\"\"\n    SELECT key, userId, rating, played, playCount, isFavorite, playbackPositionTicks, lastPlayedDate FROM UserRatings\n\"\"\");\n    }\n}\n";
	let formatted = format_host(Host::CSharp, CSHARP_SQL_QUERY, source);
	assert!(!formatted.text.contains('\t'), "{}", formatted.text);
	assert!(
		formatted.text.contains("\nselect\n  key,\n  userId,"),
		"{}",
		formatted.text
	);
}

#[test]
fn unparsable_function_bodies_in_embedded_sql_are_reported() {
	let source = "fn f() {\n    sqlx::query!(r#\"create function f() returns int language plpgsql as $$ begin frobnicate; end $$\"#);\n}\n";
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, source);
	// The statement around the body still formats; the body is flagged.
	assert!(formatted.text.contains("returns int"), "{}", formatted.text);
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
	assert!(
		formatted.warnings[0].message.contains("PL/pgSQL body left as written"),
		"{:?}",
		formatted.warnings
	);
}

/// Calls the default queries once missed: a generic call after `await`
/// in TypeScript, generic Dapper methods in C#, templated pqxx methods
/// in C++, and `.trimIndent()`ed raw strings in Kotlin.
#[test]
fn generic_and_wrapped_calls_are_found() {
	let cases = [
		(
			Host::TypeScript,
			JS_SQL_QUERY,
			"async function f() {\n  const r = await pool.query<Row>(`SELECT  1`);\n}\n",
			"await pool.query<Row>(`\n  select 1\n  `);",
		),
		(
			Host::CSharp,
			CSHARP_SQL_QUERY,
			"class A {\n    void M() {\n        conn.QueryAsync<Order>(\"\"\"\n            SELECT  1\n            \"\"\");\n    }\n}\n",
			"conn.QueryAsync<Order>(\"\"\"\n        select 1\n        \"\"\");",
		),
		(
			Host::Cpp,
			CPP_SQL_QUERY,
			"void f() {\n  tx.query<int>(R\"(\n    SELECT  1\n  )\");\n}\n",
			"tx.query<int>(R\"(\n  select 1\n  )\");",
		),
		(
			Host::Kotlin,
			KOTLIN_SQL_QUERY,
			"fun m() {\n    exec(\"\"\"\n        SELECT  1\n    \"\"\".trimIndent())\n}\n",
			"exec(\"\"\"\n    select 1\n    \"\"\".trimIndent())",
		),
	];
	for (host, query, source, expected) in cases {
		let formatted = format_host(host, query, source);
		assert!(
			formatted.text.contains(expected),
			"{}: {}",
			host.name(),
			formatted.text
		);
	}
}
