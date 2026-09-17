//! Targeted formatting-rule tests: canonical layout and comment
//! placement.

use formatter::Options;
use formatter::format_cst;
use parser::Dialect;
use parser::lexer::lex_with;

fn format(source: &str) -> String {
	let options = Options { at_params: true, ..Options::default() };
	let tokens = lex_with(source, Dialect::Postgres, options.lex_options());
	let parse = parser::parser::parse(&tokens, Dialect::Postgres);
	let formatted = format_cst(&parse.cst, &options);
	assert_eq!(formatted.fallback_statements, 0, "unexpected fallback");
	formatted.text
}

#[test]
fn short_statement_collapses() {
	assert_eq!(format("SELECT   1\n;"), "select 1;\n");
}

#[test]
fn clause_per_line_when_long() {
	let out = format(
		"select aaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbb, cccccccccccccccc, dddddddddddddddd \
         from a_rather_long_table_name where a_long_condition_column = true;",
	);
	assert_eq!(
		out,
		"select aaaaaaaaaaaaaaaa, bbbbbbbbbbbbbbbb, cccccccccccccccc, dddddddddddddddd\n\
         from a_rather_long_table_name\n\
         where a_long_condition_column = true;\n"
	);
}

#[test]
fn boolean_chain_breaks_operator_leading() {
	let out = format(
		"select 1 from t where first_long_condition and second_long_condition \
         and (third_condition or fourth_condition) and fifth_long_condition_here;",
	);
	assert_eq!(
		out,
		"select 1\n\
         from t\n\
         where\n\
         \tfirst_long_condition\n\
         \tand second_long_condition\n\
         \tand (third_condition or fourth_condition)\n\
         \tand fifth_long_condition_here;\n"
	);
}

#[test]
fn keyword_case_never_touches_identifiers_or_comments() {
	let out = format("SELECT Total AS Sum FROM T; -- keep CASE of This");
	assert_eq!(out, "select Total as Sum\nfrom T; -- keep CASE of This\n");
}

#[test]
fn sqlc_annotation_stays_leading() {
	// The annotation leads the statement, which still collapses when it
	// fits on one line.
	let out =
		format("-- name: GetThing :one\nSELECT * FROM things WHERE id = @id;");
	assert_eq!(
		out,
		"-- name: GetThing :one\nselect * from things where id = @id;\n"
	);
}

#[test]
fn leading_comments_reindent_consistently() {
	// Mixed tab/space comment indentation (the users.sql case) comes out
	// aligned to the formatter's indentation.
	let source = "SELECT CASE WHEN a THEN b ELSE\n\
                  \t\t-- If the login type is not password, then the password should be\n\
                  \x20       -- cleared.\n\
                  \t\t''::bytea END FROM t WHERE some_long_condition_column AND another_condition;";
	let out = format(source);
	assert_eq!(
		out,
		"select\n\
         \tcase\n\
         \t\twhen a then b\n\
         \t\telse\n\
         \t\t-- If the login type is not password, then the password should be\n\
         \t\t-- cleared.\n\
         \t\t''::bytea\n\
         \tend\n\
         from t\n\
         where some_long_condition_column and another_condition;\n"
	);
}

#[test]
fn comment_between_statements_attaches_to_next() {
	let out = format("select 1;\n-- belongs to two\nselect 2;");
	assert_eq!(out, "select 1;\n-- belongs to two\nselect 2;\n");
}

#[test]
fn trailing_comment_stays_trailing() {
	let out = format("select 1; -- done\nselect 2;");
	assert_eq!(out, "select 1; -- done\nselect 2;\n");
}

#[test]
fn blank_lines_between_statements_survive() {
	// None, one, and two blank lines are the author's; anything past two
	// collapses to two.
	for (input, want) in [
		("select 1;\nselect 2;", "select 1;\nselect 2;\n"),
		("select 1;\n\nselect 2;", "select 1;\n\nselect 2;\n"),
		("select 1;\n\n\nselect 2;", "select 1;\n\n\nselect 2;\n"),
		("select 1;\n\n\n\nselect 2;", "select 1;\n\n\nselect 2;\n"),
		("select 1;\n\n\n\n\n\nselect 2;", "select 1;\n\n\nselect 2;\n"),
	] {
		let out = format(input);
		assert_eq!(out, want, "for {input:?}");
		// And the shape is a fixed point.
		assert_eq!(format(&out), out, "not idempotent for {input:?}");
	}
}

#[test]
fn blank_lines_around_standalone_comments_survive() {
	// A comment separated from the next statement by a blank line is a
	// standalone remark, not a caption: both gaps are the author's.
	let out = format("select 1;\n\n\n-- a remark\n\nselect 2;");
	assert_eq!(out, "select 1;\n\n\n-- a remark\n\nselect 2;\n");
	assert_eq!(format(&out), out);
}

#[test]
fn comment_caption_versus_standalone_remark() {
	// No gap: the comment captions the statement, and stays welded to it.
	let out = format("-- caption\nselect 1;");
	assert_eq!(out, "-- caption\nselect 1;\n");
	// One gap: kept, so the comment still reads as standalone.
	let out = format("-- remark\n\nselect 1;");
	assert_eq!(out, "-- remark\n\nselect 1;\n");
	assert_eq!(format(&out), out);
	// More than one gap below a comment collapses to one.
	assert_eq!(format("-- remark\n\n\n\nselect 1;"), "-- remark\n\nselect 1;\n");
	// Gaps between stacked comments and below the block are independent.
	let out = format("-- a\n\n-- b\n\nselect 1;");
	assert_eq!(out, "-- a\n\n-- b\n\nselect 1;\n");
	assert_eq!(format(&out), out);
}

