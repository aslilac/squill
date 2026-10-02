//! squill: format SQL files.
//!
//! Thin CLI over the formatter library, suitable for pre-commit and CI.
//! `squill fmt <paths...>` writes in place; `--check` diffs and exits 1;
//! `--stdin`/`--stdout` stream. Options come from the nearest
//! squill.toml or squill.yaml (see `config`), overridden by explicit flags.

use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use std::collections::HashMap;
use std::sync::Arc;

use formatter::Options;
use rayon::prelude::*;

mod config;
mod frozen;
mod init;
#[cfg(feature = "lsp")]
mod lsp;
#[cfg(feature = "external-grammars")]
mod remote;
use config::PartialOptions;

/// `squill <version>` — whatever cargo compiled this binary with, so it
/// tracks the workspace manifest without a second place to bump.
const VERSION: &str = concat!("squill ", env!("CARGO_PKG_VERSION"));

#[cfg(feature = "lsp")]
const LANGUAGE_SERVER_USAGE: &str = "\
Usage: squill language-server start

Runs squill's language server on stdin/stdout, for editors: document
formatting, and diagnostics for what squill leaves alone.
";

const USAGE: &str = "\
squill — a SQL formatter

Usage: squill [fmt] [OPTIONS] [PATHS...]
       squill locate [--json] [OPTIONS] [PATHS...]
       squill init [--dialect <D>] [--yaml] [--yes]
       squill language-server start
       squill help [COMMAND]
       squill version

fmt is the default command: `squill .` formats the working directory,
and `squill -` formats stdin. (A file named like a command needs the
`fmt`, or a path: `squill ./init`.)

`squill language-server start` runs a language server on stdin/stdout,
for editors: document formatting, and diagnostics for what squill
leaves alone.

`squill locate` lists the SQL squill finds, without formatting it: each
string an [[embedded]] rule's query captures (formatted or not), and
each whole plain SQL file. Run it on a file to check what a custom
query matches. See `squill locate --help`.

`squill init` writes a starter squill.toml (or, with --yaml,
squill.yaml) for the project in the working directory: pick the
languages whose embedded SQL to format (the ones already holding SQL
come checked) and their dialects.

Formats the given files in place (directories are searched
recursively): .sql files, plus whatever the config's [[files]] and
[[embedded]] rules include. Reads stdin when --stdin is given.

Options:
  --check                 Don't write; print diffs and exit 1 if any file
                          would change
  --stdout                Print formatted output instead of writing files
  --stdin, -              Read SQL from stdin, write to stdout
  --stdin-filepath <PATH> Read stdin as the file at PATH (which need not
                          exist): config and rules resolve against it,
                          so a host file's embedded SQL formats too.
                          Ignored, frozen, and unconfigured host files
                          pass through unchanged. For editors.
  --strict                Exit 1 when anything was left unformatted:
                          statements that could not be parsed, embedded
                          strings squill could not rewrite safely
  --locked                Fail rather than record a grammar URL that
                          squill.lock doesn't have yet (for CI)
  --ignore <GLOB>         Skip matching paths when recursing directories
                          (repeatable; relative to the working directory;
                          `*`, `**`, `?`, `[abc]`, `{a,b}` globs)
  --dialect <D>           postgres (default) | sqlite
  --indent <STYLE>        tabs (default) | spaces
  --indent-width <N>      Indent width (and tab measure, 1 to 16),
                          default 2
  --max-width <N>         Target line width (20 to 500), default 80
  --keyword-case <CASE>   lower (default) | upper
  --quote-idents <MODE>   as-needed (default) | always
  --trailing-semicolons <MODE>
                          always | none: whether the last statement ends
                          in `;` (default always for SQL files, none for
                          embedded SQL)
  --line-ending <EOL>     lf (default) | crlf
  --at-params             Treat sqlc- and ADO.NET-style @name as
                          parameters (Postgres)
  --question-params       Treat JDBC-style ? as parameters (Postgres)
  --colon-params          Treat :name as parameters (Postgres)
  --pyformat-params       Treat Python DB-API %s / %(name)s as
                          parameters
  --no-config             Ignore config files
  --frozen <GLOB>         Treat matching paths as immutable once they
                          exist on the baseline ref: format them while
                          new, never rewrite them after (repeatable)
  --frozen-ref <REF>      Baseline ref for --frozen (default: whatever
                          the remote records as its HEAD)
  --no-frozen-fetch       Don't let --frozen ask the remote for its HEAD.
                          Needs a recorded remote HEAD or --frozen-ref
  -V, --version           Print the version and exit
  -h, --help              Show this help

Configuration: the nearest squill.toml or .config/squill.toml (or
squill.yaml / squill.yml, same keys) at or above each formatted file
supplies defaults. Top-level keys: dialect, indent, indent-width,
max-width, keyword-case, quote-idents, trailing-semicolons, line-ending,
at-params, question-params, colon-params, pyformat-params, ignore and
frozen (arrays of glob patterns), frozen-ref, and frozen-fetch. Explicit flags override the
config. The search upward stops at a git repository root, a mount
point, or a symlinked directory, so a config outside a checkout never
reaches inside it. Directory recursion honors .gitignore and skips
hidden files; explicitly listed files always format, given a rule
(or a .sql name) that says what they are.

