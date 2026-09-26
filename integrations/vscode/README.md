# squill for VS Code

Formats SQL with [squill](https://github.com/aslilac/squill), a formatter for Postgres and SQLite: `.sql` files, and the SQL embedded in your Rust, Go, Python, JavaScript/TypeScript, Gleam, C++, C#, Java, and Kotlin. It also shows what squill leaves alone (statements it can't parse, embedded strings it won't rewrite) as warnings in the editor.

The extension runs `squill lsp`, so squill itself must be installed. See [installation](https://github.com/aslilac/squill#installation). If it isn't on your `PATH`, set `squill.path`.

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

- `squill.path`: the `squill` executable, a name on `PATH` or a path. Default `squill`.
- `squill.trace.server`: log the messages to and from the language server (`off`, `messages`, `verbose`).

The **squill: Restart Language Server** command restarts it, and changing `squill.path` restarts it too. Edits to `squill.toml` apply without a restart.
