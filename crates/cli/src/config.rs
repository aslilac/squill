//! squill.toml discovery and parsing.
//!
//! The shape: formatting keys at the top level set the defaults for
//! every file; `ignore` and `frozen` pick paths out; and two kinds of
//! path-scoped rule layer on top, in file order:
//!
//! ```toml
//! dialect = "postgres"
//!
//! [[files]]                       # plain SQL, by path
//! include = ["**/*.sql.sqlite"]
//! dialect = "sqlite"
//!
//! [[embedded]]                    # SQL inside host-language files
//! include = ["**/*.rs"]
//! grammar = "rust"                # built-in, or a path to a .wasm grammar
//! query = "squill/rust.scm"       # optional for built-ins
//! ```
//!
//! A file some `[[embedded]]` rule gives a grammar is a host file; else
//! a `.sql` file, or one a `[[files]]` rule includes, is SQL. Every rule
//! of that kind whose `include` matches applies, later rules winning
//! key by key. Paths are relative to the directory the config governs.
//! Unknown keys and type mismatches are hard errors with file:line.

use std::path::Path;
use std::path::PathBuf;

use formatter::IdentQuoting;
use formatter::IndentStyle;
use formatter::KeywordCase;
use parser::Dialect;
use toml::de::DeTable;
use toml::de::DeValue;

/// Formatting options set explicitly (by config or flags); unset fields
/// fall back.
#[derive(Debug, Default, Clone)]
pub struct PartialOptions {
	pub dialect: Option<Dialect>,
	pub indent_style: Option<IndentStyle>,
	pub indent_width: Option<u8>,
	pub max_width: Option<u16>,
	pub keyword_case: Option<KeywordCase>,
	pub quoting: Option<IdentQuoting>,
	pub at_params: Option<bool>,
	pub question_params: Option<bool>,
}

impl PartialOptions {
	pub fn apply(&self, options: &mut formatter::Options) {
		if let Some(value) = self.dialect {
			options.dialect = value;
		}
		if let Some(value) = self.indent_style {
			options.indent_style = value;
		}
		if let Some(value) = self.indent_width {
			options.indent_width = value;
		}
		if let Some(value) = self.max_width {
			options.max_width = value;
		}
		if let Some(value) = self.keyword_case {
			options.keyword_case = value;
		}
		if let Some(value) = self.quoting {
			options.quoting = value;
		}
		if let Some(value) = self.at_params {
			options.at_params = value;
		}
		if let Some(value) = self.question_params {
			options.question_params = value;
		}
	}
}

/// One parsed config file.
#[derive(Debug, Default, Clone)]
pub struct Config {
	/// Top-level formatting keys: the defaults for every file.
	pub options: PartialOptions,
	/// Glob patterns of paths to skip when recursing directories.
	pub ignore: Vec<String>,
	/// Glob patterns for paths that are immutable once they reach the
	/// baseline branch: formatted while new, never rewritten after.
	pub frozen: Vec<String>,
	/// The ref `frozen` compares against. `None` discovers it from the
	/// remote's recorded HEAD.
	pub frozen_ref: Option<String>,
	/// Let squill fetch the remote's HEAD when the baseline is not
	/// already recorded locally. On by default — a CI checkout usually
	/// has nothing else to go on — and `false` restricts squill to the
	/// refs already present.
	pub frozen_fetch: Option<bool>,
	/// `[[files]]` rules, in file order.
	pub files: Vec<FileRule>,
	/// `[[embedded]]` rules, in file order.
	pub embedded: Vec<EmbeddedRule>,
}

/// `[[files]]`: plain SQL files matching `include`, and their options.
#[derive(Debug, Clone)]
pub struct FileRule {
	pub include: Vec<String>,
	pub options: PartialOptions,
}

/// `[[embedded]]`: host-language files matching `include`, the grammar
/// and query that find their SQL, and its options.
#[derive(Debug, Clone)]
pub struct EmbeddedRule {
	pub include: Vec<String>,
	pub grammar: Option<GrammarSpec>,
	/// Path to a tree-sitter query (`.scm`), resolved against the
	/// config's directory.
	pub query: Option<PathBuf>,
	pub options: PartialOptions,
}

/// Where a rule's grammar comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarSpec {
	Builtin(embed::Host),
	/// A `.wasm` grammar, resolved against the config's directory.
	Wasm(PathBuf),
}

// Shared value parsers, used by both config keys and CLI flags.
pub fn parse_dialect(value: &str) -> Result<Dialect, String> {
	match value {
		"postgres" => Ok(Dialect::Postgres),
		"sqlite" => Ok(Dialect::Sqlite),
		other => Err(format!("unknown dialect `{other}`")),
	}
}

