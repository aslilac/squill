//! TREE-101/103: recursive formatting of procedural bodies — `LANGUAGE
//! sql` through the SQL grammar, `LANGUAGE plpgsql` and `DO` blocks
//! through the PL/pgSQL grammar.

use formatter::Options;
use formatter::format_cst;
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
	let out =
		format("CREATE FUNCTION one() RETURNS int LANGUAGE sql AS $$SELECT   1$$;");
	// Multi-line bodies break the function header clause-per-line.
	assert_eq!(
		out,
		"create function one()\nreturns int\nlanguage sql\nas $$\n\tselect 1\n$$;\n"
	);
}

#[test]
fn language_before_as_also_detected() {
	let out =
		format("CREATE FUNCTION two() RETURNS int AS $$SELECT   2$$ LANGUAGE sql;");
	// The body formats as SQL, and the clause reorders canonically.
	assert_eq!(
		out,
		"create function two()\nreturns int\nlanguage sql\nas $$\n\tselect 2\n$$;\n"
	);
}

#[test]
fn dollar_tag_is_preserved() {
	let out = format(
		"CREATE FUNCTION t() RETURNS int LANGUAGE sql AS $fn$SELECT  3$fn$;",
	);
	assert!(out.contains("$fn$\n\tselect 3\n$fn$"), "tag not preserved: {out}");
}

#[test]
fn plpgsql_bodies_format() {
	let source = "CREATE FUNCTION f() RETURNS int LANGUAGE plpgsql AS $$ BEGIN\n  RETURN   1;\nEND; $$;";
	let out = format(source);
	assert_eq!(
		out,
		"create function f()\nreturns int\nlanguage plpgsql\nas $$\n\
         begin\n\
         \treturn 1;\n\
         end;\n\
         $$;\n"
	);
}

#[test]
fn do_blocks_default_to_plpgsql_and_format() {
	let source = "DO $$ BEGIN RAISE NOTICE   'hi'; END $$;";
	let out = format(source);
	assert_eq!(out, "do $$\nbegin\n\traise notice 'hi';\nend\n$$;\n");
}

#[test]
fn plpgsql_control_flow_layout() {
	let source = "CREATE FUNCTION guard() RETURNS trigger LANGUAGE plpgsql AS $fn$\n\
        DECLARE n int := 0;\n\
        BEGIN\n\
        SELECT count(*) INTO n FROM t WHERE id = NEW.id;\n\
        IF n > 10 THEN\n\
        RAISE EXCEPTION 'too many';\n\
        ELSIF n > 5 THEN RAISE WARNING 'getting close';\n\
        ELSE RETURN NEW;\n\
        END IF;\n\
        FOR i IN 1..3 LOOP PERFORM audit(i); END LOOP;\n\
        RETURN NEW;\n\
        EXCEPTION WHEN OTHERS THEN RETURN NULL;\n\
        END;\n\
        $fn$;";
	let out = format(source);
	assert_eq!(
		out,
		"create function guard()\nreturns trigger\nlanguage plpgsql\nas $fn$\n\
         declare\n\
         \tn int := 0;\n\
         begin\n\
         \tselect count(*)\n\
         \tinto n\n\
         \tfrom t\n\
         \twhere id = NEW.id;\n\
         \tif n > 10 then\n\
         \t\traise exception 'too many';\n\
         \telsif n > 5 then\n\
         \t\traise warning 'getting close';\n\
         \telse\n\
         \t\treturn NEW;\n\
         \tend if;\n\
         \tfor i in 1..3 loop\n\
         \t\tperform audit(i);\n\
         \tend loop;\n\
         \treturn NEW;\n\
         exception\n\
         \twhen OTHERS then\n\
         \t\treturn null;\n\
         end;\n\
         $fn$;\n"
	);
}

#[test]
fn plpgsql_comments_survive_in_bodies() {
	let source = "DO $$\n\
        BEGIN\n\
        -- leading comment\n\
        PERFORM 1; -- trailing comment\n\
        END;\n\
        $$;";
	let out = format(source);
	assert!(out.contains("-- leading comment"), "dropped: {out}");
	assert!(out.contains("-- trailing comment"), "dropped: {out}");
	// Idempotent with comments in play.
	assert_eq!(format(&out), out);
}

#[test]
fn unparsable_sql_body_stays_byte_identical() {
	let source = "CREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ {definitely not sql} $$;";
	let out = format(source);
	assert!(
		out.contains("$$ {definitely not sql} $$"),
		"unparsable body was modified: {out}"
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
		"create function f()\nreturns void\nlanguage sql\nas $$\n\
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
