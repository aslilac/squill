//! TREE-100 acceptance: embedded-SQL formatting through tree-sitter
//! extraction queries. Only multiline string *syntaxes* (raw strings,
//! Go backticks) are reformatted, quotes on their own lines; plain
//! quoted strings stay byte-identical.

use std::path::Path;

use embed::{GO_DB_QUERY, Host, RUST_SQLX_QUERY, format_embedded};
use formatter::Options;
use parser::lexer::LexOptions;

fn options() -> Options {
    Options::default()
}

fn fixture_dir() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sqlx_app")
}

#[test]
fn rust_sqlx_fixture_formats_and_still_compiles() {
    let fixture = fixture_dir();
    let source = std::fs::read_to_string(fixture.join("src/lib.rs")).expect("read fixture");
    let formatted =
        format_embedded(&source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");

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
    assert!(
        formatted.contains("\"SELECT id, name FROM users WHERE org_id = $1 AND deleted = false\"")
    );
    assert!(formatted.contains("\"SELECT   count(*) FROM api_keys WHERE user_id = $1\""));
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
    let twice =
        format_embedded(&formatted, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(twice, formatted, "host-file formatting must be idempotent");

    // The formatted fixture still compiles: copy the crate, substitute
    // the formatted lib.rs, and cargo check it offline.
    let dir = std::env::temp_dir().join(format!("squill-embed-fixture-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("src")).expect("mkdir");
    std::fs::copy(fixture.join("Cargo.toml"), dir.join("Cargo.toml")).expect("copy manifest");
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
    let formatted =
        format_embedded(source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(formatted, source);
}

#[test]
fn single_line_raw_strings_reformat_vertically() {
    // Raw strings are multiline-capable, so they always take the
    // vertical shape — even when currently single-line.
    let source = "fn main() {\n    let q = sqlx::query!(\n        r#\"delete from team_auto_add_rules where team_id = $1 and id = $2\"#\n    );\n}\n";
    let formatted =
        format_embedded(source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(
        formatted,
        "fn main() {\n    let q = sqlx::query!(\n        r#\"\n        delete from team_auto_add_rules\n        where team_id = $1 and id = $2\n        \"#\n    );\n}\n"
    );
    // Idempotent.
    let twice =
        format_embedded(&formatted, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(twice, formatted);
}

#[test]
fn multiline_literal_gets_quotes_on_own_lines() {
    let source = "fn main() {\n    let q = sqlx::query!(\n        r#\"SELECT id,name FROM users\n        WHERE org = $1 ORDER BY name\"#\n    );\n}\n";
    let formatted =
        format_embedded(source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    // Multi-line literals stay clause-per-line even when the SQL would
    // fit on one line.
    assert_eq!(
        formatted,
        "fn main() {\n    let q = sqlx::query!(\n        r#\"\n        select id, name\n        from users\n        where org = $1\n        order by name\n        \"#\n    );\n}\n"
    );
    assert!(
        !formatted.contains('\t'),
        "no tabs in a spaces-indented file"
    );
    // Idempotent.
    let twice =
        format_embedded(&formatted, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(twice, formatted);
}

#[test]
fn plain_strings_never_reformat() {
    // Plain `"..."` syntax is single-line-with-escapes territory: left
    // byte-identical even when the content already spans lines.
    let source =
        "fn f() { sqlx::query!(\"SELECT 'it''s' AS s, \\\"Weird\\\" FROM t\nWHERE x = $1\"); }";
    let formatted =
        format_embedded(source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(formatted, source);
}

#[test]
fn match_predicate_is_rejected() {
    let query = r#"((identifier) @sql (#match? @sql "q.*"))"#;
    let err = format_embedded("fn main() {}", Host::Rust, query, &options()).unwrap_err();
    assert!(err.to_string().contains("match"), "got: {err}");
}

#[test]
fn go_smoke_test() {
    let source = "package main\n\nfunc list(db *sql.DB) {\n\trows, _ := db.Query(`SELECT id,name FROM users\n\tWHERE active ORDER BY name`)\n\t_, _ = db.Exec(\"DELETE   FROM sessions WHERE expires_at < now()\")\n\tfmt.Println(\"SELECT   not touched\")\n}\n"
        .to_string();
    let formatted = format_embedded(&source, Host::Go, GO_DB_QUERY, &options()).expect("format");
    assert!(
        formatted
            .contains("`\n\tselect id, name\n\tfrom users\n\twhere active\n\torder by name\n\t`"),
        "multi-line raw string not formatted: {formatted}"
    );
    // Single-line strings stay byte-identical, even unformatted SQL.
    assert!(formatted.contains("\"DELETE   FROM sessions WHERE expires_at < now()\""));
    // Non-query call untouched.
    assert!(formatted.contains("SELECT   not touched"));
    // Idempotent.
    let twice = format_embedded(&formatted, Host::Go, GO_DB_QUERY, &options()).expect("format");
    assert_eq!(twice, formatted);
}
