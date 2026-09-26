# squill

A SQL formatter for Postgres and SQLite.

Get familiar with its code style in the [playground](https://mckayla.dev/squill/playground). The playground works in any modern browser, and your input stays on your machine.

## Installation

### Mise

You can use [mise](https://mise.jdx.dev) to download a precompiled binary from the latest Github release.

```sh
mise use -g github:aslilac/squill
squill fmt --check .
```

### Nix

A Nix flake is available if you're a Nix fan.

```sh
nix shell github:aslilac/squill
```

### Compile from source

Cargo can clone the source, checkout the latest release tag, and install the binary in a single command. 

```sh
cargo install --git https://github.com/aslilac/squill.git --tag v0.3.0 cli --bin squill
```

Every built-in grammar for embedded SQL is its own cargo feature (`rust`, `go`, `python`, `javascript`, `typescript`, `gleam`, `cpp`, `csharp`, `java`, `kotlin`), as is `external-grammars`, which loads grammars from `.wasm` files at runtime (and needs `cmake` to build). All are on by default; for a smaller binary, pick just the ones you need:

```sh
cargo install --git https://github.com/aslilac/squill.git --tag v0.3.0 cli --bin squill --no-default-features --features rust,go
```

## Usage

```sh
squill init         # write a starter squill.toml: pick languages to format embedded SQL in, and dialects
squill fmt --check .
```

### Options

Set in the nearest `squill.toml` or `.config/squill.toml`, overridable with flags. The search upward stops at a git repository root, a mount point, or a symlinked directory, so a config outside a checkout never reaches inside it. The defaults are the house style; everything is optional.

| key | flag | values | default |
| --- | --- | --- | --- |
| `dialect` | `--dialect` | `postgres` \| `sqlite` | `postgres` |
| `indent` | `--indent` | `tabs` \| `spaces` | `tabs` |
| `indent-width` | `--indent-width` | width of one level (and tab measure) | `2` |
| `max-width` | `--max-width` | target line width, 20 to 500 | `80` |
| `keyword-case` | `--keyword-case` | `lower` \| `upper` | `lower` |
| `quote-idents` | `--quote-idents` | `as-needed` \| `always` | `as-needed` |
| `at-params` | `--at-params` | lex [sqlc-style](https://docs.sqlc.dev/en/latest/howto/named_parameters.html) `@name` parameters | `false` |
| `question-params` | `--question-params` | lex JDBC-style `?` parameters in Postgres | `false` (`true` for `java` and `kotlin` grammars) |
| `pyformat-params` | `--pyformat-params` | lex Python DB-API `%s` / `%(name)s` parameters | `false` (`true` for the `python` grammar) |
| `ignore` | `--ignore` | glob patterns to skip when recursing | `[]` |
| `frozen` | `--frozen` | glob patterns that are immutable once on the baseline ref | `[]` |
| `frozen-ref` | `--frozen-ref` | the baseline ref `frozen` compares against | discovered from the remote |
| `frozen-fetch` | `--frozen-fetch` / `--no-frozen-fetch` | let `frozen` ask the remote for its HEAD when it isn't recorded locally | `true` |

Flags with no config key: `--check` (print diffs and exit 1 if any file would change), `--stdin` (or `-` as the path) / `--stdout`, `--stdin-filepath <path>` (read stdin as the file at that path, for editors: its config, rules, and ignores apply, and it need not exist yet), `--strict` (fail when anything was left unformatted), `--locked` (fail rather than record a grammar URL `squill.lock` doesn't have, for CI), `--no-config`, `--version` / `-V`, and `--help` / `-h`.

When recursing directories, squill honors `.gitignore` and skips hidden files; the `ignore` key and repeated `--ignore` flags skip more, with `*`, `**`, `?`, `[abc]`, and `{a,b}` glob syntax. Explicitly listed files always format.

### Frozen paths

Some files can't be rewritten after they ship, even into an identical-meaning form. sqlx records a checksum of every migration and refuses to run when one changes — but a *new* migration should still be formatted, and ignoring the whole directory gives that up.

`frozen` marks paths that squill formats only while they are new:

```toml
frozen = ["migrations/**"]
```

A matching file is skipped once it exists in the baseline ref's tree, so the shape it shipped in is the shape it keeps — including when squill's own style changes later, and including when you adopt squill on a codebase whose existing migrations were never formatted. New files under the same globs format normally.

This is not `ignore`: it applies to files named explicitly on the command line too, since the point is that they are never rewritten. If a frozen path can't be checked — no git repository, or a baseline ref that doesn't resolve — squill stops with an error rather than guess, because guessing wrong is the failure the setting exists to prevent.

The baseline is read with one `git ls-tree` per repository, and only the ref's tip is needed — so a CI checkout at `fetch-depth: 1` is enough.

Every baseline comes from the remote, never from a guess about which branch is which. In order: `frozen-ref` if you set one; then `refs/remotes/<remote>/HEAD`, which `git clone` records; then a `--depth=1` fetch of the remote's HEAD. Whatever your default branch is called, it just works.

That last step is why CI works unchanged. A `pull_request` checkout has no base-branch ref at all, and a push build of a topic branch has exactly one remote-tracking ref which is the *topic* branch — taking it as the baseline would freeze migrations that never shipped. Only the remote can tell those apart.

`--no-frozen-fetch` (or `frozen-fetch = false`) keeps squill off the network, for an offline or air-gapped build. A normal clone still works, because `git clone` already recorded the remote's HEAD. When nothing authoritative is available, squill stops and asks for `frozen-ref` rather than inferring one.

### Editors

Editors pipe the buffer through `squill fmt --stdin-filepath <path>`, which formats it as that file would be formatted on disk (config, rules, embedded SQL and all). Setups for Helix, Zed, and VS Code are in the [editor docs](https://mckayla.dev/squill/docs/editors/).

### Rules

Rules scope settings to paths. Each needs an `include` list of globs (relative to the config, like `ignore`); every rule whose `include` matches a file applies, in file order, later rules winning key by key.

`[[files]]` rules cover plain SQL. `*.sql` files always format; a rule sets options for the files it matches and brings in files by other names, so a mixed-dialect tree needs one config:

```toml
dialect = "postgres"

[[files]]
include = ["**/*.sql.sqlite", "storage/sqlite/**"]
dialect = "sqlite"
```

### SQL in your source code

`[[embedded]]` rules format SQL embedded in host code. A rule names the tree-sitter `grammar` that parses the file and, optionally, a `query` that finds the SQL strings in it:

```toml
[[embedded]]
include = ["**/*.rs"]
grammar = "rust"
dialect = "sqlite"

[[embedded]]
include = ["web/**/*.ts"]
grammar = "typescript"
indent = "spaces"
indent-width = 2
```

Built-in grammars, each with a default query: `rust` (sqlx's `query!`-family macros and `query`/`query_as`/`query_scalar` functions), `go` (`database/sql` calls), `python` (`.execute`-family and `text(...)`, with `%s` / `%(name)s` params preserved), `javascript`/`typescript`/`tsx` (`.query`/`.execute`/`.prepare` and `sql`-tagged templates), `gleam` (`sqlight.query` as SQLite, `pog.query` etc. as the configured dialect), `cpp` (raw strings passed to sqlite3, libpq, and libpqxx), `csharp` (raw strings in EF Core migrations, raw-SQL and Dapper calls, and `CommandText`), and `java`/`kotlin` (text blocks and raw strings passed to JDBC, JPA, Spring, and Exposed calls, with `?` placeholders preserved).

Any other language works with a grammar compiled to wasm (`tree-sitter build --wasm`, or the `.wasm` many grammars publish with each release) and a query of your own:

```toml
[[embedded]]
include = ["**/*.lua"]
grammar = ".config/squill/tree-sitter-lua.wasm"
query = ".config/squill/lua.scm"
```

`grammar` can also be an `https` URL to the `.wasm`. The first download records its SHA-256 in a `squill.lock` beside the config (commit it), and every later download must match; downloads are cached by hash, and `squill fmt --locked` (for CI) refuses to record a URL the lockfile doesn't have.

Only multiline string syntaxes are reformatted — raw strings, backticks, triple quotes, templates, text blocks, Gleam strings — always into a vertical block: quotes on their own lines, one clause per line. A plain Rust or Go string that already spans lines is rewritten as a raw string; a single-line one stays byte-identical, and so do Python f-strings and interpolated templates (SQL with holes is never touched). Every rewrite is checked by re-parsing the host file, and a string squill declines to touch is reported as a diagnostic. Embedded SQL copies the host file's own indent character unless an indent style is configured, so a spaces-indented file never gains tabs.

## Design

- **`crates/parser`** — hand-written dual-dialect lexer (lossless: every byte is a token, including comments), recursive-descent parser with Pratt expressions, error recovery into verbatim `ErrorStatement` nodes, and a codegen'd CST layer on `cstree` (see `syntax.def`).
- **`crates/formatter`** — Wadler/Prettier doc IR and renderer, the CST-to-doc rules, vendored Postgres/SQLite keyword tables, and the semantics-preserving identifier-quoting transform.
- **`crates/embed`** — formats SQL embedded in host files, located via tree-sitter queries over built-in or wasm-loaded grammars.
- **`crates/cli`** — the `squill` binary.
- **`crates/corpus-report`** — the corpus coverage harness (not installed with the cli).

## License

[MPL-2.0](LICENSE).