Rules scope settings to paths. Every rule whose `include` matches a
file applies, later rules winning key by key; paths are relative to
the directory the config governs. A [[files]] rule covers plain SQL
(and brings in files not named *.sql):

    [[files]]
    include = [\"**/*.sql.sqlite\", \"migrations/sqlite/**\"]
    dialect = \"sqlite\"

An [[embedded]] rule formats SQL inside host-language files, found by
a tree-sitter grammar and query:

    [[embedded]]
    include = [\"**/*.rs\"]
    grammar = \"rust\"
    query = \".config/squill/rust.scm\"   # optional for built-ins
    dialect = \"sqlite\"

Built-in grammars: rust, go, python, javascript, typescript, tsx,
gleam, c++, c#, java, kotlin, swift — each with a default query.
Any other language works with a grammar built by `tree-sitter build
--wasm` (grammar = \"grammars/tree-sitter-lua.wasm\", or an https
URL, whose SHA-256 is locked in squill.lock) and a query. Queries
capture the SQL string as @sql (or @sql.postgres / @sql.sqlite to
fix its dialect); only #eq?, #not-eq?, and #any-of? predicates are
supported.

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

const LOCATE_USAGE: &str = "\
Usage: squill locate [OPTIONS] [PATHS...]

Lists the SQL squill finds in the given files (directories are searched
as `squill fmt` searches them), without formatting anything: every
string an [[embedded]] rule's query captures, whether or not squill
would rewrite it, and every plain SQL file whole. One line each:

    src/db.rs:12:9-17:9 postgres  select m.id, m.name…

giving where the string's contents start and end (lines and columns,
1-based, end exclusive), and the dialect they're read in — marked
\"(from the query)\" when a capture like @sql.sqlite set it, over the
configured dialect.

Options:
  --json                  One JSON object per line instead: path, start
                          and end (byte offsets, end exclusive), line,
                          column, end_line, end_column, dialect, and
                          pinned_dialect (the query set the dialect)
  --stdin-filepath <PATH> Read stdin as the file at PATH
  --locked, --no-config, --ignore <GLOB>, --dialect <D>, --at-params,
  --question-params, --colon-params, --pyformat-params
                          As for `squill fmt`
  -h, --help              Show this help
";

#[derive(Default)]
struct Args {
	/// `squill locate`: list where the SQL is instead of formatting it.
	locate: bool,
	/// `squill locate --json`.
	json: bool,
	check: bool,
	stdout_mode: bool,
	stdin_mode: bool,
	/// `--stdin-filepath`: the path the stdin stream stands for, which
	/// config and rules resolve against.
	stdin_path: Option<PathBuf>,
	/// `--locked`: a grammar URL missing from squill.lock is an error,
	/// not a new entry.
	locked: bool,
	strict: bool,
	no_config: bool,
	frozen: Vec<String>,
	frozen_ref: Option<String>,
	frozen_fetch: Option<bool>,
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
	Init(init::InitArgs),
	#[cfg(feature = "lsp")]
	Lsp,
	Print(String),
}

/// The usage text for `squill help <command>` and `<command> --help`.
fn command_usage(command: &str) -> Option<&'static str> {
	match command {
		"fmt" | "help" | "version" => Some(USAGE),
		"locate" => Some(LOCATE_USAGE),
		"init" => Some(init::USAGE),
		#[cfg(feature = "lsp")]
		"language-server" => Some(LANGUAGE_SERVER_USAGE),
		_ => None,
	}
}

/// Could this first argument be meant as a path rather than a command?
/// Anything path-like is; a bare word only when there's a file by that
/// name, so a mistyped command (`squill fmtt`) is reported as one.
fn is_path_like(arg: &str) -> bool {
	arg.starts_with('-')
		|| arg.contains(['/', '.', std::path::MAIN_SEPARATOR])
		|| std::path::Path::new(arg).exists()
}

