//! TREE-101: recursive formatting of `LANGUAGE sql` procedural bodies;
//! plpgsql bodies stay byte-identical until their formatter lands.

use formatter::{Options, format_cst};
use parser::Dialect;
use parser::lexer::lex_with;

fn format(source: &str) -> String {
    let options = Options::default();
    let tokens = lex_with(source, Dialect::Postgres, options.lex_options());
    let parse = parser::parser::parse(&tokens, Dialect::Postgres);
    let formatted = format_cst(&parse.cst, &options);
    assert_eq!(formatted.fallback_statements, 0, "unexpected fallback");
    formatted.text
}

#[test]
fn language_sql_body_formats_and_anchors() {
    let out = format("CREATE FUNCTION one() RETURNS int LANGUAGE sql AS $$SELECT   1$$;");
    assert_eq!(
        out,
        "create function one() returns int language sql as $$\n\tselect 1\n$$;\n"
    );
}

#[test]
fn language_before_as_also_detected() {
    let out = format("CREATE FUNCTION two() RETURNS int AS $$SELECT   2$$ LANGUAGE sql;");
    assert_eq!(
        out,
        "create function two() returns int as $$\n\tselect 2\n$$ language sql;\n"
    );
}

#[test]
fn dollar_tag_is_preserved() {
    let out = format("CREATE FUNCTION t() RETURNS int LANGUAGE sql AS $fn$SELECT  3$fn$;");
    assert!(
        out.contains("$fn$\n\tselect 3\n$fn$"),
        "tag not preserved: {out}"
    );
}

#[test]
fn plpgsql_bodies_stay_byte_identical() {
    let body = "BEGIN\n  RETURN   1;\nEND;";
    let source = format!("CREATE FUNCTION f() RETURNS int LANGUAGE plpgsql AS $$ {body} $$;");
    let out = format(&source);
    assert!(
        out.contains(&format!("$$ {body} $$")),
        "plpgsql body was modified: {out}"
    );
}

#[test]
fn do_blocks_default_to_plpgsql_and_stay_untouched() {
    let source = "DO $$ BEGIN RAISE NOTICE   'hi'; END $$;";
    let out = format(source);
    assert!(
        out.contains("$$ BEGIN RAISE NOTICE   'hi'; END $$"),
        "DO body was modified: {out}"
    );
}

#[test]
fn unparseable_sql_body_stays_byte_identical() {
    let source = "CREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ {definitely not sql} $$;";
    let out = format(source);
    assert!(
        out.contains("$$ {definitely not sql} $$"),
        "unparseable body was modified: {out}"
    );
}

#[test]
fn multi_statement_sql_body() {
    let out = format(
        "CREATE FUNCTION f() RETURNS void LANGUAGE sql AS $$\n\
         INSERT INTO audit (kind) VALUES ('x');\n\
         DELETE   FROM audit WHERE created_at < now() - interval '90 days';\n\
         $$;",
    );
    assert_eq!(
        out,
        "create function f() returns void language sql as $$\n\
         \tinsert into audit (kind) values ('x');\n\
         \tdelete from audit where created_at < now() - interval '90 days';\n\
         $$;\n"
    );
}

#[test]
fn splice_is_idempotent() {
    let sources = [
        "CREATE FUNCTION one() RETURNS int LANGUAGE sql AS $$SELECT   1$$;",
        "CREATE FUNCTION f() RETURNS void LANGUAGE sql AS $$ INSERT INTO t VALUES (1); SELECT * FROM t WHERE a = 1 AND b = 2; $$;",
    ];
    for source in sources {
        let once = format(source);
        let twice = format(&once);
        assert_eq!(twice, once, "not idempotent for {source}");
    }
}

#[test]
fn recursive_equivalence_covers_bodies() {
    use parser::lexer::LexOptions;
    // Whitespace-only changes inside a body are equivalent...
    assert!(formatter::check::tokens_equivalent(
        "create function f() language sql as $$SELECT   1$$;",
        "create function f() language sql as $$\n\tselect 1\n$$;",
        Dialect::Postgres,
        LexOptions::default(),
    ));
    // ...but token changes inside a body are not.
    assert!(!formatter::check::tokens_equivalent(
        "create function f() language sql as $$select 1$$;",
        "create function f() language sql as $$select 2$$;",
        Dialect::Postgres,
        LexOptions::default(),
    ));
    // Comments inside bodies are counted.
    let comments = formatter::check::comment_texts(
        "create function f() language sql as $$ -- inner\nselect 1$$; -- outer",
        Dialect::Postgres,
        LexOptions::default(),
    );
    assert_eq!(comments, ["-- inner", "-- outer"]);
}
