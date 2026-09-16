//! squill: format SQL files.
//!
//! Thin CLI over the formatter library, suitable for pre-commit and CI.
//! `squill fmt <paths...>` writes in place; `--check` diffs and exits 1;
//! `--stdin`/`--stdout` stream. Options come from the nearest
//! squill.toml (see `config`), overridden by explicit flags.

use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use formatter::Options;
use rayon::prelude::*;

mod config;
mod frozen;
use config::PartialOptions;

/// `squill <version>` — whatever cargo compiled this binary with, so it
/// tracks the workspace manifest without a second place to bump.
const VERSION: &str = concat!("squill ", env!("CARGO_PKG_VERSION"));

const USAGE: &str = "\
squill — a SQL formatter

Usage: squill fmt [OPTIONS] [PATHS...]

Formats the given .sql files in place (directories are searched
recursively). Reads stdin when --stdin is given.

Options:
  --check                 Don't write; print diffs and exit 1 if any file
                          would change
  --stdout                Print formatted output instead of writing files
  --stdin                 Read from stdin, write to stdout
  --strict                Exit 1 when statements could not be parsed and
                          passed through verbatim
  --ignore <GLOB>         Skip matching paths when recursing directories
                          (repeatable; relative to the working directory;
                          `*`, `**`, `?`, `[abc]`, `{a,b}` globs)
  --dialect <D>           postgres (default) | sqlite
  --indent <STYLE>        tab (default) | spaces
  --indent-width <N>      Indent width (and tab measure), default 2
  --max-width <N>         Target line width (20 to 500), default 80
  --keyword-case <CASE>   lower (default) | upper
  --quote-idents <MODE>   as-needed (default) | always
  --at-params             Treat sqlc-style @name as parameters (Postgres)
  --no-config             Ignore squill.toml files
  --frozen <GLOB>         Treat matching paths as immutable once they
                          exist on the baseline ref: format them while
                          new, never rewrite them after (repeatable)
  --frozen-ref <REF>      Baseline ref for --frozen (default: whatever
                          the remote records as its HEAD)
  --frozen-fetch          Let --frozen fetch the remote's HEAD when no
                          baseline ref is available locally
  --embedded              Also format SQL embedded in host files (.rs,
                          .go, .py, .js/.ts/.tsx, .gleam) when recursing
                          directories (explicit host paths always format)
  --embedded-query <SCM>  Override the tree-sitter extraction query
  -V, --version           Print the version and exit
  -h, --help              Show this help

Configuration: the nearest squill.toml or .config/squill.toml at or
above each formatted file supplies defaults (keys: dialect, indent,
indent-width, max-width, keyword-case, quote-idents, at-params, ignore
and frozen — arrays of glob patterns relative to the config file — and
frozen-ref).
Explicit flags override the config. The search upward stops at a git
repository root, a mount point, or a symlinked directory, so a config
outside a checkout never reaches inside it. Directory recursion honors
.gitignore and skips hidden files; explicitly listed files always
format.

A [rust], [go], [python], [javascript], [typescript], or [gleam]
section takes the same keys (except ignore) and overrides them for SQL
embedded in files of that language — so one config can ask for two
spaces in JavaScript and tabs in Go:

    indent = \"tab\"

    [javascript]
    indent = \"spaces\"
    indent-width = 2

