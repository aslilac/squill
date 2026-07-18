//! TREE-100 acceptance: embedded-SQL formatting through tree-sitter
//! extraction queries.

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

    // SQL got formatted: lowercased keywords, normalized spacing.
    assert!(
        formatted.contains("select id, name, created_at"),
        "first query not formatted: {formatted}"
    );
    assert!(
        formatted.contains("select count(*)"),
        "scalar query not formatted: {formatted}"
    );
    // ...non-SQL and decoys stayed byte-identical.
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

/// Pull string literal contents back out (crudely) for the equivalence
/// check: everything between the sqlx macro parens.
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
fn multiline_output_reanchors_to_host_indent() {
    let source = "fn main() {\n    let q = sqlx::query!(\n        r#\"SELECT aaaaaaaaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbbbbbbbb, cccccccccccccccccccc FROM long_table WHERE x = $1 ORDER BY y\"#\n    );\n}\n";
    let formatted =
        format_embedded(source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    // Spaces-indented host: continuation lines use spaces, anchored to
    // the literal's line indentation.
    assert!(
        formatted.contains("\n        from long_table"),
        "continuation lines must anchor to host indent: {formatted}"
    );
    assert!(
        !formatted.contains('\t'),
        "no tabs may be injected into a spaces-indented file"
    );
    // Still idempotent.
    let twice =
        format_embedded(&formatted, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    assert_eq!(twice, formatted);
}

#[test]
fn plain_string_escapes_round_trip() {
    let source =
        r#"fn f() { sqlx::query!("SELECT 'it''s' AS s, \"Weird\" FROM t WHERE x = $1"); }"#;
    let formatted =
        format_embedded(source, Host::Rust, RUST_SQLX_QUERY, &options()).expect("format");
    // The doubled SQL quote and the escaped Rust quotes both survive.
    assert!(
        formatted.contains(r#"select 'it''s' as s, \"Weird\""#),
        "escapes mangled: {formatted}"
    );
}

#[test]
fn match_predicate_is_rejected() {
    let query = r#"((identifier) @sql (#match? @sql "q.*"))"#;
    let err = format_embedded("fn main() {}", Host::Rust, query, &options()).unwrap_err();
    assert!(err.to_string().contains("match"), "got: {err}");
}

#[test]
fn go_smoke_test() {
    let source = "package main\n\nfunc list(db *sql.DB) {\n\trows, _ := db.Query(`SELECT id,name FROM users WHERE active ORDER BY name`)\n\t_, _ = db.Exec(\"DELETE   FROM sessions WHERE expires_at < now()\")\n\tfmt.Println(\"SELECT   not touched\")\n}\n"
        .to_string();
    let formatted = format_embedded(&source, Host::Go, GO_DB_QUERY, &options()).expect("format");
    assert!(
        formatted.contains("`select id, name from users where active order by name`"),
        "raw string not formatted: {formatted}"
    );
    assert!(
        formatted.contains("\"delete from sessions where expires_at < now()\""),
        "interpreted string not formatted: {formatted}"
    );
    // Non-query call untouched.
    assert!(formatted.contains("SELECT   not touched"));
    // Idempotent.
    let twice = format_embedded(&formatted, Host::Go, GO_DB_QUERY, &options()).expect("format");
    assert_eq!(twice, formatted);
}
