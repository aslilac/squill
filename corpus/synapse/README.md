# Synapse SQL corpus

Vendored snapshot of the database schema from [Synapse](https://github.com/element-hq/synapse), the Matrix homeserver, used as a mixed SQLite/Postgres grammar-coverage corpus for the parser and formatter (see `cargo run -p corpus-report`).

- **Upstream commit:** `a61ce0cfd43f9447fcdec8c3bc9f344a8c63d5ca`
- **Vendored:** 2026-09-25
- **Sources:** `synapse/storage/schema/**/*.{sql,sql.sqlite,sql.postgres}` → the same paths below `synapse/storage/schema/`
- **Left out:** `main/delta/87/02_per_connection_state.sql` and `state/delta/89/01_state_groups_deletion.sql`, which use Synapse's `$%AUTO_INCREMENT_PRIMARY_KEY%$` placeholder. Synapse substitutes it before the SQL reaches a database, so neither file is SQL as written.
- **Dialects:** Synapse runs on both databases. `*.sql.sqlite` files are SQLite, `*.sql.postgres` files are Postgres, and plain `*.sql` files are written to run on both; the harness reads those as SQLite.

This is a checked-in copy, not a submodule, so working against the corpus never requires network access. To refresh, clone upstream, re-copy the files, and update the commit SHA above.

Upstream is licensed AGPL-3.0 (see `LICENSE`; it is also available under a commercial license); these files are test fixtures only and are not compiled into or distributed with squill.