One section can name several languages, comma separated. TOML has no
bare comma in a table header, so quote the list:

    [\"javascript, typescript\"]
    indent = \"spaces\"
    indent-width = 2

Embedded SQL copies the host file's own indent character unless an
indent style is configured, so a spaces-indented file never gains tabs
by accident.

Frozen paths are for files that cannot change after they ship, such as
sqlx migrations, whose checksums a reformat would break:

    frozen = [\"migrations/**\"]

A matching file is formatted while it is new and skipped once it exists
on the baseline ref, so squill's own style can move on without ever
rewriting one. Unlike ignore, this applies to files named explicitly on
the command line too — the point is that they are never rewritten.
";

struct Args {
	check: bool,
	stdout_mode: bool,
	stdin_mode: bool,
	strict: bool,
	no_config: bool,
	/// Include host-language files when recursing directories.
	embed: bool,
	/// Override the tree-sitter extraction query (.scm source).
	embed_query: Option<String>,
	frozen: Vec<String>,
	frozen_ref: Option<String>,
	frozen_fetch: bool,
	/// Glob patterns to skip when recursing (cwd-relative).
	ignore: Vec<String>,
	overrides: PartialOptions,
	paths: Vec<PathBuf>,
}

/// What the command line asked for: work to do, or a message to print on
/// the way out. `--help` and `--version` are requests that were answered,
/// not errors, so they go to stdout and exit 0.
enum Invocation {
	Run(Box<Args>),
	Print(String),
}

fn parse_args() -> Result<Invocation, String> {
	let mut argv = std::env::args().skip(1).peekable();
	match argv.next().as_deref() {
		Some("fmt") => {}
		Some("-h" | "--help") => return Ok(Invocation::Print(USAGE.to_string())),
		Some("-V" | "--version") => {
			return Ok(Invocation::Print(VERSION.to_string()));
		}
		// A bare `squill` names no command: usage, but as a complaint.
		None => return Err(USAGE.to_string()),
		Some(other) => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
	}
	let mut args = Args {
		check: false,
		stdout_mode: false,
		stdin_mode: false,
		strict: false,
		no_config: false,
		embed: false,
		embed_query: None,
		frozen: Vec::new(),
		frozen_ref: None,
		frozen_fetch: false,
		ignore: Vec::new(),
		overrides: PartialOptions::default(),
		paths: Vec::new(),
	};
	let value = |argv: &mut dyn Iterator<Item = String>, flag: &str| {
		argv.next().ok_or_else(|| format!("{flag} needs a value"))
	};
	while let Some(arg) = argv.next() {
		match arg.as_str() {
			"--check" => args.check = true,
			"--stdout" => args.stdout_mode = true,
			"--stdin" => args.stdin_mode = true,
			"--strict" => args.strict = true,
			"--no-config" => args.no_config = true,
			"--embedded" => args.embed = true,
			"--embedded-query" => {
				let path = value(&mut argv, "--embedded-query")?;
				args.embed_query = Some(
					std::fs::read_to_string(&path)
						.map_err(|err| format!("--embedded-query {path}: {err}"))?,
				);
			}
			"--ignore" => args.ignore.push(value(&mut argv, "--ignore")?),
			"--frozen" => args.frozen.push(value(&mut argv, "--frozen")?),
			"--frozen-ref" => {
				args.frozen_ref = Some(value(&mut argv, "--frozen-ref")?)
			}
			"--frozen-fetch" => args.frozen_fetch = true,
			"--at-params" => args.overrides.at_params = Some(true),
			"--dialect" => {
				args.overrides.dialect =
					Some(config::parse_dialect(&value(&mut argv, "--dialect")?)?)
			}
			"--indent" => {
				args.overrides.indent_style =
					Some(config::parse_indent(&value(&mut argv, "--indent")?)?)
			}
			"--indent-width" => {
				args.overrides.indent_width = Some(
					value(&mut argv, "--indent-width")?
						.parse()
						.map_err(|_| "--indent-width needs a number".to_string())?,
				)
			}
			"--max-width" => {
				let width: u16 = value(&mut argv, "--max-width")?
					.parse()
					.map_err(|_| "--max-width needs a number".to_string())?;
				if !(20..=500).contains(&width) {
					return Err("--max-width expects 20 to 500".to_string());
				}
				args.overrides.max_width = Some(width);
			}
			"--keyword-case" => {
				args.overrides.keyword_case = Some(config::parse_keyword_case(&value(
					&mut argv,
					"--keyword-case",
				)?)?)
			}
			"--quote-idents" => {
				args.overrides.quoting =
					Some(config::parse_quoting(&value(&mut argv, "--quote-idents")?)?)
			}
			"-h" | "--help" => return Ok(Invocation::Print(USAGE.to_string())),
			"-V" | "--version" => {
				return Ok(Invocation::Print(VERSION.to_string()));
			}
			flag if flag.starts_with('-') => {
				return Err(format!("unknown flag `{flag}`\n\n{USAGE}"));
			}
			path => args.paths.push(PathBuf::from(path)),
		}
	}
	if args.stdin_mode && !args.paths.is_empty() {
		return Err("--stdin cannot be combined with paths".to_string());
	}
	if !args.stdin_mode && args.paths.is_empty() {
		return Err(format!("no input files\n\n{USAGE}"));
	}
	Ok(Invocation::Run(Box::new(args)))
}

type ConfigCache = std::collections::HashMap<PathBuf, PartialOptions>;

/// Read and parse a config file, memoized on its path.
fn load_partial(
	config_path: &Path,
	cache: &mut ConfigCache,
) -> Result<PartialOptions, String> {
	if let Some(partial) = cache.get(config_path) {
		return Ok(partial.clone());
	}
	let text = std::fs::read_to_string(config_path)
		.map_err(|err| format!("{}: {err}", config_path.display()))?;
	let partial = config::parse_config(&text, config_path)?;
	cache.insert(config_path.to_path_buf(), partial.clone());
	Ok(partial)
}

/// Effective options for one file, plus where embedding should take its
/// indent character from.
struct Resolved {
	options: Options,
	indent: embed::Indent,
}

/// Resolve effective options for a file in `dir`: defaults, then the
/// nearest config file's top-level keys (unless --no-config), then that
/// file's `[<language>]` section for `host`, then explicit flags.
///
/// Embedded SQL normally copies the host file's own indent character.
/// An indent style named anywhere in that chain is a deliberate choice,
/// so it wins over the host file instead.
fn resolve_options(
	dir: &Path,
	host: Option<embed::Host>,
	args: &Args,
	cache: &mut ConfigCache,
) -> Result<Resolved, String> {
	let mut options = Options::default();
	let mut indent_set = args.overrides.indent_style.is_some();
	if !args.no_config
		&& let Some(config_path) = config::discover(dir)
	{
		let partial = load_partial(&config_path, cache)?;
		partial.apply(&mut options);
		indent_set |= partial.indent_style.is_some();
		if let Some(section) =
			host.map(config::language_key).and_then(|key| partial.for_language(key))
		{
			section.apply(&mut options);
			indent_set |= section.indent_style.is_some();
		}
	}
	args.overrides.apply(&mut options);
	let indent = if indent_set {
		embed::Indent::Configured
	} else {
		embed::Indent::FromHost
	};
	Ok(Resolved { options, indent })
}

/// The embed host for a path, by extension.
fn host_for(path: &Path) -> Option<embed::Host> {
	match path.extension()?.to_str()? {
		"rs" => Some(embed::Host::Rust),
		"go" => Some(embed::Host::Go),
		"py" => Some(embed::Host::Python),
		"js" | "mjs" | "cjs" | "jsx" => Some(embed::Host::JavaScript),
		"ts" | "mts" | "cts" => Some(embed::Host::TypeScript),
		"tsx" => Some(embed::Host::Tsx),
		"gleam" => Some(embed::Host::Gleam),
		_ => None,
	}
}

/// Default extraction query for a host.
fn default_query(host: embed::Host) -> &'static str {
	match host {
		embed::Host::Rust => embed::RUST_SQLX_QUERY,
		embed::Host::Go => embed::GO_DB_QUERY,
		embed::Host::Python => embed::PYTHON_DB_QUERY,
		embed::Host::JavaScript | embed::Host::TypeScript | embed::Host::Tsx => {
			embed::JS_SQL_QUERY
		}
		embed::Host::Gleam => embed::GLEAM_SQL_QUERY,
	}
}

/// `, N frozen` when any were skipped, and nothing at all when none
/// were — the note should only appear when it explains something.
fn frozen_note(count: usize) -> String {
	match count {
		0 => String::new(),
		n => format!(", {n} frozen"),
	}
}

/// Remove files that a `frozen` glob claims and the baseline ref already
/// carries, returning how many were dropped.
///
/// Patterns come from the nearest config for each file (anchored at that
/// config) and from `--frozen` (anchored at the working directory), so a
/// file is judged by the config that governs it. The baseline is read
/// once per repository, only when something actually matches.
fn drop_frozen(
	files: &mut Vec<PathBuf>,
	args: &Args,
	cwd: &Path,
	cache: &mut ConfigCache,
) -> Result<usize, String> {
	let flag_globs = if args.frozen.is_empty() {
		None
	} else {
		Some(build_ignore_set(&args.frozen, cwd)?)
	};
	if flag_globs.is_none() && args.no_config {
		return Ok(0);
	}

	// Lazily built, since most runs configure no frozen paths at all.
	let mut config_globs: std::collections::HashMap<PathBuf, Option<IgnoreSet>> =
		std::collections::HashMap::new();
	let mut baselines: std::collections::HashMap<PathBuf, frozen::Baseline> =
		std::collections::HashMap::new();

	let mut dropped = 0;
	let mut kept = Vec::with_capacity(files.len());
	for path in std::mem::take(files) {
		let dir = path.parent().unwrap_or_else(|| Path::new("."));
		let mut claimed = flag_globs.as_ref().is_some_and(|globs| {
			globs.matches(std::path::absolute(&path).ok().as_deref(), &path)
		});
		let mut frozen_ref = args.frozen_ref.clone();
		let mut frozen_fetch = args.frozen_fetch;

		if !args.no_config
			&& let Some(config_path) = config::discover(dir)
		{
			let partial = load_partial(&config_path, cache)?;
			if frozen_ref.is_none() {
				frozen_ref = partial.frozen_ref.clone();
			}
			frozen_fetch |= partial.frozen_fetch.unwrap_or(false);
			let globs = match config_globs.get(&config_path) {
				Some(globs) => globs,
				None => {
					let built = if partial.frozen.is_empty() {
						None
					} else {
						Some(build_ignore_set(
							&partial.frozen,
							config::anchor_dir(&config_path),
						)?)
					};
					config_globs.entry(config_path.clone()).or_insert(built)
				}
			};
			claimed |= globs.as_ref().is_some_and(|globs| {
				globs.matches(std::path::absolute(&path).ok().as_deref(), &path)
			});
		}

		if !claimed {
			kept.push(path);
			continue;
		}

		let Some(root) = frozen::repo_root(dir) else {
			return Err(format!(
				"{} matches a `frozen` pattern but is not in a git repository, so \
				 there is no baseline to compare against",
				path.display()
			));
		};
		let baseline = match baselines.get(&root) {
			Some(baseline) => baseline,
			None => {
				let loaded = frozen::load(&root, frozen_ref.as_deref(), frozen_fetch)?;
				baselines.entry(root.clone()).or_insert(loaded)
			}
		};
		if baseline.contains(&path) {
			dropped += 1;
		} else {
			kept.push(path);
		}
	}
	*files = kept;
	Ok(dropped)
}

/// One formatted source, with human-readable diagnostics.
struct Outcome {
	formatted: String,
	/// `line:col: message` diagnostics (ErrorStatements + verbatim
	/// fallbacks).
	diagnostics: Vec<String>,
}

fn format_source(source: &str, options: &Options) -> Outcome {
	let tokens =
		parser::lexer::lex_with(source, options.dialect, options.lex_options());
	let parse = parser::parser::parse(&tokens, options.dialect);
	let result = formatter::format_cst(&parse.cst, options);
	let mut diagnostics: Vec<String> = parse
		.diagnostics
		.iter()
		.map(|d| {
			let (line, col) = line_col(source, d.start);
			format!("{line}:{col}: {} (statement passed through verbatim)", d.message)
		})
		.collect();
	if result.fallback_statements > 0 {
		diagnostics.push(format!(
			"{} statement(s) passed through verbatim (formatter self-check)",
			result.fallback_statements
		));
	}
	Outcome { formatted: result.text, diagnostics }
}

/// 1-based line and column for a byte offset.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
	let offset = offset.min(source.len());
	let before = &source[..offset];
	let line = before.matches('\n').count() + 1;
	let col = before.chars().rev().take_while(|&c| c != '\n').count() + 1;
	(line, col)
}

