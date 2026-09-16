# squill

A SQL formatter for Postgres and SQLite.

Get familiar with its code style in the [playground](https://mckayla.dev/squill/playground). The playground works in any modern browser, and your input stays on your machine.

## Installation

### Mise

You can use [mise](https://mise.jdx.dev) and the Github backend, which will download a precompiled binary from the latest Github release.

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
cargo install --git https://github.com/aslilac/squill.git --tag v0.1.1 cli --bin squill
```

## Usage

### Options

Set in the nearest `squill.toml` or `.config/squill.toml`, overridable with flags. The search upward stops at a git repository root, a mount point, or a symlinked directory, so a config outside a checkout never reaches inside it. The defaults are the house style; everything is optional.

| key | flag | values | default |
| --- | --- | --- | --- |
| `dialect` | `--dialect` | `postgres` \| `sqlite` | `postgres` |
| `indent` | `--indent` | `tab` \| `spaces` | `tab` |
| `indent-width` | `--indent-width` | width of one level (and tab measure) | `2` |
| `max-width` | `--max-width` | target line width, 20 to 500 | `80` |
| `keyword-case` | `--keyword-case` | `lower` \| `upper` | `lower` |
| `quote-idents` | `--quote-idents` | `as-needed` \| `always` | `as-needed` |
| `at-params` | `--at-params` | lex [sqlc-style](https://docs.sqlc.dev/en/latest/howto/named_parameters.html) `@name` parameters | `false` |
| `ignore` | `--ignore` | glob patterns to skip when recursing | `[]` |

Flags with no config key: `--check` (print diffs and exit 1 if any file would change), `--stdin` / `--stdout`, `--strict` (fail on statements that could not be parsed), `--no-config`, `--embedded`, and `--embedded-query`.

When recursing directories, squill honors `.gitignore` and skips hidden files; the `ignore` key and repeated `--ignore` flags skip more, with `*`, `**`, `?`, `[abc]`, and `{a,b}` glob syntax. Explicitly listed files always format.

### SQL in your source code

SQL embedded in host code formats too: `squill fmt src/queries.rs` rewrites the string literals inside `sqlx::query!`-family macros, with built-in support for Rust, Go (`database/sql` calls), Python (`.execute`-family and `text(...)`, with `%s` / `%(name)s` params preserved), JavaScript/TypeScript/TSX (`.query`/`.execute`/`.prepare` and `sql`-tagged templates), and Gleam (`sqlight.query` as SQLite, `pog.query` etc. as the session dialect). Only multiline string syntaxes — raw strings, backticks, triple quotes, templates, Gleam strings — are reformatted, always into a vertical block: quotes on their own lines, one clause per line. Plain single-line strings stay byte-identical, and so do Python f-strings and `${}`-interpolated templates (SQL with holes is never touched). Directories include host files with `--embedded`; `--embedded-query custom.scm` swaps the tree-sitter extraction query.

### Per-language configuration

Embedded SQL copies the host file's own indent character, so a spaces-indented file never gains tabs. To set it deliberately — per language, from one config file — add a `[rust]`, `[go]`, `[python]`, `[javascript]`, `[typescript]`, or `[gleam]` section. A section takes the same keys as the top level (all but `ignore`, which is file-wide) and overrides them for files of that language:

```toml
indent = "tab"

[javascript]
indent = "spaces"
indent-width = 2

[python]
dialect = "sqlite"
indent = "spaces"
indent-width = 4
```

`[javascript]` covers `.js`/`.jsx` and `[typescript]` covers `.ts`/`.tsx`. Flags still win over both layers.

One section can name several languages, comma separated — TOML has no bare comma in a table header, so quote the list:

```toml
["javascript, typescript"]
indent = "spaces"
indent-width = 2
```

## Design

- **`crates/parser`** — hand-written dual-dialect lexer (lossless: every byte is a token, including comments), recursive-descent parser with Pratt expressions, error recovery into verbatim `ErrorStatement` nodes, and a codegen'd CST layer on `cstree` (see `syntax.def`).
- **`crates/formatter`** — Wadler/Prettier doc IR and renderer, the CST-to-doc rules, vendored Postgres/SQLite keyword tables, and the semantics-preserving identifier-quoting transform.
- **`crates/embed`** — formats SQL embedded in host files (Rust sqlx macros, Go database/sql calls) located via tree-sitter queries.
- **`crates/cli`** — the `squill` binary.
- **`crates/corpus-report`** — the corpus coverage harness (not installed with the cli).

## License

[MPL-2.0](LICENSE).