pub fn parse_indent(value: &str) -> Result<IndentStyle, String> {
	match value {
		"tab" | "tabs" => Ok(IndentStyle::Tab),
		"spaces" => Ok(IndentStyle::Spaces),
		other => Err(format!("unknown indent style `{other}`")),
	}
}

pub fn parse_keyword_case(value: &str) -> Result<KeywordCase, String> {
	match value {
		"lower" => Ok(KeywordCase::Lower),
		"upper" => Ok(KeywordCase::Upper),
		other => Err(format!("unknown keyword case `{other}`")),
	}
}

pub fn parse_quoting(value: &str) -> Result<IdentQuoting, String> {
	match value {
		"as-needed" => Ok(IdentQuoting::UnquotedWhenSafe),
		"always" => Ok(IdentQuoting::AlwaysQuoted),
		other => Err(format!("unknown quoting mode `{other}`")),
	}
}

/// Find the nearest config at or above `dir`: `squill.toml`, or
/// `.config/squill.toml` when the bare file is absent at that level.
///
/// The walk stops at the boundaries a config file has no business
/// crossing, checking each directory before deciding whether to leave
/// it: a git repository root, a mount point, and a symlinked directory
/// (whose lexical parent is not where it actually lives).
pub fn discover(dir: &Path) -> Option<PathBuf> {
	// Absolute first: `Path::parent` on a relative path runs out at the
	// working directory, which would cut the walk short of the repo
	// root. Lexical, not canonical — resolving symlinks here would erase
	// the very boundary we mean to stop at.
	let start = std::path::absolute(dir).ok()?;
	let mut current = start.as_path();
	loop {
		for candidate in
			[current.join("squill.toml"), current.join(".config/squill.toml")]
		{
			if candidate.is_file() {
				return Some(candidate);
			}
		}
		// A repository root: `.git` is a directory in a normal checkout
		// and a file in a worktree or submodule.
		if current.join(".git").exists() {
			return None;
		}
		// A symlinked directory: `parent()` would walk the path we came
		// in by, not the tree this directory really sits in.
		if std::fs::symlink_metadata(current)
			.is_ok_and(|meta| meta.file_type().is_symlink())
		{
			return None;
		}
		let parent = current.parent()?;
		if !same_device(current, parent) {
			return None;
		}
		current = parent;
	}
}

/// Are the two directories on the same filesystem? Unknowable without
/// `st_dev`, so platforms without it simply never stop here.
#[cfg(unix)]
fn same_device(a: &Path, b: &Path) -> bool {
	use std::os::unix::fs::MetadataExt;
	match (std::fs::metadata(a), std::fs::metadata(b)) {
		(Ok(a), Ok(b)) => a.dev() == b.dev(),
		// An unreadable parent is a boundary of its own kind.
		_ => false,
	}
}

#[cfg(not(unix))]
fn same_device(_a: &Path, _b: &Path) -> bool {
	true
}

/// The directory `ignore` patterns in `config_path` are relative to:
/// the config file's directory, or its parent for `.config/squill.toml`.
pub fn anchor_dir(config_path: &Path) -> &Path {
	let dir = config_path.parent().unwrap_or(Path::new("."));
	if dir.file_name().is_some_and(|name| name == ".config") {
		dir.parent().unwrap_or(dir)
	} else {
		dir
	}
}

/// Parse a config file. Errors carry `path:line:` prefixes.
pub fn parse_config(text: &str, path: &Path) -> Result<Config, String> {
	let located = |offset: usize, message: &str| {
		let (line, _) = crate::line_col(text, offset);
		format!("{}:{line}: {message}", path.display())
	};
	let table = DeTable::parse(text).map_err(|parse_err| {
		let offset = parse_err.span().map_or(text.len(), |span| span.start);
		located(offset, parse_err.message())
	})?;
	let anchor = anchor_dir(path);
	let mut config = Config::default();
	for (key, value) in table.get_ref() {
		let key_name: &str = key.get_ref().as_ref();
		let offset = key.span().start;
		match key_name {
			"files" | "embedded" => {
				let DeValue::Array(items) = value.get_ref() else {
					return Err(located(
						offset,
						&format!(
							"`{key_name}` is a list of rules; write each as `[[{key_name}]]`"
						),
					));
				};
				for item in items.iter() {
					let item_offset = item.span().start;
					let DeValue::Table(rule) = item.get_ref() else {
						return Err(located(
							item_offset,
							&format!(
								"`{key_name}` is a list of rules; write each as `[[{key_name}]]`"
							),
						));
					};
					let kind =
						if key_name == "files" { Scope::Files } else { Scope::Embedded };
					let parsed = parse_rule(rule, kind, item_offset, anchor, &located)?;
					match parsed {
						Rule::Files(rule) => config.files.push(rule),
						Rule::Embedded(rule) => config.embedded.push(rule),
					}
				}
			}
			_ => {
				apply_key(&mut config, None, key_name, offset, value, anchor, &located)?
			}
		}
	}
	Ok(config)
}

