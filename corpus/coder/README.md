# coder/coder SQL corpus

Vendored snapshot of the SQL files from [coder/coder](https://github.com/coder/coder),
used as the grammar-coverage corpus for the parser and formatter
(see `cargo run -p cli --bin corpus-report`).

- **Upstream commit:** `36e36e204864b114e7524e26f0f9435f37849449`
- **Vendored:** 2026-07-17
- **Sources:**
  - `coderd/database/migrations/*.sql` → `migrations/`
  - `coderd/database/queries/*.sql` → `queries/`

This is a checked-in copy, not a submodule, so working against the corpus never
requires network access. To refresh, sparse-clone upstream, re-copy the two
directories, and update the commit SHA above.

Upstream is licensed AGPL-3.0; these files are test fixtures only and are not
compiled into or distributed with squill.
