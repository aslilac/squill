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
         \tselect count(*) into n from t where id = NEW.id;\n\
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

/// The oracle lets whitespace move only inside procedural bodies — the
/// dollar strings the formatter reformats. Anywhere else a dollar string
/// is a value, and its whitespace is data.
#[test]
fn oracle_whitespace_leeway_is_only_for_bodies() {
	let equivalent = |a: &str, b: &str| {
		formatter::check::tokens_equivalent(
			a,
			b,
			Dialect::Postgres,
			Options::default().lex_options(),
		)
	};
	// A body: re-indenting it is sanctioned.
	assert!(equivalent(
		"create function f() returns int language sql as $$select 1$$;",
		"create function f() returns int language sql as $$\n  select 1\n$$;",
	));
	assert!(equivalent(
		"do $$begin null; end$$;",
		"do $$\nbegin\n  null;\nend\n$$;"
	));
	// A value: the same change is a different string.
	assert!(!equivalent(
		"insert into notes values ($$line one\nline two$$);",
		"insert into notes values ($$line one\n    line two$$);",
	));
	// One statement's body marker doesn't reach the next statement.
	assert!(!equivalent(
		"do $$begin null; end$$; select $$a b$$;",
		"do $$begin null; end$$; select $$a  b$$;",
	));
	// A value inside a body is still a value.
	assert!(!equivalent(
		"do $$begin raise notice $x$a b$x$; end$$;",
		"do $$begin raise notice $x$a  b$x$; end$$;",
	));
}

/// Labels sit on their own line, written tight, for blocks and loops.
#[test]
fn labels_are_tight_and_on_their_own_line() {
	assert_eq!(
		format(
			"do $$ << blk >> begin << outer >> loop exit outer; end loop outer; end blk $$;"
		),
		"do $$\n<<blk>>\nbegin\n\t<<outer>>\n\tloop\n\t\texit outer;\n\tend loop outer;\nend blk\n$$;\n"
	);
}

/// A procedural body that doesn't parse is left as written, and says so,
/// pointing into the source.
#[test]
fn unparsable_bodies_are_reported() {
	let source = "create function f() returns int language plpgsql as $$\nbegin\n  frobnicate 1;\nend\n$$;\ncreate function g() returns int language sql as $$ frobnicate 1 $$;\ncreate function h() returns int language plpgsql as $$ begin return 1; end $$;\n";
	let options = Options::default();
	let tokens = lex_with(source, Dialect::Postgres, options.lex_options());
	let parse = parser::parser::parse(&tokens, Dialect::Postgres);
	let formatted = format_cst(&parse.cst, &options);
	let found: Vec<(&str, &str)> = formatted
		.body_diagnostics
		.iter()
		.map(|d| (&source[d.start..d.end], d.message.as_str()))
		.collect();
	assert_eq!(found.len(), 2, "{found:?}");
	assert_eq!(found[0].0, "frobnicate");
	assert!(found[0].1.contains("PL/pgSQL body left as written"), "{found:?}");
	assert_eq!(found[1].0, "frobnicate");
	assert!(
		found[1].1.contains("SQL function body left as written"),
		"{found:?}"
	);
}

/// A query inside PL/pgSQL (a loop's, a cursor's, or a statement on its
/// own) stays on one line when it fits, like one at the top level.
#[test]
fn plpgsql_queries_stay_on_one_line_when_they_fit() {
	assert_eq!(
		format(
			"do $$ declare c cursor for select * from t; begin for r in select * from t where id = 1 loop null; end loop; end $$;"
		),
		"do $$\ndeclare\n\tc cursor for select * from t;\nbegin\n\tfor r in select * from t where id = 1 loop\n\t\tnull;\n\tend loop;\nend\n$$;\n"
	);
}

/// Trailing comments inside PL/pgSQL stay on their statement's line,
/// without leaving a blank line behind.
#[test]
fn plpgsql_trailing_comments() {
	assert_eq!(
		format(
			"do $$ declare n int := 0; -- count\nbegin\n if n > 0 then -- positive\n  perform 1; -- one\n else\n  perform 2; -- two\n end if; -- done\nend $$;"
		),
		"do $$\ndeclare\n\tn int := 0; -- count\nbegin\n\tif n > 0 then -- positive\n\t\tperform 1; -- one\n\telse\n\t\tperform 2; -- two\n\tend if; -- done\nend\n$$;\n"
	);
}

/// Blank lines between PL/pgSQL statements are the author's, kept (one
/// at most); none after a block opens or before it closes.
#[test]
fn plpgsql_blank_lines_between_statements() {
	assert_eq!(
		format(
			"do $$ declare a int := 1;\n\nb int := 2;\nbegin\n\nperform 1;\n\n\nperform 2;\nif a > 0 then perform 3;\n\nperform 4; end if;\n\nend $$;"
		),
		"do $$\ndeclare\n\ta int := 1;\n\n\tb int := 2;\nbegin\n\tperform 1;\n\n\tperform 2;\n\tif a > 0 then\n\t\tperform 3;\n\n\t\tperform 4;\n\tend if;\nend\n$$;\n"
	);
}

#[test]
fn assignment_targets_keep_their_case() {
	// `NEW` is a keyword elsewhere, but an assignment's target is a name,
	// just as `NEW.b` on the right is.
	assert_eq!(
		format("DO $$ BEGIN NEW.a := NEW.b; END $$;"),
		"do $$\nbegin\n\tNEW.a := NEW.b;\nend\n$$;\n"
	);
}

/// A string spanning lines inside a body is data: its lines take no
/// indent when the body is anchored.
#[test]
fn strings_spanning_lines_in_bodies_keep_their_lines() {
	assert_eq!(
		format(
			"create function f() returns text language sql as $$\nselect 'a\nb';\n$$;"
		),
		"create function f()\nreturns text\nlanguage sql\nas $$\n\tselect\n\t\t'a\nb';\n$$;\n"
	);
	assert_eq!(
		format(
			"create function f() returns text language plpgsql as $$\nbegin\nreturn 'a\nb';\nend\n$$;"
		),
		"create function f()\nreturns text\nlanguage plpgsql\nas $$\nbegin\n\treturn 'a\nb';\nend\n$$;\n"
	);
}