fn parse_args() -> Result<Invocation, String> {
	let mut argv = std::env::args().skip(1).peekable();
	let mut locate = false;
	// `fmt` is the default command: `squill .`, `squill -`, and
	// `squill --check src/` all format, and leave their first argument for
	// fmt to read.
	let first = argv.peek().cloned();
	let command = match first.as_deref() {
		Some(
			"fmt" | "locate" | "language-server" | "init" | "help" | "version" | "-h"
			| "--help" | "-V" | "--version",
		)
		| None => argv.next(),
		Some(other) if is_path_like(other) => Some("fmt".to_string()),
		Some(other) => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
	};
	match command.as_deref() {
		Some("fmt") => {}
		Some("locate") => locate = true,
		Some("help") => {
			return match (argv.next(), argv.next()) {
				(None, _) => Ok(Invocation::Print(USAGE.to_string())),
				(Some(command), None) => command_usage(&command)
					.map(|usage| Invocation::Print(usage.to_string()))
					.ok_or_else(|| format!("unknown command `{command}`\n\n{USAGE}")),
				(Some(_), Some(extra)) => {
					Err(format!("unknown argument `{extra}`\n\n{USAGE}"))
				}
			};
		}
		Some("version") => {
			return match argv.next() {
				None => Ok(Invocation::Print(VERSION.to_string())),
				Some(extra) => Err(format!("unknown argument `{extra}`\n\n{USAGE}")),
			};
		}
		#[cfg(feature = "lsp")]
		Some("language-server") => {
			return match (argv.next().as_deref(), argv.next()) {
				(Some("start"), None) => Ok(Invocation::Lsp),
				(Some("-h" | "--help"), None) => {
					Ok(Invocation::Print(LANGUAGE_SERVER_USAGE.to_string()))
				}
				(None, _) => Err(LANGUAGE_SERVER_USAGE.to_string()),
				(Some("start"), Some(extra)) => {
					Err(format!("unknown argument `{extra}`\n\n{LANGUAGE_SERVER_USAGE}"))
				}
				(Some(other), _) => Err(format!(
					"unknown language-server command `{other}`\n\n{LANGUAGE_SERVER_USAGE}"
				)),
			};
		}
		Some("init") => {
			if argv.peek().is_some_and(|arg| arg == "-h" || arg == "--help") {
				return Ok(Invocation::Print(init::USAGE.to_string()));
			}
			return init::parse(argv).map(Invocation::Init);
		}
		Some("-h" | "--help") => return Ok(Invocation::Print(USAGE.to_string())),
		Some("-V" | "--version") => {
			return Ok(Invocation::Print(VERSION.to_string()));
		}
		// A bare `squill` names no command: usage, but as a complaint.
		None => return Err(USAGE.to_string()),
		Some(other) => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
	}
	let usage = if locate { LOCATE_USAGE } else { USAGE };
	let mut args = Args {
		locate,
		json: false,
		check: false,
		stdout_mode: false,
		stdin_mode: false,
		stdin_path: None,
		locked: false,
		strict: false,
		no_config: false,
		frozen: Vec::new(),
		frozen_ref: None,
		frozen_fetch: None,
		ignore: Vec::new(),
		overrides: PartialOptions::default(),
		paths: Vec::new(),
	};
	let value = |argv: &mut dyn Iterator<Item = String>, flag: &str| {
		argv.next().ok_or_else(|| format!("{flag} needs a value"))
	};
	while let Some(arg) = argv.next() {
		match arg.as_str() {
			"--json" if locate => args.json = true,
			"--check" | "--stdout" | "--strict" if locate => {
				return Err(format!("`squill locate` doesn't take {arg}"));
			}
			"--check" => args.check = true,
			"--stdout" => args.stdout_mode = true,
			// `-` as a path is stdin, by the usual convention.
			"--stdin" | "-" => args.stdin_mode = true,
			"--stdin-filepath" => {
				args.stdin_mode = true;
				args.stdin_path =
					Some(PathBuf::from(value(&mut argv, "--stdin-filepath")?));
			}
			"--strict" => args.strict = true,
			"--locked" => args.locked = true,
			"--no-config" => args.no_config = true,
			"--ignore" => args.ignore.push(value(&mut argv, "--ignore")?),
			"--frozen" => args.frozen.push(value(&mut argv, "--frozen")?),
			"--frozen-ref" => {
				args.frozen_ref = Some(value(&mut argv, "--frozen-ref")?)
			}
			"--frozen-fetch" => args.frozen_fetch = Some(true),
			"--no-frozen-fetch" => args.frozen_fetch = Some(false),
			"--at-params" => args.overrides.at_params = Some(true),
			"--question-params" => args.overrides.question_params = Some(true),
			"--colon-params" => args.overrides.colon_params = Some(true),
			"--pyformat-params" => args.overrides.pyformat_params = Some(true),
			"--dialect" => {
				args.overrides.dialect =
					Some(config::parse_dialect(&value(&mut argv, "--dialect")?)?)
			}
			"--indent" => {
				args.overrides.indent_style =
					Some(config::parse_indent(&value(&mut argv, "--indent")?)?)
			}
			"--indent-width" => {
				let width: u8 = value(&mut argv, "--indent-width")?
					.parse()
					.map_err(|_| "--indent-width needs a number".to_string())?;
				if !(1..=16).contains(&width) {
					return Err("--indent-width expects 1 to 16".to_string());
				}
				args.overrides.indent_width = Some(width);
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
			"--trailing-semicolons" => {
				args.overrides.trailing_semicolons =
					Some(config::parse_trailing_semicolons(&value(
						&mut argv,
						"--trailing-semicolons",
					)?)?)
			}
			"--line-ending" => {
				args.overrides.line_ending =
					Some(config::parse_line_ending(&value(&mut argv, "--line-ending")?)?)
			}
			"--quote-idents" => {
				args.overrides.quoting =
					Some(config::parse_quoting(&value(&mut argv, "--quote-idents")?)?)
			}
			"-h" | "--help" => return Ok(Invocation::Print(usage.to_string())),
			"-V" | "--version" => {
				return Ok(Invocation::Print(VERSION.to_string()));
			}
			flag if flag.starts_with('-') => {
				return Err(format!("unknown flag `{flag}`\n\n{usage}"));
			}
			path => args.paths.push(PathBuf::from(path)),
		}
	}
	if args.stdin_mode && !args.paths.is_empty() {
		return Err(
			"--stdin (or `-`, or --stdin-filepath) cannot be combined with paths"
				.to_string(),
		);
	}
	if !args.stdin_mode && args.paths.is_empty() {
		return Err(format!("no input files\n\n{usage}"));
	}
	Ok(Invocation::Run(Box::new(args)))
}

/// A parsed config with its rule globs compiled.
struct Loaded {
	/// Where it was read from: `squill.lock` sits beside it.
	path: PathBuf,
	config: config::Config,
	/// One matcher per `[[files]]` rule, in order.
	files: Vec<IgnoreSet>,
	/// One matcher per `[[embedded]]` rule, in order.
	embedded: Vec<IgnoreSet>,
}

/// Everything resolution reads from disk, memoized: configs by path,
/// the nearest config by directory, grammars and queries by path.
#[derive(Default)]
struct Caches {
	configs: HashMap<PathBuf, Arc<Loaded>>,
	nearest: HashMap<PathBuf, Result<Option<PathBuf>, String>>,
	/// Loaded wasm grammars, by path or URL.
	#[cfg(feature = "external-grammars")]
	grammars: HashMap<String, Arc<embed::Grammar>>,
	queries: HashMap<PathBuf, Arc<str>>,
}

impl Caches {
	/// Drop everything read from config files (and the queries they
	/// name), keeping loaded grammars: the language server re-reads
	/// config for every request, but a wasm grammar is costly to load.
	#[cfg_attr(not(feature = "lsp"), allow(dead_code))]
	fn forget_config(&mut self) {
		self.configs.clear();
		self.nearest.clear();
		self.queries.clear();
	}

	/// The nearest config at or above `dir`.
	fn discover(&mut self, dir: &Path) -> Result<Option<PathBuf>, String> {
		self
			.nearest
			.entry(dir.to_path_buf())
			.or_insert_with(|| config::discover(dir))
			.clone()
	}

	/// Read, parse, and compile a config file.
	fn load(&mut self, config_path: &Path) -> Result<Arc<Loaded>, String> {
		if let Some(loaded) = self.configs.get(config_path) {
			return Ok(loaded.clone());
		}
		let text = std::fs::read_to_string(config_path)
			.map_err(|err| format!("{}: {err}", config_path.display()))?;
		let config = config::parse_config(&text, config_path)?;
		let anchor = config::anchor_dir(config_path);
		let anchor =
			std::path::absolute(anchor).unwrap_or_else(|_| anchor.to_path_buf());
		let compile = |include: &[String]| {
			build_ignore_set(include, &anchor)
				.map_err(|message| format!("{}: {message}", config_path.display()))
		};
		let files = config
			.files
			.iter()
			.map(|rule| compile(&rule.include))
			.collect::<Result<_, _>>()?;
		let embedded = config
			.embedded
			.iter()
			.map(|rule| compile(&rule.include))
			.collect::<Result<_, _>>()?;
		let loaded = Arc::new(Loaded {
			path: config_path.to_path_buf(),
			config,
			files,
			embedded,
		});
		self.configs.insert(config_path.to_path_buf(), loaded.clone());
		Ok(loaded)
	}

	/// The config governing a file in `dir`, unless --no-config.
	fn governing(
		&mut self,
		dir: &Path,
		args: &Args,
	) -> Result<Option<Arc<Loaded>>, String> {
		if args.no_config {
			return Ok(None);
		}
		match self.discover(dir)? {
			Some(config_path) => self.load(&config_path).map(Some),
			None => Ok(None),
		}
	}

	// With no built-in grammars compiled in, `Builtin` cannot occur.
	#[cfg_attr(
		not(any(
			feature = "rust",
			feature = "go",
			feature = "python",
			feature = "javascript",
			feature = "typescript",
			feature = "gleam",
			feature = "cxx",
			feature = "csharp",
			feature = "java",
			feature = "kotlin",
			feature = "swift"
		)),
		allow(unreachable_code)
	)]
	/// The grammar a rule names. A URL grammar is checked against (or
	/// recorded in) the `squill.lock` beside `config_path`.
	#[cfg_attr(not(feature = "external-grammars"), allow(unused_variables))]
	fn grammar(
		&mut self,
		spec: &config::GrammarSpec,
		config_path: &Path,
		locked: bool,
	) -> Result<Arc<embed::Grammar>, String> {
		match spec {
			config::GrammarSpec::Builtin(host) => Ok(Arc::new((*host).into())),
			#[cfg(feature = "external-grammars")]
			config::GrammarSpec::Wasm(path) => {
				let key = path.display().to_string();
				if let Some(grammar) = self.grammars.get(&key) {
					return Ok(grammar.clone());
				}
				let grammar = embed::wasm::WasmGrammar::load(path)
					.map_err(|err| err.to_string())?;
				let grammar = Arc::new(embed::Grammar::Wasm(grammar));
				self.grammars.insert(key, grammar.clone());
				Ok(grammar)
			}
			#[cfg(feature = "external-grammars")]
			config::GrammarSpec::Url(url) => {
				if let Some(grammar) = self.grammars.get(url) {
					return Ok(grammar.clone());
				}
				let lockfile = config_path.with_file_name("squill.lock");
				let bytes = remote::grammar_bytes(url, &lockfile, locked)?;
				let name = config::url_file_name(url)
					.and_then(embed::wasm::language_name)
					.ok_or_else(|| format!("{url}: cannot name a grammar from it"))?;
				let grammar =
					embed::wasm::WasmGrammar::from_bytes(name, url.clone(), bytes)
						.map_err(|err| err.to_string())?;
				let grammar = Arc::new(embed::Grammar::Wasm(grammar));
				self.grammars.insert(url.clone(), grammar.clone());
				Ok(grammar)
			}
			#[cfg(not(feature = "external-grammars"))]
			config::GrammarSpec::Wasm(_) | config::GrammarSpec::Url(_) => {
				Err("this squill was built without wasm grammar support".to_string())
			}
		}
	}

	fn query(&mut self, path: &Path) -> Result<Arc<str>, String> {
		if let Some(query) = self.queries.get(path) {
			return Ok(query.clone());
		}
		let query: Arc<str> = std::fs::read_to_string(path)
			.map_err(|err| format!("query {}: {err}", path.display()))?
			.into();
		self.queries.insert(path.to_path_buf(), query.clone());
		Ok(query)
	}
}

