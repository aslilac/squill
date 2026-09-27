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
	feature = "kotlin",
	feature = "swift",
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
use embed::SWIFT_SQL_QUERY;
use embed::format_embedded;
use formatter::Options;
use parser::lexer::LexOptions;
use std::path::Path;

/// The CLI's options for embedded SQL: defaults, and no final `;`.
fn options() -> Options {
	Options {
		trailing_semicolons: formatter::TrailingSemicolons::None,
		..Options::default()
	}
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
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
		Indent::FROM_HOST,
	)
	.expect("format")
	.text;
	assert_eq!(twice, formatted);
}

/// Format with a host's default query, returning text and warnings.
fn format_host(host: Host, query: &str, source: &str) -> embed::Embedded {
	format_host_with(host, query, source, &options())
}

/// Format with a host's default query and the given options.
fn format_host_with(
	host: Host,
	query: &str,
	source: &str,
	options: &Options,
) -> embed::Embedded {
	let formatted =
		format_embedded(source, &host.into(), query, options, Indent::FROM_HOST)
			.expect("format");
	let twice = format_embedded(
		&formatted.text,
		&host.into(),
		query,
		options,
		Indent::FROM_HOST,
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
			"b.Sql(\"\"\"\n        update BaseItems\n        set OwnerId = null\n        where OwnerId not in (select Id from BaseItems)\n        \"\"\");"
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
	// sqlite3_* calls are SQLite. The query promises raw strings take
	// line breaks, so a one-line one gets the multi-line layout too.
	assert_eq!(
		formatted.text,
		"void f() {\n  sqlite3_prepare_v2(db, R\"sql(\n  select a\n  from t\n  limit 1\n  )sql\", -1, &s, 0);\n  txn.exec(R\"(\n  select 1\n  )\");\n}\n"
	);
}

#[test]
fn java_text_blocks() {
	let source = "class A {\n    void m() {\n        conn.prepareStatement(\"\"\"\n            SELECT id FROM users WHERE org = ?\n            \"\"\");\n        conn.prepareStatement(\"\"\"\n            SELECT id \\\n            FROM users\n            \"\"\");\n    }\n}\n";
	// JDBC's `?` placeholders, as a Java project would configure.
	let options = Options { question_params: true, ..options() };
	let formatted =
		format_host_with(Host::Java, JAVA_SQL_QUERY, source, &options);
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
			Indent::FROM_HOST,
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
			"    as $$\n    begin\n        raise notice 'one\ntwo';\n    end\n    $$\n"
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
		formatted.text.contains("\nselect\n    key,\n    userId,"),
		"{}",
		formatted.text
	);
}

#[test]
fn embedded_sql_indents_by_the_files_step() {
	let two = "fn f() {\n  if x {\n    sqlx::query!(r#\"select a, b from t where alpha = 1 and beta = 2 or alpha = 3 and beta = 4 or alpha = 5 and gamma = 6\"#);\n  }\n}\n";
	let four = two.replace("\n  ", "\n    ").replace("\n      ", "\n        ");
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, two);
	assert!(
		formatted.text.contains("\n    where\n      alpha = 1"),
		"{}",
		formatted.text
	);
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, &four);
	assert!(
		formatted.text.contains("\n        where\n            alpha = 1"),
		"{}",
		formatted.text
	);
	// A configured width wins over the file's.
	let configured = format_embedded(
		&four,
		&Host::Rust.into(),
		RUST_SQLX_QUERY,
		&options(),
		Indent { configured_width: true, ..Indent::FROM_HOST },
	)
	.expect("format");
	assert!(
		configured.text.contains("\n        where\n          alpha = 1"),
		"{}",
		configured.text
	);
}

#[test]
fn max_width_counts_from_the_left_margin() {
	// A 76-column select list: it fits in 80 at column 0, not at 12.
	let columns =
		"select id, name, email, created_at, updated_at, organization_id, deleted";
	let sql = format!("{columns} from members");
	let at = |indent: &str| {
		format!("fn f() {{\n{indent}sqlx::query!(r#\"{sql}\"#);\n}}\n")
	};
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, &at(""));
	assert!(
		formatted.text.contains(&format!("\n{columns}\n")),
		"{}",
		formatted.text
	);
	let formatted = format_host(Host::Rust, RUST_SQLX_QUERY, &at("            "));
	assert!(!formatted.text.contains(columns), "{}", formatted.text);
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

