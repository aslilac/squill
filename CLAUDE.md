# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`squill` — a SQL formatter for Postgres and SQLite built on a lossless CST. Rust workspace (edition 2024, `unsafe` denied, MPL-2.0).

## Commands

```sh
cargo test --workspace                                  # full test suite
cargo test -p formatter --test snapshots                # one test target
cargo test -p parser parse_select                       # filter by test name
cargo fmt --check                                       # rustfmt (hard tabs, max_width 80)
cargo clippy --workspace --all-targets -- -D warnings   # CI fails on warnings
cargo run -p cli --bin squill -- fmt --stdin            # run the formatter
cargo run -p corpus-report -- --summary                 # corpus coverage numbers
```

Snapshot tests use `insta` (formatter snapshots cover every file of every corpus under `corpus/`, named `<corpus>__<path>`). After an intentional formatting change, review/accept with `cargo insta review` (or `INSTA_UPDATE=always cargo test ...` then inspect the diff).

Docs recipes (`docs/recipes/<name>/`: a `squill.toml`, maybe a query, `before.*` and `after.*`) are shown on the site by `docs/src/components/Recipe.astro` and checked by `crates/cli/tests/recipes.rs` — regenerate `after` with `squill fmt --stdout before.<ext>` from inside the recipe directory when formatting changes. Recipes whose grammar is an https URL (they carry a `squill.lock`) run only with `cargo test -p cli --test recipes -- --ignored`.

JS lives in one pnpm workspace rooted at the repo (`docs/` and `integrations/vscode/`, one lockfile). Docs site (`docs/`, Astro + Monaco playground): `pnpm dev` / `pnpm build`. The pre-step (`scripts/build-wasm.sh`) compiles `crates/playground` to wasm; it needs the `wasm32-wasip1` target plus a C toolchain for the tree-sitter grammars. `nix develop` supplies both (see `flake.nix`); otherwise add the target with `rustup` and install wasi-sdk at `~/.local/wasi-sdk` (see `docs/README.md`).

## Architecture

Pipeline: **lex → parse (CST) → doc IR → render → safety check**, one crate per stage:

- **`crates/parser`** — hand-written dual-dialect lexer (lossless: every byte, including whitespace/comments, is a token), recursive-descent parser with Pratt expressions (`src/parser/{grammar,dml,ddl,expr,plpgsql}.rs`), CST on `cstree`. Unparsable statements recover into verbatim `ErrorStatement` nodes rather than failing.
  - **`syntax.def` is the source of truth for all token/node kinds.** `build.rs` code-gens the `SyntaxKind` enum and the typed AST accessor layer (`src/ast.rs`) from it. To add a node kind or accessor, edit `syntax.def`, not the generated code. The format is documented at the top of that file.
- **`crates/formatter`** — Wadler/Prettier-style doc IR (`doc.rs`), renderer (`printer.rs`), CST→doc rules (`rules.rs`), vendored keyword tables (`keywords.rs`), identifier-quoting transform (`quoting.rs`), and the safety check (`check.rs`).
- **`crates/embed`** — formats SQL embedded in host source, located via tree-sitter queries. Built-in grammars: Rust, Go, Python, JS/TS/TSX, Gleam (whole-literal codecs, hand-written per host), plus C++, C#, Java, Kotlin (content capture); every built-in grammar is its own cargo feature (all on in the CLI, the playground takes only the first seven). The `external-grammars` feature loads any grammar from a `.wasm` file at runtime via wasmtime (content capture); it needs `cmake` to build and stays off for the playground. Only multiline string syntaxes are rewritten; interpolated strings (f-strings, `${}` templates) are never touched. Every edit is re-verified by re-parsing the host file; declined strings come back as `Warning`s.
- **`crates/cli`** — the `squill` binary (config resolution: nearest `squill.toml` or `.config/squill.toml` (or `squill.yaml`/`.yml`, parsed with saphyr into the same tree as TOML; two in one directory is an error), then every matching `[[files]]` rule (plain SQL) or `[[embedded]]` rule (host files: `grammar`, `query`), in file order, then flags; directory recursion respects `.gitignore` plus `ignore` globs from config/`--ignore`; `frozen` globs skip files already present in a baseline git ref, via `frozen.rs`; `grammar` may be an https URL, downloaded with reqwest and locked by SHA-256 in a `squill.lock` beside the config, cached by hash, via `remote.rs`; `--locked` refuses new lock entries).
- **`crates/corpus`** — the list of vendored corpora (`corpus/coder` Postgres, `corpus/anki` and `corpus/vaultwarden` SQLite, `corpus/synapse` both, by file suffix) and each file's dialect and lex options. Every corpus harness walks it; a new corpus is one entry there plus a directory with a README and LICENSE.
- **`crates/corpus-report`** — the corpus coverage harness (CI-only, kept out of `cargo install`).
- **`crates/playground`** — wasm module for the docs-site playground (built with the `wasm-release` profile).
- **`integrations/`** — editor clients for `squill language-server start` (served by `crates/cli/src/lsp.rs`): `vscode/` (TypeScript, esbuild-bundled; formats SQL only, embedded SQL via the `source.formatSql` code action; without squill on PATH it downloads the latest release, verified against the SHA-256 `digest` GitHub's API reports for each release asset; the manually run `.github/workflows/vscode.yaml` packages it; extension versions are independent of squill's). The Zed extension likewise downloads the latest release when squill isn't on PATH, so GitHub's newest release must always be squill's and `zed/` (its own Cargo workspace, excluded from squill's; Zed loads extensions as components, so it builds for `wasm32-wasip2`, unlike the playground's `wasm32-wasip1` module the browser loads. The nix dev shell has both targets: `cargo build --manifest-path integrations/zed/Cargo.toml --target wasm32-wasip2`).

`crates/parser/fuzz/` and `crates/embed/tests/fixtures/sqlx_app/` are excluded from the workspace.

## The safety oracle (core invariant)

Every formatted statement is re-lexed and compared against its input; on any mismatch the original text passes through verbatim — output must never be less correct than input. Tests enforce this corpus-wide (`crates/formatter/tests/oracle.rs`) over every vendored corpus under `corpus/`, each file in its own dialect:

1. **Idempotence** — `format(format(x)) == format(x)`
2. **Token equivalence** — non-trivia token stream unchanged, modulo keyword case and sanctioned quote changes
3. **Comment conservation** — never dropped, duplicated, or reordered

Any change to lexer, parser, or formatter rules must keep the oracle green; don't weaken `check.rs` to make a formatting change pass.

## CI / releases

CI is Forgejo Actions (`.forgejo/workflows/check.yaml`): typos spellcheck, fmt, clippy `-D warnings`, tests, corpus report — `runs-on: ubuntu-26.04` with Rust preinstalled on the runner image (no `container:`). Tags `v*` trigger `release.yaml`, which builds a static x86_64 binary (`RUSTFLAGS=-C target-feature=+crt-static` on the stock gnu target) and uploads it to the Forgejo release via plain API calls.