/// What a file is, as far as formatting goes.
enum Kind {
	Sql,
	/// A host-language file, and how to find its SQL.
	Embedded {
		grammar: Arc<embed::Grammar>,
		query: Arc<str>,
	},
}

/// Effective options for one file, plus where embedding should take its
/// indent character from.
struct Resolved {
	options: Options,
	indent: embed::Indent,
	kind: Kind,
}

/// The directory a file is in. A bare file name's parent is the empty
/// path, which is the working directory, not a path that doesn't exist.
fn parent_dir(path: &Path) -> &Path {
	match path.parent() {
		Some(parent) if !parent.as_os_str().is_empty() => parent,
		_ => Path::new("."),
	}
}

/// Is this a plain SQL file by name alone?
fn is_sql_file(path: &Path) -> bool {
	path.extension().is_some_and(|ext| ext == "sql")
}

/// Resolve a file: whether squill formats it at all (`None` when
/// nothing claims it), as what, and with which options. Defaults, then
/// the nearest config's top-level keys, then every matching rule of the
/// file's kind in order, then explicit flags.
///
/// Embedded SQL normally copies the host file's own indent character.
/// An indent style named anywhere in that chain is a deliberate choice,
/// so it wins over the host file instead.
fn resolve(
	path: &Path,
	args: &Args,
	caches: &mut Caches,
) -> Result<Option<Resolved>, String> {
	let dir = parent_dir(path);
	let mut options = Options::default();
	let mut indent = embed::Indent {
		configured_style: args.overrides.indent_style.is_some(),
		configured_width: args.overrides.indent_width.is_some(),
	};
	let mut kind = None;
	if let Some(loaded) = caches.governing(dir, args)? {
		let config = &loaded.config;
		config.options.apply(&mut options);
		indent.configured_style |= config.options.indent_style.is_some();
		indent.configured_width |= config.options.indent_width.is_some();
		let absolute = std::path::absolute(path).ok();
		let matches = |set: &IgnoreSet| set.matches(absolute.as_deref(), path);
		let embedded: Vec<&config::EmbeddedRule> = config
			.embedded
			.iter()
			.zip(&loaded.embedded)
			.filter(|(_, set)| matches(set))
			.map(|(rule, _)| rule)
			.collect();
		let files: Vec<&config::FileRule> = config
			.files
			.iter()
			.zip(&loaded.files)
			.filter(|(_, set)| matches(set))
			.map(|(rule, _)| rule)
			.collect();
		if let Some(spec) =
			embedded.iter().rev().find_map(|rule| rule.grammar.as_ref())
		{
			let grammar = caches.grammar(spec, &loaded.path, args.locked)?;
			let query =
				match embedded.iter().rev().find_map(|rule| rule.query.as_ref()) {
					Some(query_path) => caches.query(query_path)?,
					None => match spec {
						config::GrammarSpec::Builtin(host) => host.default_query().into(),
						config::GrammarSpec::Wasm(_) | config::GrammarSpec::Url(_) => {
							let wasm = match spec {
								config::GrammarSpec::Wasm(wasm) => wasm.display().to_string(),
								config::GrammarSpec::Url(url) => url.clone(),
								config::GrammarSpec::Builtin(_) => unreachable!(),
							};
							return Err(format!(
								"{}: the wasm grammar {wasm} has no built-in query; give \
								 its [[embedded]] rule a `query`",
								path.display(),
							));
						}
					},
				};
			// Embedded SQL goes without a final `;` unless configured.
			let mut semicolons_set = config.options.trailing_semicolons.is_some()
				|| args.overrides.trailing_semicolons.is_some();
			for rule in &embedded {
				rule.options.apply(&mut options);
				indent.configured_style |= rule.options.indent_style.is_some();
				indent.configured_width |= rule.options.indent_width.is_some();
				semicolons_set |= rule.options.trailing_semicolons.is_some();
			}
			if !semicolons_set {
				options.trailing_semicolons = formatter::TrailingSemicolons::None;
			}
			kind = Some(Kind::Embedded { grammar, query });
		} else if is_sql_file(path) || !files.is_empty() {
			for rule in &files {
				rule.options.apply(&mut options);
				indent.configured_style |= rule.options.indent_style.is_some();
				indent.configured_width |= rule.options.indent_width.is_some();
			}
			kind = Some(Kind::Sql);
		}
	} else if is_sql_file(path) {
		kind = Some(Kind::Sql);
	}
	let Some(kind) = kind else {
		return Ok(None);
	};
	args.overrides.apply(&mut options);
	Ok(Some(Resolved { options, indent, kind }))
}