/// Ignore patterns compiled to globs, anchored to a directory: paths
/// are matched relative to `anchor`.
#[derive(Clone)]
struct IgnoreSet {
	set: globset::GlobSet,
	anchor: PathBuf,
}

impl IgnoreSet {
	fn matches(&self, absolute: Option<&Path>, walk_relative: &Path) -> bool {
		let relative = absolute
			.and_then(|path| path.strip_prefix(&self.anchor).ok())
			.unwrap_or(walk_relative);
		self.set.is_match(relative)
	}
}

/// Compile ignore patterns. Each pattern also matches anywhere below
/// the anchor (`**/pat`) and prunes whole directories (`pat/**`),
/// gitignore-style.
fn build_ignore_set(
	patterns: &[String],
	anchor: &Path,
) -> Result<IgnoreSet, String> {
	let mut builder = globset::GlobSetBuilder::new();
	for pattern in patterns {
		let base = pattern.trim_end_matches('/');
		if base.is_empty() {
			return Err(format!("invalid ignore pattern `{pattern}`"));
		}
		for variant in [
			base.to_string(),
			format!("{base}/**"),
			format!("**/{base}"),
			format!("**/{base}/**"),
		] {
			let glob = globset::GlobBuilder::new(&variant)
				.literal_separator(true)
				.build()
				.map_err(|err| format!("invalid ignore pattern `{pattern}`: {err}"))?;
			builder.add(glob);
		}
	}
	let set =
		builder.build().map_err(|err| format!("invalid ignore pattern: {err}"))?;
	Ok(IgnoreSet { set, anchor: anchor.to_path_buf() })
}