/// Which table a key was found in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
	TopLevel,
	Files,
	Embedded,
}

enum Rule {
	Files(FileRule),
	Embedded(EmbeddedRule),
}

/// A rule's keys, collected before checking the rule is whole.
#[derive(Default)]
struct RuleParts {
	include: Option<Vec<String>>,
	grammar: Option<GrammarSpec>,
	query: Option<PathBuf>,
}

fn parse_rule(
	table: &DeTable<'_>,
	scope: Scope,
	offset: usize,
	anchor: &Path,
	located: &dyn Fn(usize, &str) -> String,
) -> Result<Rule, String> {
	let mut target = Config::default();
	let mut parts = RuleParts::default();
	for (key, value) in table {
		let key_name: &str = key.get_ref().as_ref();
		let key_offset = key.span().start;
		apply_key(
			&mut target,
			Some((scope, &mut parts)),
			key_name,
			key_offset,
			value,
			anchor,
			located,
		)?;
	}
	let section =
		if scope == Scope::Files { "[[files]]" } else { "[[embedded]]" };
	let include = parts.include.ok_or_else(|| {
		located(offset, &format!("{section} needs an `include` list"))
	})?;
	Ok(match scope {
		Scope::Files => Rule::Files(FileRule { include, options: target.options }),
		_ => Rule::Embedded(EmbeddedRule {
			include,
			grammar: parts.grammar,
			query: parts.query,
			options: target.options,
		}),
	})
}

/// Apply one `key = value` pair: to the config's top level, or (with
/// `rule`) to a rule being built.
fn apply_key(
	config: &mut Config,
	mut rule: Option<(Scope, &mut RuleParts)>,
	key_name: &str,
	offset: usize,
	value: &toml::Spanned<DeValue<'_>>,
	anchor: &Path,
	located: &dyn Fn(usize, &str) -> String,
) -> Result<(), String> {
	let err = |message: String| located(offset, &message);
	let scope = rule.as_ref().map_or(Scope::TopLevel, |(scope, _)| *scope);
	let options = &mut config.options;
	let string = |what: &str| -> Result<String, String> {
		match value.get_ref() {
			DeValue::String(raw) => Ok(raw.to_string()),
			_ => Err(err(format!("`{what}` expects a quoted string"))),
		}
	};
	match key_name {
		"dialect" => {
			options.dialect = Some(parse_dialect(&string(key_name)?).map_err(&err)?)
		}
		"indent" => {
			options.indent_style =
				Some(parse_indent(&string(key_name)?).map_err(&err)?)
		}
		"keyword-case" => {
			options.keyword_case =
				Some(parse_keyword_case(&string(key_name)?).map_err(&err)?)
		}
		"quote-idents" => {
			options.quoting = Some(parse_quoting(&string(key_name)?).map_err(&err)?)
		}
		"indent-width" => match as_integer(value.get_ref()) {
			Some(n) if (1..=16).contains(&n) => {
				options.indent_width = Some(n as u8);
			}
			_ => {
				return Err(err(
					"`indent-width` expects an integer from 1 to 16".into(),
				));
			}
		},
		"max-width" => match as_integer(value.get_ref()) {
			Some(n) if (20..=500).contains(&n) => {
				options.max_width = Some(n as u16);
			}
			_ => {
				return Err(err(
					"`max-width` expects an integer from 20 to 500".into(),
				));
			}
		},
		"at-params" => match value.get_ref() {
			DeValue::Boolean(flag) => options.at_params = Some(*flag),
			_ => return Err(err("`at-params` expects true or false".into())),
		},
		"question-params" => match value.get_ref() {
			DeValue::Boolean(flag) => options.question_params = Some(*flag),
			_ => {
				return Err(err("`question-params` expects true or false".into()));
			}
		},
		"include" if scope != Scope::TopLevel => {
			let patterns = glob_list(key_name, value, located)?;
			if patterns.is_empty() {
				return Err(err("`include` needs at least one pattern".into()));
			}
			if let Some((_, parts)) = rule.as_mut() {
				parts.include = Some(patterns);
			}
		}
		"grammar" if scope == Scope::Embedded => {
			let raw = string(key_name)?;
			let spec = parse_grammar(&raw, anchor).map_err(&err)?;
			if let Some((_, parts)) = rule.as_mut() {
				parts.grammar = Some(spec);
			}
		}
		"query" if scope == Scope::Embedded => {
			let raw = string(key_name)?;
			if raw.trim().is_empty() {
				return Err(err("`query` expects a path to a .scm file".into()));
			}
			if let Some((_, parts)) = rule.as_mut() {
				parts.query = Some(anchor.join(raw));
			}
		}
		"grammar" | "query" if scope == Scope::Files => {
			return Err(err(format!(
				"`{key_name}` belongs in an [[embedded]] rule; [[files]] rules are plain SQL"
			)));
		}
		"include" | "grammar" | "query" => {
			return Err(err(format!(
				"`{key_name}` belongs in a [[files]] or [[embedded]] rule"
			)));
		}
		"ignore" | "frozen" | "frozen-ref" | "frozen-fetch"
			if scope != Scope::TopLevel =>
		{
			return Err(err(format!(
				"`{key_name}` applies to the whole file; put it at the top level"
			)));
		}
		"frozen-fetch" => match value.get_ref() {
			DeValue::Boolean(flag) => config.frozen_fetch = Some(*flag),
			_ => return Err(err("`frozen-fetch` expects true or false".into())),
		},
		"frozen-ref" => {
			let raw = string(key_name)?;
			if raw.trim().is_empty() {
				return Err(err("`frozen-ref` expects a ref name".into()));
			}
			config.frozen_ref = Some(raw);
		}
		"ignore" => config.ignore = glob_list(key_name, value, located)?,
		"frozen" => config.frozen = glob_list(key_name, value, located)?,
		other => {
			if matches!(value.get_ref(), DeValue::Table(_)) {
				return Err(err(match scope {
					Scope::TopLevel => format!(
						"unknown section `[{other}]`; path-scoped settings go in \
						 [[files]] or [[embedded]] rules"
					),
					_ => "rules do not nest".to_string(),
				}));
			}
			return Err(err(format!("unknown key `{other}`")));
		}
	}
	Ok(())
}