/// Why a file named on the command line doesn't format: it isn't
/// `*.sql`, and no rule covers it. Says which rule would, guessing the
/// language from the extension the way `squill init` does.
fn unclaimed(path: &Path, args: &Args, caches: &mut Caches) -> String {
	let config = if args.no_config {
		None
	} else {
		match caches.discover(parent_dir(path)) {
			Ok(config) => config,
			Err(message) => return message,
		}
	};
	let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
	let glob = match path.extension().and_then(|ext| ext.to_str()) {
		Some(extension) => format!("**/*.{extension}"),
		None => format!("**/{name}"),
	};
	// What formatting it would mean, the rule that would say so, and
	// whether `squill init` writes that rule.
	let (kind, what, rule, wizard) = match init::host_for(path) {
		Some((host, _)) => (
			"[[embedded]]",
			format!("the SQL embedded in this {} file", init::display_name(host)),
			format!(
				"[[embedded]]\n    include = [\"{glob}\"]\n    grammar = \"{}\"",
				host.name()
			),
			true,
		),
		None => (
			"[[files]]",
			"it as SQL".to_string(),
			format!("[[files]]\n    include = [\"{glob}\"]"),
			false,
		),
	};
	let path = path.display();
	match config {
		None if args.no_config => format!(
			"{path}: not a .sql file; formatting {what} needs a {kind} rule, and \
			 --no-config ignores them"
		),
		None if wizard => format!(
			"{path}: not a .sql file, and no squill config covers it; to format \
			 {what}, run `squill init`, or write a squill.toml with a rule:\n\n    {rule}"
		),
		None => format!(
			"{path}: not a .sql file, and no squill config covers it; to format \
			 {what}, write a squill.toml with a rule:\n\n    {rule}"
		),
		Some(config) => format!(
			"{path}: not a .sql file, and no rule covers it; to format {what}, \
			 add one to {}:\n\n    {rule}",
			config.display()
		),
	}
}

