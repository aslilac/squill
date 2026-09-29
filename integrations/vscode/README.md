# squill for VS Code

Formats SQL with [squill](https://github.com/aslilac/squill), a formatter for Postgres and SQLite: `.sql` files, and the SQL embedded in your Rust, Go, Python, JavaScript/TypeScript, Gleam, C++, C#, Java, Kotlin, and Swift. It also shows what squill leaves alone (statements it can't parse, embedded strings it won't rewrite) as warnings in the editor.

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

## Highlighting embedded SQL

The SQL in strings that an `[[embedded]]` rule covers is highlighted: its keywords, names, functions, types, numbers, parameters, and comments. Its own strings and punctuation keep your theme's string color. VS Code doesn't let an extension use the theme's syntax colors here, so these are theme colors of their own, matching the Dark+ and Light+ themes by default. A theme can set them, or you can:

```json
{
	"workbench.colorCustomizations": {
		"squill.sql.keyword": "#c678dd",
		"squill.sql.name": "#e06c75"
	}
}
```

The others are `squill.sql.function`, `squill.sql.type`, `squill.sql.number`, `squill.sql.parameter`, and `squill.sql.comment`. To turn the highlighting off, set `squill.highlightEmbeddedSql` to `false`.

## Settings

- `squill.path`: the `squill` executable to run. Empty (the default) means the one on `PATH`, else a downloaded one.
- `squill.highlightEmbeddedSql`: highlight the SQL embedded in other languages' strings (on by default).
- `squill.trace.server`: log the messages to and from the language server (`off`, `messages`, `verbose`).

The **squill: Restart Language Server** command restarts it, and changing `squill.path` restarts it too. Edits to `squill.toml` apply without a restart.