/// Recursively gather formattable files under the directory `root`.
/// The walker honors .gitignore and skips hidden entries; `ignores`
/// prunes squill's own patterns on top.
fn collect_files(
	root: &Path,
	embed: bool,
	ignores: Vec<IgnoreSet>,
	out: &mut Vec<PathBuf>,
) -> Result<(), String> {
	let root_buf = root.to_path_buf();
	let mut builder = ignore::WalkBuilder::new(root);
	builder.filter_entry(move |entry| {
		if entry.depth() == 0 {
			return true;
		}
		// Match anchor-relative, falling back to the walk-relative path
		// when the entry is outside an anchor (e.g. cwd-anchored
		// --ignore patterns while formatting a tree elsewhere).
		let walk_relative =
			entry.path().strip_prefix(&root_buf).unwrap_or(entry.path());
		let absolute = std::path::absolute(entry.path()).ok();
		!ignores.iter().any(|set| set.matches(absolute.as_deref(), walk_relative))
	});
	for entry in builder.build() {
		let entry = entry.map_err(|err| format!("{}: {err}", root.display()))?;
		if !entry.file_type().is_some_and(|kind| kind.is_file()) {
			continue;
		}
		let path = entry.into_path();
		if path.extension().is_some_and(|ext| ext == "sql")
			|| (embed && host_for(&path).is_some())
		{
			out.push(path);
		}
	}
	Ok(())
}

