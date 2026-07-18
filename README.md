# squill

A SQL formatter for Postgres and SQLite, built on a lossless CST.

```console
$ echo 'SELECT id,name FROM users WHERE org_id=$1 ORDER BY name;' | squill fmt --stdin
select id, name from users where org_id = $1 order by name;

$ echo 'SELECT u.id,count(*) FILTER (WHERE o.active) AS n FROM users u LEFT JOIN orgs o ON o.id=u.org_id GROUP BY u.id ORDER BY n DESC;' | squill fmt --stdin
select u.id, count(*) filter (where o.active) as n
from users u left join orgs o on o.id = u.org_id
group by u.id
order by n desc;
```

Short statements collapse onto one line; long ones break clause-per-line
at 80 columns.

## Quickstart

```console
cargo run -p cli --bin squill -- fmt path/to/queries/   # format in place
cargo run -p cli --bin squill -- fmt --check .          # CI mode: diff + exit 1
squill fmt --help                                       # the full flag surface
```

Options (via the nearest `squill.toml` or flags — flags win): `--dialect postgres|sqlite`,
`--indent tab|spaces`, `--indent-width N` (default 2), `--keyword-case
lower|upper`, `--quote-idents unquote-safe|always`, `--at-params` for
sqlc-style `@name` parameters, `--strict` to fail on statements that
could not be parsed.

SQL embedded in host code formats too: `squill fmt src/queries.rs`
rewrites the string literals inside `sqlx::query!`-family macros (and
`database/sql` calls in Go). Only multi-line literals are reformatted —
single-line strings stay byte-identical — and the quotes sit on their
own lines around the SQL. Directories include host files with
`--embed`; `--embed-query custom.scm` swaps the tree-sitter extraction
query.

## Design

- **`crates/parser`** — hand-written dual-dialect lexer (lossless: every
  byte is a token, including comments), recursive-descent parser with
  Pratt expressions, error recovery into verbatim `ErrorStatement`
  nodes, and a codegen'd CST layer on `cstree` (see `syntax.def`).
- **`crates/formatter`** — Wadler/Prettier doc IR and renderer, the
  CST-to-doc rules, vendored Postgres/SQLite keyword tables, and the
  semantics-preserving identifier-quoting transform.
- **`crates/embed`** — formats SQL embedded in host files (Rust sqlx
  macros, Go database/sql calls) located via tree-sitter queries.
- **`crates/cli`** — the `squill` binary and the `corpus-report`
  coverage harness.

## Why you can trust it

Every formatted statement is re-lexed and compared with its input:
token streams must match modulo whitespace, keyword case, and sanctioned
identifier-quote changes, and comments must survive in order. On any
mismatch the original text passes through verbatim — output is never
less correct than input. CI enforces the safety oracle over the whole
vendored coder/coder corpus (1,168 files, 4,174 statements, 100%
parsed and formatted):

1. **Idempotence** — `format(format(x)) == format(x)`.
2. **Token equivalence** — the meaning-bearing token stream never
   changes.
3. **Comment conservation** — never dropped, duplicated, or reordered.

`cargo run -p cli --bin corpus-report -- --summary` prints the current
coverage numbers.
