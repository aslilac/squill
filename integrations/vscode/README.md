# squill for VS Code

Formats SQL with [squill](https://github.com/aslilac/squill), a formatter for Postgres and SQLite: `.sql` files, and the SQL embedded in your Rust, Go, Python, JavaScript/TypeScript, Gleam, C++, C#, Java, and Kotlin. It also shows what squill leaves alone (statements it can't parse, embedded strings it won't rewrite) as warnings in the editor.

The extension runs squill's language server (`squill language-server start`). It uses the `squill` on your `PATH` if there is one, so the editor formats exactly as your CLI and CI do; otherwise it downloads the latest release from GitHub (macOS on Apple silicon, Linux and Windows on x64 and ARM64), checks it against the SHA-256 digest GitHub reports for it, and looks for a newer one at most once a day. To use a particular squill, set `squill.path`.

## Formatting SQL files

squill registers as a formatter for SQL. Make it the default and format on save:

```json
{
	"[sql]": {
		"editor.defaultFormatter": "aslilac.squill",
		"editor.formatOnSave": true
	}
}
```

## Formatting embedded SQL

VS Code runs one formatter per file, and your Rust files already have one. squill instead offers a **Format SQL with squill** code action (`source.formatSql`), which VS Code can run on save, after the formatter:

```json
{
	"[rust]": {
		"editor.codeActionsOnSave": {
			"source.formatSql": "explicit"
		}
	}
}
```

Embedded SQL is only formatted in files that an `[[embedded]]` rule in your `squill.toml` covers; run `squill init` to set one up. See [embedded SQL](https://mckayla.dev/squill/docs/embedded/).

## Settings

- `squill.path`: the `squill` executable to run. Empty (the default) means the one on `PATH`, else a downloaded one.
- `squill.trace.server`: log the messages to and from the language server (`off`, `messages`, `verbose`).

The **squill: Restart Language Server** command restarts it, and changing `squill.path` restarts it too. Edits to `squill.toml` apply without a restart.