/// Resolve the stdin stream as the file at `path` (`--stdin-filepath`).
/// `None` means hand it back untouched: the path is ignored or frozen,
/// or it's neither `*.sql` nor covered by a rule. The file itself need
/// not exist, so an unsaved editor buffer works. `known_sql` is for a
/// caller that knows the text is SQL whatever its name (the language
/// server, for a document its client calls SQL), which makes a file no
/// rule covers SQL too.
fn stdin_as(
	path: &Path,
	args: &Args,
	cwd: &Path,
	caches: &mut Caches,
	known_sql: bool,
) -> Result<Option<Resolved>, String> {
	let dir = parent_dir(path);
	let absolute = std::path::absolute(path).ok();
	let mut ignores = vec![build_ignore_set(&args.ignore, cwd)?];
	if !args.no_config
		&& let Some(config_path) = caches.discover(dir)?
	{
		let loaded = caches.load(&config_path)?;
		if !loaded.config.ignore.is_empty() {
			let anchor = config::anchor_dir(&config_path);
			let anchor =
				std::path::absolute(anchor).unwrap_or_else(|_| anchor.to_path_buf());
			ignores.push(build_ignore_set(&loaded.config.ignore, &anchor)?);
		}
	}
	let relative = path.strip_prefix(cwd).unwrap_or(path);
	if ignores.iter().any(|set| set.matches(absolute.as_deref(), relative)) {
		return Ok(None);
	}
	let mut files = vec![path.to_path_buf()];
	if drop_frozen(&mut files, args, cwd, caches)? > 0 {
		return Ok(None);
	}
	if let Some(resolved) = resolve(path, args, caches)? {
		return Ok(Some(resolved));
	}
	if !known_sql {
		return Ok(None);
	}
	let mut resolved = resolve_sql_defaults(dir, args, caches)?;
	resolved.kind = Kind::Sql;
	Ok(Some(resolved))
}

