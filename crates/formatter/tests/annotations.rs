//! Migration tools and query compilers read directives out of SQL
//! comments: goose's `-- +goose StatementBegin`, dbmate's `-- migrate:up`,
//! sqlc's `-- name: GetUser :one`, and so on. Formatting must leave every
//! one on its own line, in its place relative to the statements.

use formatter::Options;
use formatter::format_cst;
use parser::Dialect;
use parser::lexer::lex_with;

/// Format as Postgres with sqlc's `@name` params, returning the text and
/// how many statements passed through verbatim.
fn format(source: &str) -> (String, usize) {
	let options = Options { at_params: true, ..Options::default() };
	let tokens = lex_with(source, Dialect::Postgres, options.lex_options());
	let parse = parser::parser::parse(&tokens, Dialect::Postgres);
	let formatted = format_cst(&parse.cst, &options);
	(formatted.text, formatted.fallback_statements)
}

/// Format `source` cleanly into `expected`, and `expected` into itself.
fn check(source: &str, expected: &str) {
	let (text, fallbacks) = format(source);
	assert_eq!(fallbacks, 0, "unexpected fallback");
	assert_eq!(text, expected);
	assert_eq!(format(expected).0, expected, "not idempotent");
}

/// goose: `Up`/`Down` sections, `StatementBegin`/`StatementEnd` around
/// a function body, and `NO TRANSACTION`.
#[test]
fn goose() {
	check(
		r#"-- +goose Up
-- +goose StatementBegin
CREATE TABLE users (id bigserial PRIMARY KEY, email text NOT NULL UNIQUE);
-- +goose StatementEnd

-- +goose StatementBegin
CREATE OR REPLACE FUNCTION touch_updated_at() RETURNS trigger LANGUAGE plpgsql AS $$
BEGIN
  NEW.updated_at = now();
  RETURN NEW;
END;
$$;
-- +goose StatementEnd

-- +goose NO TRANSACTION
CREATE INDEX CONCURRENTLY users_email ON users (email);

-- +goose Down
-- +goose StatementBegin
DROP FUNCTION touch_updated_at();
-- +goose StatementEnd
DROP TABLE users;
"#,
		r#"-- +goose Up
-- +goose StatementBegin
create table users (
	id bigserial primary key,
	email text not null unique
);
-- +goose StatementEnd

-- +goose StatementBegin
create or replace function touch_updated_at()
returns trigger
language plpgsql
as $$
begin
	NEW.updated_at = now();
	return NEW;
end;
$$;
-- +goose StatementEnd

-- +goose NO TRANSACTION
create index concurrently users_email on users (email);

-- +goose Down
-- +goose StatementBegin
drop function touch_updated_at();
-- +goose StatementEnd
drop table users;
"#,
	);
}

/// dbmate: `migrate:up` and `migrate:down` sections.
#[test]
fn dbmate() {
	check(
		r#"-- migrate:up
CREATE TABLE posts (id serial PRIMARY KEY, title text NOT NULL);
INSERT INTO posts (title) VALUES ('hello');

-- migrate:down
DROP TABLE posts;
"#,
		r#"-- migrate:up
create table posts (
	id serial primary key,
	title text not null
);
insert into posts (title) values ('hello');

-- migrate:down
drop table posts;
"#,
	);
}

/// dbmate: a section's options, and an empty `down`.
#[test]
fn dbmate_notx() {
	check(
		r#"-- migrate:up transaction:false
ALTER TYPE mood ADD VALUE 'meh';

-- migrate:down
"#,
		r#"-- migrate:up transaction:false
alter type mood add value 'meh';

-- migrate:down
"#,
	);
}

/// sqlc: a `-- name:` line (or `/* name: */`) above each query, with
/// its doc comments under it.
#[test]
fn sqlc() {
	check(
		r#"-- name: GetUser :one
SELECT id, email FROM users WHERE id = $1 LIMIT 1;

-- name: ListUsers :many
-- Every user, newest first.
SELECT id, email FROM users ORDER BY id DESC;

-- name: CreateUser :one
INSERT INTO users (email) VALUES (sqlc.arg(email)) RETURNING *;

-- name: DeleteUsers :exec
DELETE FROM users WHERE id = ANY(@ids::bigint[]);

/* name: CountUsers :one */
SELECT count(*) FROM users;
"#,
		r#"-- name: GetUser :one
select id, email from users where id = $1 limit 1;

-- name: ListUsers :many
-- Every user, newest first.
select id, email from users order by id desc;

-- name: CreateUser :one
insert into users (email) values (sqlc.arg(email)) returning *;

-- name: DeleteUsers :exec
delete from users where id = any(@ids::bigint[]);

/* name: CountUsers :one */
select count(*) from users;
"#,
	);
}

/// Flyway: a script configuration comment on the first line.
#[test]
fn flyway() {
	check(
		r#"-- flyway:executeInTransaction=false
CREATE INDEX CONCURRENTLY posts_title ON posts (title);
"#,
		r#"-- flyway:executeInTransaction=false
create index concurrently posts_title on posts (title);
"#,
	);
}

/// Sqitch: the header comments `sqitch add` writes.
#[test]
fn sqitch() {
	check(
		r#"-- Deploy app:users to pg
-- requires: appschema

BEGIN;
CREATE TABLE app.users (id int);
COMMIT;
"#,
		r#"-- Deploy app:users to pg
-- requires: appschema

begin;
create table app.users (
	id int
);
commit;
"#,
	);
}

/// A `;` squill adds goes on the statement, above the marker after it.
#[test]
fn an_added_semicolon_stays_inside_the_markers() {
	check(
		"-- +goose StatementBegin\nSELECT 1\n-- +goose StatementEnd\n",
		"-- +goose StatementBegin\nselect 1;\n-- +goose StatementEnd\n",
	);
}

/// A statement squill can't parse passes through verbatim, and the
/// markers around it stay where they were.
#[test]
fn markers_around_an_unparsable_statement_stay_put() {
	let source = "-- +goose Up\n-- +goose StatementBegin\nFROBNICATE the  thing;\n-- +goose StatementEnd\n-- +goose Down\nDROP   TABLE a;\n";
	assert_eq!(
		format(source).0,
		"-- +goose Up\n-- +goose StatementBegin\nFROBNICATE the  thing;\n-- +goose StatementEnd\n-- +goose Down\ndrop table a;\n"
	);
}