/// Above this many (estimated) changed lines, a fine-grained diff is
/// neither readable nor cheap — Myers is O(lines × edits) and goes
/// quadratic on a first format of a large file.
const FINE_DIFF_EDIT_LIMIT: usize = 1000;

fn print_diff(path: &str, before: &str, after: &str) {
	println!("--- {path}");
	println!("+++ {path} (formatted)");
	// The guard must be deterministic (a wall-clock deadline would make
	// --check output machine-dependent): estimate the edit volume from
	// line multisets in O(lines), and print a whole-file replacement
	// hunk when a fine diff is not worth computing.
	if estimated_edits(before, after) > FINE_DIFF_EDIT_LIMIT {
		let before_lines: Vec<&str> = before.lines().collect();
		let after_lines: Vec<&str> = after.lines().collect();
		println!("@@ -1,{} +1,{} @@", before_lines.len(), after_lines.len());
		for line in before_lines {
			println!("-{line}");
		}
		for line in after_lines {
			println!("+{line}");
		}
		return;
	}
	let diff = similar::TextDiff::from_lines(before, after);
	for hunk in diff.unified_diff().context_radius(2).iter_hunks() {
		print!("{hunk}");
	}
}

/// Lower bound on the diff's edit count: lines whose occurrence counts
/// differ between the two texts (order-insensitive, so cheap).
fn estimated_edits(before: &str, after: &str) -> usize {
	let mut counts: std::collections::HashMap<&str, isize> =
		std::collections::HashMap::new();
	for line in before.lines() {
		*counts.entry(line).or_default() += 1;
	}
	for line in after.lines() {
		*counts.entry(line).or_default() -= 1;
	}
	counts.values().map(|count| count.unsigned_abs()).sum()
}

