# squill for Zed

Runs [squill](https://github.com/aslilac/squill)'s language server, `squill lsp`: formatting for SQL files and for the SQL embedded in your Rust, Go, Python, JavaScript/TypeScript, Gleam, C++, C#, Java, and Kotlin, plus warnings for what squill leaves alone (statements it can't parse, embedded strings it won't rewrite).

squill itself must be installed and on your `PATH`; see [installation](https://github.com/aslilac/squill#installation). Syntax support for SQL files comes from Zed's SQL extension.

To install this extension from a checkout, run **zed: install dev extension** and pick this directory.

## Formatting SQL files

```json
{
	"languages": {
		"SQL": {
			"formatter": { "language_server": { "name": "squill" } },
			"format_on_save": "on"
		}
	}
}
```

## Formatting embedded SQL

Zed runs formatters in order, so keep the language's own and follow it with squill:

```json
{
	"languages": {
		"Rust": {
			"formatter": [
				{ "language_server": { "name": "rust-analyzer" } },
				{ "language_server": { "name": "squill" } }
			]
		}
	}
}
```

Embedded SQL is only formatted in files that an `[[embedded]]` rule in your `squill.toml` covers; run `squill init` to set one up.

## Settings

```json
{
	"lsp": {
		"squill": {
			"binary": { "path": "/path/to/squill", "arguments": ["lsp"] }
		}
	}
}
```

`binary.path` overrides the `squill` found on your `PATH`. Edits to `squill.toml` apply without a restart.