/// Options for SQL no rule covers (stdin, or an editor's SQL document):
/// defaults, top-level keys, flags.
fn resolve_sql_defaults(
	dir: &Path,
	args: &Args,
	caches: &mut Caches,
) -> Result<Resolved, String> {
	let mut options = Options::default();
	if let Some(loaded) = caches.governing(dir, args)? {
		loaded.config.options.apply(&mut options);
	}
	args.overrides.apply(&mut options);
	Ok(Resolved { options, indent: embed::Indent::CONFIGURED, kind: Kind::Sql })
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
	caches: &mut Caches,
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
		let dir = parent_dir(&path);
		let mut claimed = flag_globs.as_ref().is_some_and(|globs| {
			globs.matches(std::path::absolute(&path).ok().as_deref(), &path)
		});
		let mut frozen_ref = args.frozen_ref.clone();
		// Flag, then config, then the default: fetch when we have to.
		let mut frozen_fetch = args.frozen_fetch;

		if !args.no_config
			&& let Some(config_path) = caches.discover(dir)?
		{
			let loaded = caches.load(&config_path)?;
			let partial = &loaded.config;
			if frozen_ref.is_none() {
				frozen_ref = partial.frozen_ref.clone();
			}
			frozen_fetch = frozen_fetch.or(partial.frozen_fetch);
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
				let loaded = frozen::load(
					&root,
					frozen_ref.as_deref(),
					frozen_fetch.unwrap_or(true),
				)?;
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

/// One formatted source, with its diagnostics.
struct Outcome {
	formatted: String,
	/// Statements passed through verbatim, embedded strings left alone.
	diagnostics: Vec<Diagnostic>,
}

/// Something squill left unformatted, and where.
struct Diagnostic {
	/// Byte range in the source; `None` when it concerns the whole file.
	range: Option<std::ops::Range<usize>>,
	message: String,
}

impl Diagnostic {
	/// `line:col: message`, or ` message` without a position, to follow
	/// `path:`.
	fn render(&self, source: &str) -> String {
		match &self.range {
			Some(range) => {
				let (line, col) = line_col(source, range.start);
				format!("{line}:{col}: {}", self.message)
			}
			None => format!(" {}", self.message),
		}
	}
}

fn format_source(source: &str, options: &Options) -> Outcome {
	let tokens =
		parser::lexer::lex_with(source, options.dialect, options.lex_options());
	let parse = parser::parser::parse(&tokens, options.dialect);
	let result = formatter::format_cst(&parse.cst, options);
	let mut diagnostics: Vec<Diagnostic> = parse
		.diagnostics
		.iter()
		.map(|d| Diagnostic {
			range: Some(d.start..d.end),
			message: format!("{} (statement passed through verbatim)", d.message),
		})
		.collect();
	diagnostics.extend(result.body_diagnostics.into_iter().map(|body| {
		Diagnostic { range: Some(body.start..body.end), message: body.message }
	}));
	diagnostics.sort_by_key(|diagnostic| {
		diagnostic.range.as_ref().map_or(usize::MAX, |range| range.start)
	});
	if result.fallback_statements > 0 {
		diagnostics.push(Diagnostic {
			range: None,
			message: format!(
				"{} statement(s) passed through verbatim (formatter self-check)",
				result.fallback_statements
			),
		});
	}
	Outcome { formatted: result.text, diagnostics }
}

/// Where the SQL in one source is, the way its resolution reads it: every
/// string a host file's query captures, or a SQL file whole.
fn locate_resolved(
	source: &str,
	resolved: &Resolved,
) -> Result<Vec<embed::Located>, String> {
	match &resolved.kind {
		Kind::Embedded { grammar, query } => {
			embed::locate_sql(source, grammar, query, resolved.options.dialect)
				.map_err(|err| err.to_string())
		}
		Kind::Sql => Ok(vec![embed::Located {
			range: 0..source.len(),
			dialect: resolved.options.dialect,
			pinned_dialect: false,
		}]),
	}
}

/// Print `squill locate`'s lines for one source.
fn print_located(
	label: &str,
	source: &str,
	found: &[embed::Located],
	json: bool,
) {
	for located in found {
		let (line, column) = line_col(source, located.range.start);
		let (end_line, end_column) = line_col(source, located.range.end);
		let dialect = match located.dialect {
			parser::Dialect::Postgres => "postgres",
			parser::Dialect::Sqlite => "sqlite",
		};
		if json {
			println!(
				"{{\"path\":{},\"start\":{},\"end\":{},\"line\":{line},\"column\":{column},\"end_line\":{end_line},\"end_column\":{end_column},\"dialect\":\"{dialect}\",\"pinned_dialect\":{}}}",
				json_string(label),
				located.range.start,
				located.range.end,
				located.pinned_dialect,
			);
		} else {
			let text = source[located.range.clone()].trim();
			let first = text.lines().next().unwrap_or("");
			let more = if first.len() < text.len() { "…" } else { "" };
			let pinned =
				if located.pinned_dialect { " (from the query)" } else { "" };
			println!(
				"{label}:{line}:{column}-{end_line}:{end_column} {dialect}{pinned}  {first}{more}"
			);
		}
	}
}

/// A JSON string literal.
fn json_string(text: &str) -> String {
	let mut out = String::from("\"");
	for c in text.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
			c => out.push(c),
		}
	}
	out.push('"');
	out
}