#[test]
fn error_statements_pass_through_verbatim() {
	let source = "INSERT   INTO ; select 1;";
	let out = format(source);
	assert_eq!(out, "INSERT   INTO ;\nselect 1;\n");
}

#[test]
fn dml_and_ddl_format() {
	let out = format(
		"UPDATE users SET name = 'x', updated_at = NOW() WHERE id = @id RETURNING *;\n\
         INSERT INTO t (a, b) VALUES (1, 2) ON CONFLICT (a) DO NOTHING;\n\
         CREATE TABLE t (id uuid NOT NULL, PRIMARY KEY (id));",
	);
	// Short DML collapses; CREATE TABLE column lists always break.
	assert_eq!(
		out,
		"update users set name = 'x', updated_at = now() where id = @id returning *;\n\
         insert into t (a, b) values (1, 2) on conflict (a) do nothing;\n\
         create table t (\n\tid uuid not null,\n\tprimary key (id)\n);\n"
	);
}

#[test]
fn crlf_input_normalizes_to_lf() {
	// Windows line endings are trivia; output is always LF (and a second
	// pass over the LF output is a no-op).
	let out = format("SELECT   1;\r\nSELECT 2\r\nFROM t;\r\n");
	assert_eq!(out, "select 1;\nselect 2 from t;\n");
	assert!(!out.contains('\r'));
	assert_eq!(format(&out), out);
}

#[test]
fn function_attributes_reorder_even_with_empty_parens() {
	// `f()` parses as a call expression (name and parens in one node);
	// the canonical attribute order must still apply.
	let out =
		format("create function f() returns trigger as 'x' language plpgsql;");
	assert_eq!(
		out,
		"create function f()\nreturns trigger\nlanguage plpgsql\nas 'x';\n"
	);
}

#[test]
fn keyword_case_spares_names_and_types() {
	// Only real keywords respond to keyword-case; table, column, and
	// type names keep the author's spelling in both directions — even
	// a column named after a keyword, like `name`.
	let options = Options {
		keyword_case: formatter::KeywordCase::Upper,
		..Options::default()
	};
	let tokens = lex_with(
		"create table Foo_Bar (id uuid not null, name text not null, primary key (id));",
		Dialect::Postgres,
		options.lex_options(),
	);
	let parse = parser::parser::parse(&tokens, Dialect::Postgres);
	let out = format_cst(&parse.cst, &options).text;
	assert_eq!(
		out,
		"CREATE TABLE Foo_Bar (\n\tid uuid NOT NULL,\n\tname TEXT NOT NULL,\n\tPRIMARY KEY (id)\n);\n"
	);
	// And lower mode leaves deliberately-cased names alone.
	assert_eq!(
		format("CREATE TABLE Foo (Name TEXT);"),
		"create table Foo (\n\tName text\n);\n"
	);
}

#[test]
fn array_types_and_modifiers_glue_to_the_type() {
	// `text[]`, not `text []` — and the same in every type position,
	// including the ones DDL parses as a plain token soup.
	assert_eq!(
		format("create table t (a text[], b int[3], c numeric(10,2)[]);"),
		"create table t (\n\ta text[],\n\tb int[3],\n\tc numeric(10, 2)[]\n);\n"
	);
	assert_eq!(
		format(
			"create table t (a character varying(64), b timestamp(6) with time zone);"
		),
		"create table t (\n\ta character varying(64),\n\tb timestamp(6) with time zone\n);\n"
	);
	assert_eq!(
		format("alter table t add column c varchar(4096) not null;"),
		"alter table t add column c varchar(4096) not null;\n"
	);
	assert_eq!(
		format("alter table t alter column c type character varying(64);"),
		"alter table t alter column c type character varying(64);\n"
	);
	assert_eq!(
		format("create domain d as varchar(64);"),
		"create domain d as varchar(64);\n"
	);
	// Extension types are typed the same way, and are recognized by not
	// being keywords.
	assert_eq!(
		format("create table t (embedding vector(1536));"),
		"create table t (\n\tembedding vector(1536)\n);\n"
	);
	// Casts already glued; they must stay that way.
	assert_eq!(format("select a::varchar(64)[];"), "select a::varchar(64)[];\n");
}

#[test]
fn parens_that_are_not_type_modifiers_keep_their_space() {
	// A numeric paren group is only a type modifier when the word before
	// it is a type — `values (1, 2)` and `in (1, 2)` are not.
	assert_eq!(
		format("insert into t values (1, 2);"),
		"insert into t values (1, 2);\n"
	);
	assert_eq!(
		format(
			"create table t (a int, primary key (a), foreign key (a) references o (id));"
		),
		"create table t (\n\ta int,\n\tprimary key (a),\n\tforeign key (a) references o (id)\n);\n"
	);
	assert_eq!(
		format("create table t (a int) with (fillfactor = 70);"),
		"create table t (\n\ta int\n) with (fillfactor = 70);\n"
	);
}

#[test]
fn array_constructor_glues_to_its_bracket() {
	assert_eq!(
		format("select array [1, 2], array []::text[];"),
		"select array[1, 2], array[]::text[];\n"
	);
	assert_eq!(
		format("select 1 where x = any (array ['a']);"),
		"select 1 where x = any(array['a']);\n"
	);
	// `array(subquery)` is the other spelling, and keeps its own shape.
	assert_eq!(format("select array (select 1);"), "select array (select 1);\n");
}
