# squill for Zed

Runs [squill](https://github.com/aslilac/squill)'s language server, `squill language-server start`: formatting for SQL files and for the SQL embedded in your Rust, Go, Python, JavaScript/TypeScript, Gleam, C++, C#, Java, Kotlin, and Swift, plus highlighting for that embedded SQL and warnings for what squill leaves alone (statements it can't parse, embedded strings it won't rewrite).

The extension uses the `squill` on your `PATH` if there is one, so the editor formats exactly as your CLI and CI do; otherwise it downloads the latest release from GitHub (macOS on Apple silicon, Linux and Windows on x86-64 and ARM64). Syntax support for SQL files comes from Zed's SQL extension.

Install it from Zed's extensions: run **zed: extensions** and search for squill.

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

Zed runs formatters in order, so keep the language's own and follow it with squill. squill highlights the embedded SQL too, as semantic tokens, which Zed only asks for when `semantic_tokens` is on: `"combined"` lays them over Zed's own highlighting (`"full"` would replace it, leaving only the SQL colored).

```json
{
	"languages": {
		"Rust": {
			"formatter": [
				{ "language_server": { "name": "rust-analyzer" } },
				{ "language_server": { "name": "squill" } }
			],
			"semantic_tokens": "combined"
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
			"binary": {
				"path": "/path/to/squill",
				"arguments": ["language-server", "start"]
			}
		}
	}
}
```

`binary.path` overrides both the `squill` on your `PATH` and the downloaded one. Edits to `squill.toml` apply without a restart.
