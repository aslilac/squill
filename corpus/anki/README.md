# Anki SQL corpus

Vendored snapshot of the SQL files from [Anki](https://github.com/ankitects/anki), used as a SQLite grammar-coverage corpus for the parser and formatter (see `cargo run -p corpus-report`). Anki keeps its queries in `.sql` files that its Rust code loads at runtime, so this is real query SQL: selects, updates, upserts, rather than just DDL.

- **Upstream commit:** `1f7c8d7c4c402da06ab10b8f5d0acca6d5abf748`
- **Vendored:** 2026-09-25
- **Sources:** `rslib/src/**/*.sql` → the same paths below `rslib/src/`
- **Dialect:** SQLite

Left out: `storage/notetype/field_names_for_notes.sql`, a fragment that ends at `WHERE id IN` for Anki's code to append an id list. It is not SQL until then.

This is a checked-in copy, not a submodule, so working against the corpus never requires network access. To refresh, clone upstream, re-copy the files, and update the commit SHA above.

Upstream is licensed AGPL-3.0-or-later (see `LICENSE`); these files are test fixtures only and are not compiled into or distributed with squill.
