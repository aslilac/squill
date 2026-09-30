# squill for JetBrains IDEs

Formats SQL with [squill](https://github.com/aslilac/squill), a formatter for Postgres and SQLite: `.sql` files, and the SQL embedded in your Rust, Go, Python, JavaScript/TypeScript, Gleam, C++, C#, Java, Kotlin, and Swift. It highlights that embedded SQL, and shows what squill leaves alone (statements it can't parse, embedded strings it won't rewrite) as warnings in the editor.

The plugin runs squill's language server (`squill language-server start`) through the IDE's own LSP support, so it needs a 2026.1.4 or newer IDE: IntelliJ IDEA, WebStorm, PyCharm, GoLand, RustRover, CLion, Rider, and the rest. It uses the `squill` on your `PATH` if there is one, so the IDE formats exactly as your CLI and CI do; otherwise it downloads the latest release from GitHub (macOS on Apple silicon, Linux and Windows on x64 and ARM64), checks it against the SHA-256 digest GitHub reports for it, and looks for a newer one at most once a day. To use a particular squill, set its path under **Settings | Tools | squill**.

Like any language server in these IDEs, squill runs only in trusted projects, and only for files inside the project.

## Formatting SQL files

**Code | Reformat Code** formats a SQL file with squill. To do it on save, turn on **Reformat code** under **Settings | Tools | Actions on Save**.

## Formatting embedded SQL

Reformat Code keeps the IDE's own formatter for other languages. squill formats their SQL on save instead: turn on **Format SQL with squill** under **Settings | Tools | Actions on Save**. It runs after the IDE's other actions on save, including Reformat code.

Embedded SQL is only formatted in files that an `[[embedded]]` rule in your `squill.toml` covers; run `squill init` to set one up. See [embedded SQL](https://mckayla.dev/squill/docs/embedded/). When a project's squill config has an `[[embedded]]` rule, the plugin offers to turn formatting on save on. **Don't ask again** stops it offering in that project.

## Highlighting embedded SQL

The SQL in strings that an `[[embedded]]` rule covers is highlighted: its keywords, names, functions, types, numbers, parameters, and comments. Its own strings and punctuation keep your color scheme's string color. The colors follow your scheme's language defaults, and can be set on their own under **Settings | Editor | Color Scheme | squill**. To turn the highlighting off, clear **Highlight the SQL embedded in other languages' strings** under **Settings | Tools | squill**.

## Settings

Under **Settings | Tools | squill**:

- **Executable**: the `squill` to run. Empty (the default) means the one on `PATH`, else a downloaded one. Changing it restarts squill.
- **Highlight the SQL embedded in other languages' strings** (on by default).

The language services widget in the status bar shows squill's state, and restarts it. Edits to `squill.toml` apply without a restart.

## Building

With the repository's nix dev shell (Gradle 9 and Java 21), or any Java 21:

```sh
./gradlew buildPlugin   # build/distributions/squill-<version>.zip
./gradlew runIde        # try it in a sandboxed IDE
./gradlew test          # against ../../target/debug/squill, or $SQUILL
./gradlew verifyPlugin  # the Plugin Verifier, against the oldest and newest IDEs
```

`SQUILL_TEST_DOWNLOAD=1 ./gradlew test` also tests downloading the latest release from GitHub.