/// Swift: `"""` strings — GRDB's `sql:` arguments as SQLite, SQLite.swift
/// and PostgresNIO calls in the configured dialect. Interpolated and
/// escaped strings, and single-line ones, are never touched.
#[test]
fn swift_multi_line_strings() {
	let source = r#"func f() throws {
    try db.execute(sql: """
        SELECT id FROM t WHERE a = ? AND b = :name
        """, arguments: [1])
    let n = try Int.fetchOne(db, sql: "SELECT   1")
    try conn.run("""
        select   \(x)
        """)
    let rows = try await client.query("""
        SELECT id FROM users WHERE org = $1 ORDER BY id
        """, logger: logger)
}
"#;
	let formatted = format_host(Host::Swift, SWIFT_SQL_QUERY, source);
	assert_eq!(
		formatted.text,
		r#"func f() throws {
    try db.execute(sql: """
    select id
    from t
    where a = ? and b = :name
    """, arguments: [1])
    let n = try Int.fetchOne(db, sql: "SELECT   1")
    try conn.run("""
        select   \(x)
        """)
    let rows = try await client.query("""
    select id
    from users
    where org = $1
    order by id
    """, logger: logger)
}
"#
	);
	assert!(formatted.warnings.is_empty(), "{:?}", formatted.warnings);
}

/// A capture names its dialect (`@sql.sqlite`) or takes the configured
/// one (`@sql`); an unknown one is refused rather than matching nothing,
/// and a pinned string's parse warning says whose dialect it was.
#[test]
fn dialect_captures() {
	let unknown = format_embedded(
		"",
		&Host::Cpp.into(),
		"((raw_string_content) @sql.mysql)",
		&options(),
		Indent::FROM_HOST,
	);
	let Err(err) = unknown else { panic!("@sql.mysql accepted") };
	assert!(err.to_string().contains("unknown capture `@sql.mysql`"), "{err}");

	let source = "void f() {\n  sqlite3_exec(db, R\"(\n    frobnicate 1\n  )\", 0, 0, 0);\n  txn.exec(R\"(\n    frobnicate 2\n  )\");\n}\n";
	let formatted = format_host(Host::Cpp, CPP_SQL_QUERY, source);
	let messages: Vec<&str> =
		formatted.warnings.iter().map(|w| w.message.as_str()).collect();
	assert_eq!(messages.len(), 2, "{messages:?}");
	assert!(
		messages[0].starts_with(
			"embedded SQL did not parse as SQLite, the dialect its query sets ("
		),
		"{messages:?}"
	);
	assert!(
		messages[1].starts_with("embedded SQL did not parse as Postgres ("),
		"{messages:?}"
	);
}

#[test]
fn one_line_raw_strings_become_multi_line() {
	// C#'s `"""` must open and close on lines of their own once the
	// content spans lines, which is squill's layout.
	let source = "class M {\n    void Up(DbCommand cmd) {\n        cmd.CommandText = \"\"\"select   1 from t\"\"\";\n    }\n}\n";
	let formatted = format_host(Host::CSharp, CSHARP_SQL_QUERY, source);
	assert!(
		formatted.text.contains(
			"cmd.CommandText = \"\"\"\n        select 1\n        from t\n        \"\"\";"
		),
		"{}",
		formatted.text
	);
	assert!(formatted.warnings.is_empty(), "{:?}", formatted.warnings);

	let source = "fun m() {\n    db.query(\"\"\"select   1 from t\"\"\")\n}\n";
	let formatted = format_host(Host::Kotlin, KOTLIN_SQL_QUERY, source);
	assert!(
		formatted
			.text
			.contains("db.query(\"\"\"\n    select 1\n    from t\n    \"\"\")"),
		"{}",
		formatted.text
	);
}

