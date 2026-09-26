# Vaultwarden SQL corpus

Vendored snapshot of the SQLite migrations from [Vaultwarden](https://github.com/dani-garcia/vaultwarden), used as a SQLite grammar-coverage corpus for the parser and formatter (see `cargo run -p corpus-report`). Diesel migrations: DDL, and the SQLite way of altering a table by copying it.

- **Upstream commit:** `061694d0cb3bbf5d4c7e920c892824f0020cff83`
- **Vendored:** 2026-09-25
- **Sources:** `migrations/sqlite/**/*.sql` → `migrations/`
- **Dialect:** SQLite

This is a checked-in copy, not a submodule, so working against the corpus never requires network access. To refresh, clone upstream, re-copy the files, and update the commit SHA above.

Upstream is licensed AGPL-3.0 (see `LICENSE`); these files are test fixtures only and are not compiled into or distributed with squill.