/// A `grammar` value: a built-in name, or a path ending in `.wasm`.
fn parse_grammar(raw: &str, anchor: &Path) -> Result<GrammarSpec, String> {
	if raw.ends_with(".wasm") {
		if cfg!(feature = "wasm") {
			return Ok(GrammarSpec::Wasm(anchor.join(raw)));
		}
		return Err(format!(
			"`{raw}`: this squill was built without wasm grammar support"
		));
	}
	embed::Host::from_name(raw).map(GrammarSpec::Builtin).ok_or_else(|| {
		let names: Vec<&str> =
			embed::Host::ALL.iter().map(|host| host.name()).collect();
		format!(
			"unknown grammar `{raw}`; expected one of {}, or a path to a .wasm grammar",
			names.join(", ")
		)
	})
}

/// An array of glob patterns, each checked to compile.
fn glob_list(
	key_name: &str,
	value: &toml::Spanned<DeValue<'_>>,
	located: &dyn Fn(usize, &str) -> String,
) -> Result<Vec<String>, String> {
	let DeValue::Array(items) = value.get_ref() else {
		return Err(located(
			value.span().start,
			&format!("`{key_name}` expects an array of strings"),
		));
	};
	let mut patterns = Vec::new();
	for item in items.iter() {
		let item_err = |message: String| located(item.span().start, &message);
		let DeValue::String(pattern) = item.get_ref() else {
			return Err(item_err(format!(
				"`{key_name}` expects an array of quoted strings"
			)));
		};
		let pattern: &str = pattern.as_ref();
		if pattern.is_empty() {
			return Err(item_err(format!("empty {key_name} pattern")));
		}
		globset::Glob::new(pattern).map_err(|glob_err| {
			item_err(format!("invalid {key_name} pattern `{pattern}`: {glob_err}"))
		})?;
		patterns.push(pattern.to_string());
	}
	Ok(patterns)
}

/// The integer value, if the TOML value is an in-range integer.
fn as_integer(value: &DeValue) -> Option<i64> {
	match value {
		DeValue::Integer(n) => i64::from_str_radix(n.as_str(), n.radix()).ok(),
		_ => None,
	}
}