#[test]
fn unpromised_strings_are_left_alone() {
	// Without the query's `#set!` promises, squill assumes the worst: a
	// one-line string may not take line breaks, and a backslash may be
	// an escape.
	let query =
		CPP_SQL_QUERY.replace(" (#set! squill.raw) (#set! squill.multiline)", "");
	assert!(!query.contains("#set!"));
	let source = "void f() {\n  txn.exec(R\"(select   1)\");\n  txn.exec(R\"(\n    SELECT '\\d'\n  )\");\n}\n";
	let formatted = format_host(Host::Cpp, &query, source);
	assert_eq!(formatted.text, source);
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
	assert!(formatted.warnings[0].message.contains("squill.raw"));
	// Promised raw, the backslash is just a character.
	let formatted = format_host(Host::Cpp, CPP_SQL_QUERY, source);
	assert!(formatted.text.contains("  select '\\d'\n"), "{}", formatted.text);
	assert!(formatted.warnings.is_empty(), "{:?}", formatted.warnings);
}

#[test]
fn unknown_properties_are_refused() {
	let query = CPP_SQL_QUERY.replace("squill.multiline", "squill.multi-line");
	let result = format_embedded(
		"",
		&Host::Cpp.into(),
		&query,
		&options(),
		Indent::FROM_HOST,
	);
	let Err(error) = result else {
		panic!("a misspelled property should be refused");
	};
	assert!(error.to_string().contains("squill.multi-line"), "{error}");
}

#[test]
fn a_closing_delimiter_at_the_margin_stays_there() {
	// Some syntaxes need it there (a bare Ruby heredoc's terminator), so
	// a string closed at the margin is closed at the margin again.
	let source = "void f() {\n  txn.exec(R\"(\n    SELECT a FROM t\n)\");\n}\n";
	let formatted = format_host(Host::Cpp, CPP_SQL_QUERY, source);
	assert_eq!(
		formatted.text,
		"void f() {\n  txn.exec(R\"(\n  select a\n  from t\n)\");\n}\n"
	);
}

#[test]
fn a_broken_multiline_promise_fails_the_reparse() {
	// A query that promises a one-line `"..."` takes line breaks is
	// wrong, and C++'s grammar rejects the result, so the re-parse
	// catches it; the raw string beside it still formats. (Not every
	// grammar is that strict: Java's, Kotlin's, and Swift's all accept a
	// line break in a one-line string.)
	let query = r#"
((call_expression
   function: (identifier) @_f
   arguments: (argument_list (string_literal (string_content) @sql)))
 (#eq? @_f "run")
 (#set! squill.multiline))

((call_expression
   function: (field_expression field: (field_identifier) @_f)
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql)))
 (#eq? @_f "exec")
 (#set! squill.raw)
 (#set! squill.multiline))
"#;
	let source =
		"void f() {\n  run(\"select   1\");\n  txn.exec(R\"(select   2)\");\n}\n";
	let formatted = format_host(Host::Cpp, query, source);
	assert!(formatted.text.contains("run(\"select   1\")"), "{}", formatted.text);
	assert!(
		formatted.text.contains("txn.exec(R\"(\n  select 2\n  )\")"),
		"{}",
		formatted.text
	);
	assert_eq!(formatted.warnings.len(), 1, "{:?}", formatted.warnings);
	assert!(formatted.warnings[0].message.contains("parses"));
}

#[test]
fn gleam_strings_piped_into_a_query() {
	let source = "pub fn f(db) {\n  \"SELECT   1\"\n  |> pog.query\n  |> pog.execute(db)\n  \"select id from t where a = ?\" |> sqlight.query(on: db, with: [], expecting: d)\n  \"SELECT   2\" |> query\n}\n";
	let formatted = format_host(Host::Gleam, GLEAM_SQL_QUERY, source);
	// `"…" |> pog.query`, chained on.
	assert!(
		formatted.text.contains("  \"\n  select 1\n  \"\n  |> pog.query\n"),
		"{}",
		formatted.text
	);
	// sqlight's dialect comes along through a pipe: `?` lexes as SQLite.
	assert!(
		formatted.text.contains(
			"\"\n  select id\n  from t\n  where a = ?\n  \" |> sqlight.query("
		),
		"{}",
		formatted.text
	);
	// A bare `query`.
	assert!(
		formatted.text.contains("\"\n  select 2\n  \" |> query"),
		"{}",
		formatted.text
	);
	assert!(formatted.warnings.is_empty(), "{:?}", formatted.warnings);
}
