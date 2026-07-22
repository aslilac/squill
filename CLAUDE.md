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
cargo run -p cli --bin corpus-report -- --summary       # corpus coverage numbers
```

Snapshot tests use `insta` (formatter snapshots cover every file in `corpus/coder/`). After an intentional formatting change, review/accept with `cargo insta review` (or `INSTA_UPDATE=always cargo test ...` then inspect the diff).

Docs site (`docs/`, Astro + Monaco playground): `pnpm dev` / `pnpm build`. The pre-step compiles `crates/playground` to wasm and needs `rustup target add wasm32-wasip1` plus wasi-sdk at `~/.local/wasi-sdk` (see `docs/README.md`).

## Architecture

Pipeline: **lex → parse (CST) → doc IR → render → safety check**, one crate per stage:

- **`crates/parser`** — hand-written dual-dialect lexer (lossless: every byte, including whitespace/comments, is a token), recursive-descent parser with Pratt expressions (`src/parser/{grammar,dml,ddl,expr,plpgsql}.rs`), CST on `cstree`. Unparseable statements recover into verbatim `ErrorStatement` nodes rather than failing.
  - **`syntax.def` is the source of truth for all token/node kinds.** `build.rs` code-gens the `SyntaxKind` enum and the typed AST accessor layer (`src/ast.rs`) from it. To add a node kind or accessor, edit `syntax.def`, not the generated code. The format is documented at the top of that file.
- **`crates/formatter`** — Wadler/Prettier-style doc IR (`doc.rs`), renderer (`printer.rs`), CST→doc rules (`rules.rs`), vendored keyword tables (`keywords.rs`), identifier-quoting transform (`quoting.rs`), and the safety check (`check.rs`).
- **`crates/embed`** — formats SQL embedded in host source (Rust sqlx macros, Go, Python, JS/TS, Gleam) located via tree-sitter queries. Only multiline string syntaxes are rewritten; interpolated strings (f-strings, `${}` templates) are never touched.
- **`crates/cli`** — the `squill` binary (config resolution: nearest `squill.toml`, flags win) and the `corpus-report` coverage harness.
- **`crates/playground`** — wasm module for the docs-site playground (built with the `wasm-release` profile).

`crates/parser/fuzz/` and `crates/embed/tests/fixtures/sqlx_app/` are excluded from the workspace.

## The safety oracle (core invariant)

Every formatted statement is re-lexed and compared against its input; on any mismatch the original text passes through verbatim — output must never be less correct than input. Tests enforce this corpus-wide (`crates/formatter/tests/oracle.rs`) over the vendored `corpus/coder/` tree:

1. **Idempotence** — `format(format(x)) == format(x)`
2. **Token equivalence** — non-trivia token stream unchanged, modulo keyword case and sanctioned quote changes
3. **Comment conservation** — never dropped, duplicated, or reordered

Any change to lexer, parser, or formatter rules must keep the oracle green; don't weaken `check.rs` to make a formatting change pass.

## CI / releases

CI is Forgejo Actions (`.forgejo/workflows/ci.yml`): fmt, clippy `-D warnings`, tests, corpus report — `runs-on: ubuntu-26.04` with Rust preinstalled on the runner image (no `container:`). Tags `v*` trigger `release.yml`, which builds a static x86_64 binary (`RUSTFLAGS=-C target-feature=+crt-static` on the stock gnu target) and uploads it to the Forgejo release via plain API calls.