fn main() -> ExitCode {
	let args = match parse_args() {
		Ok(Invocation::Run(args)) => *args,
		Ok(Invocation::Print(message)) => {
			println!("{message}");
			return ExitCode::SUCCESS;
		}
		Err(message) => {
			eprintln!("{message}");
			return ExitCode::from(2);
		}
	};

	if args.stdin_mode {
		let mut source = String::new();
		if let Err(err) = std::io::stdin().read_to_string(&mut source) {
			eprintln!("squill: cannot read stdin: {err}");
			return ExitCode::from(2);
		}
		let mut cache = std::collections::HashMap::new();
		let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
		// The stdin stream is always SQL, never a host file.
		let resolved = match resolve_options(&cwd, None, &args, &mut cache) {
			Ok(resolved) => resolved,
			Err(message) => {
				eprintln!("squill: {message}");
				return ExitCode::from(2);
			}
		};
		let outcome = format_source(&source, &resolved.options);
		for diagnostic in &outcome.diagnostics {
			eprintln!("<stdin>:{diagnostic}");
		}
		if args.check {
			if outcome.formatted != source {
				print_diff("<stdin>", &source, &outcome.formatted);
				return ExitCode::from(1);
			}
		} else {
			print!("{}", outcome.formatted);
		}
		if args.strict && !outcome.diagnostics.is_empty() {
			return ExitCode::from(1);
		}
		return ExitCode::SUCCESS;
	}

	let mut cache: ConfigCache = std::collections::HashMap::new();
	let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
	let cli_ignores = match build_ignore_set(&args.ignore, &cwd) {
		Ok(set) => set,
		Err(message) => {
			eprintln!("squill: {message}");
			return ExitCode::from(2);
		}
	};
	let mut files = Vec::new();
	for path in &args.paths {
		if !path.is_dir() {
			// Explicit file arguments always format, bypassing ignores.
			files.push(path.clone());
			continue;
		}
		let mut ignores = vec![cli_ignores.clone()];
		if !args.no_config
			&& let Some(config_path) = config::discover(path)
		{
			let partial = match load_partial(&config_path, &mut cache) {
				Ok(partial) => partial,
				Err(message) => {
					eprintln!("squill: {message}");
					return ExitCode::from(2);
				}
			};
			if !partial.ignore.is_empty() {
				let anchor = config::anchor_dir(&config_path);
				let anchor =
					std::path::absolute(anchor).unwrap_or_else(|_| anchor.to_path_buf());
				match build_ignore_set(&partial.ignore, &anchor) {
					Ok(set) => ignores.push(set),
					Err(message) => {
						eprintln!("squill: {}: {message}", config_path.display());
						return ExitCode::from(2);
					}
				}
			}
		}
		if let Err(message) = collect_files(path, args.embed, ignores, &mut files) {
			eprintln!("squill: {message}");
			return ExitCode::from(2);
		}
	}
	files.sort();
	files.dedup();

	// Drop files the baseline already carries. Before any formatting, so
	// a frozen file is never read, diffed, or counted.
	let frozen_count = match drop_frozen(&mut files, &args, &cwd, &mut cache) {
		Ok(count) => count,
		Err(message) => {
			eprintln!("squill: {message}");
			return ExitCode::from(2);
		}
	};

	struct FileResult {
		path: PathBuf,
		source: String,
		outcome: Outcome,
	}

	// Resolve options per file up front (sequential, cached per config
	// path); a config error is a hard error before any file is touched.
	let mut per_file_options = Vec::with_capacity(files.len());
	for path in &files {
		let dir = path.parent().unwrap_or_else(|| Path::new("."));
		match resolve_options(dir, host_for(path), &args, &mut cache) {
			Ok(resolved) => per_file_options.push(resolved),
			Err(message) => {
				eprintln!("squill: {message}");
				return ExitCode::from(2);
			}
		}
	}

	let results: Vec<Result<FileResult, String>> = files
		.par_iter()
		.zip(per_file_options.par_iter())
		.map(|(path, resolved)| {
			let source = std::fs::read_to_string(path)
				.map_err(|err| format!("{}: {err}", path.display()))?;
			let outcome = match host_for(path) {
				Some(host) => {
					// SQL embedded in a host-language file (sqlx macros,
					// database/sql calls) via the tree-sitter engine.
					let query =
						args.embed_query.as_deref().unwrap_or_else(|| default_query(host));
					let formatted = embed::format_embedded(
						&source,
						host,
						query,
						&resolved.options,
						resolved.indent,
					)
					.map_err(|err| format!("{}: {err}", path.display()))?;
					Outcome { formatted, diagnostics: Vec::new() }
				}
				None => format_source(&source, &resolved.options),
			};
			Ok(FileResult { path: path.clone(), source, outcome })
		})
		.collect();

	let mut changed = 0usize;
	let mut diagnostics = 0usize;
	let mut io_error = false;
	// Deterministic output: results arrive in input order.
	for result in results {
		let result = match result {
			Ok(result) => result,
			Err(message) => {
				eprintln!("squill: {message}");
				io_error = true;
				continue;
			}
		};
		let path = result.path.display().to_string();
		for diagnostic in &result.outcome.diagnostics {
			eprintln!("{path}:{diagnostic}");
			diagnostics += 1;
		}
		if result.outcome.formatted != result.source {
			changed += 1;
			if args.check {
				print_diff(&path, &result.source, &result.outcome.formatted);
			} else if args.stdout_mode {
				print!("{}", result.outcome.formatted);
			} else if let Err(err) =
				std::fs::write(&result.path, &result.outcome.formatted)
			{
				eprintln!("squill: {path}: {err}");
				io_error = true;
			} else {
				println!("{path}");
			}
		} else if args.stdout_mode {
			print!("{}", result.outcome.formatted);
		}
	}

	if args.check {
		eprintln!(
			"{} file(s) checked, {changed} would be reformatted, {diagnostics} diagnostic(s){}",
			files.len(),
			frozen_note(frozen_count)
		);
	} else if !args.stdout_mode {
		eprintln!(
			"{} file(s) checked, {changed} reformatted, {diagnostics} diagnostic(s){}",
			files.len(),
			frozen_note(frozen_count)
		);
	}

	if io_error {
		ExitCode::from(2)
	} else if (args.check && changed > 0) || (args.strict && diagnostics > 0) {
		ExitCode::from(1)
	} else {
		ExitCode::SUCCESS
	}
}