/// Format one source the way its resolution says: SQL, or a host file
/// whose embedded SQL the rule's grammar and query find.
fn format_resolved(
	source: &str,
	resolved: &Resolved,
) -> Result<Outcome, String> {
	match &resolved.kind {
		Kind::Embedded { grammar, query } => {
			let embedded = embed::format_embedded(
				source,
				grammar,
				query,
				&resolved.options,
				resolved.indent,
			)
			.map_err(|err| err.to_string())?;
			let diagnostics = embedded
				.warnings
				.into_iter()
				.map(|warning| Diagnostic {
					range: Some(warning.offset..warning.end),
					message: warning.message,
				})
				.collect();
			Ok(Outcome { formatted: embedded.text, diagnostics })
		}
		Kind::Sql => Ok(format_source(source, &resolved.options)),
	}
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

/// Recursively gather formattable files under the directory `root`:
/// every file [`resolve`] claims. The walker honors .gitignore and skips
/// hidden entries; `ignores` prunes squill's own patterns on top.
fn collect_files(
	root: &Path,
	ignores: Vec<IgnoreSet>,
	args: &Args,
	caches: &mut Caches,
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
		if resolve(&path, args, caches)?.is_some() {
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
		Ok(Invocation::Init(args)) => return init::run(args),
		#[cfg(feature = "lsp")]
		Ok(Invocation::Lsp) => return lsp::run(),
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
		let mut caches = Caches::default();
		let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
		let resolved = match &args.stdin_path {
			// With a path, the stream is whatever that file would be.
			Some(path) => stdin_as(path, &args, &cwd, &mut caches, false),
			// Without one, it is always SQL, never a host file.
			None => resolve_sql_defaults(&cwd, &args, &mut caches).map(Some),
		};
		let resolved = match resolved {
			Ok(Some(resolved)) => resolved,
			Ok(None) if args.locate => return ExitCode::SUCCESS,
			Ok(None) => {
				// Not ours to touch: hand the buffer back as it came.
				if !args.check {
					print!("{source}");
				}
				return ExitCode::SUCCESS;
			}
			Err(message) => {
				eprintln!("squill: {message}");
				return ExitCode::from(2);
			}
		};
		let label = args
			.stdin_path
			.as_ref()
			.map_or_else(|| "<stdin>".to_string(), |path| path.display().to_string());
		if args.locate {
			return match locate_resolved(&source, &resolved) {
				Ok(found) => {
					print_located(&label, &source, &found, args.json);
					ExitCode::SUCCESS
				}
				Err(message) => {
					eprintln!("squill: {label}: {message}");
					ExitCode::from(2)
				}
			};
		}
		let outcome = match format_resolved(&source, &resolved) {
			Ok(outcome) => outcome,
			Err(message) => {
				eprintln!("squill: {label}: {message}");
				return ExitCode::from(2);
			}
		};
		for diagnostic in &outcome.diagnostics {
			eprintln!("{label}:{}", diagnostic.render(&source));
		}
		if args.check {
			if outcome.formatted != source {
				print_diff(&label, &source, &outcome.formatted);
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

	let mut caches = Caches::default();
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
		let discovered =
			if args.no_config { Ok(None) } else { caches.discover(path) };
		let discovered = match discovered {
			Ok(discovered) => discovered,
			Err(message) => {
				eprintln!("squill: {message}");
				return ExitCode::from(2);
			}
		};
		if let Some(config_path) = discovered {
			let loaded = match caches.load(&config_path) {
				Ok(loaded) => loaded,
				Err(message) => {
					eprintln!("squill: {message}");
					return ExitCode::from(2);
				}
			};
			if !loaded.config.ignore.is_empty() {
				let anchor = config::anchor_dir(&config_path);
				let anchor =
					std::path::absolute(anchor).unwrap_or_else(|_| anchor.to_path_buf());
				match build_ignore_set(&loaded.config.ignore, &anchor) {
					Ok(set) => ignores.push(set),
					Err(message) => {
						eprintln!("squill: {}: {message}", config_path.display());
						return ExitCode::from(2);
					}
				}
			}
		}
		if let Err(message) =
			collect_files(path, ignores, &args, &mut caches, &mut files)
		{
			eprintln!("squill: {message}");
			return ExitCode::from(2);
		}
	}
	files.sort();
	files.dedup();

	// Drop files the baseline already carries. Before any formatting, so
	// a frozen file is never read, diffed, or counted.
	// (Locating reads without writing, so frozen files are fair game.)
	let frozen = if args.locate {
		Ok(0)
	} else {
		drop_frozen(&mut files, &args, &cwd, &mut caches)
	};
	let frozen_count = match frozen {
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
		// Only an explicit file can go unclaimed, and then it's an error.
		let resolved = match resolve(path, &args, &mut caches) {
			Ok(Some(resolved)) => Ok(resolved),
			Ok(None) => Err(unclaimed(path, &args, &mut caches)),
			Err(message) => Err(message),
		};
		match resolved {
			Ok(resolved) => per_file_options.push(resolved),
			Err(message) => {
				eprintln!("squill: {message}");
				return ExitCode::from(2);
			}
		}
	}

	if args.locate {
		let mut failed = false;
		for (path, resolved) in files.iter().zip(&per_file_options) {
			let label = path.display().to_string();
			let found = std::fs::read_to_string(path)
				.map_err(|err| err.to_string())
				.and_then(|source| {
					locate_resolved(&source, resolved).map(|found| (source, found))
				});
			match found {
				Ok((source, found)) => {
					print_located(&label, &source, &found, args.json)
				}
				Err(message) => {
					eprintln!("squill: {label}: {message}");
					failed = true;
				}
			}
		}
		return if failed { ExitCode::from(2) } else { ExitCode::SUCCESS };
	}

	let results: Vec<Result<FileResult, String>> = files
		.par_iter()
		.zip(per_file_options.par_iter())
		.map(|(path, resolved)| {
			let source = std::fs::read_to_string(path)
				.map_err(|err| format!("{}: {err}", path.display()))?;
			let outcome = format_resolved(&source, resolved)
				.map_err(|err| format!("{}: {err}", path.display()))?;
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
			eprintln!("{path}:{}", diagnostic.render(&result.source));
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
