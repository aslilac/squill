//! squill.toml discovery and parsing.
//!
//! The config surface is exactly the CLI's option surface — nothing new
//! sneaks in through config. Files are parsed with the `toml` crate's
//! spanned API. The shape is one flat top-level table plus one optional
//! section per embedded host language (`[go]`, `[javascript]`, …), which
//! accepts the same keys and overrides them for files of that language.
//! Sections do not nest, and the path-scoped keys (`ignore`, `frozen`,
//! `frozen-ref`) are file-wide, so they stay top level. Unknown keys and type mismatches are hard errors with
//! file:line.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;

use formatter::IdentQuoting;
use formatter::IndentStyle;
use formatter::KeywordCase;
use parser::Dialect;
use toml::de::DeTable;
use toml::de::DeValue;

/// Options set explicitly (by config or flags); unset fields fall back.
#[derive(Debug, Default, Clone)]
pub struct PartialOptions {
	pub dialect: Option<Dialect>,
	pub indent_style: Option<IndentStyle>,
	pub indent_width: Option<u8>,
	pub max_width: Option<u16>,
	pub keyword_case: Option<KeywordCase>,
	pub quoting: Option<IdentQuoting>,
	pub at_params: Option<bool>,
	/// Glob patterns of paths to skip when recursing directories,
	/// relative to the config file's directory. File-wide: a language
	/// section never carries one.
	pub ignore: Vec<String>,
	/// Glob patterns for paths that are immutable once they reach the
	/// baseline branch: formatted while new, never rewritten after.
	/// File-wide, like [`ignore`](Self::ignore).
	pub frozen: Vec<String>,
	/// The ref `frozen` compares against. `None` discovers it from the
	/// remote's recorded HEAD.
	pub frozen_ref: Option<String>,
	/// Let squill fetch the remote's HEAD when no baseline ref is
	/// available locally. Off by default: formatting should not depend
	/// on the network unless asked.
	pub frozen_fetch: Option<bool>,
	/// Per-language overrides, keyed by section name (see
	/// [`language_key`]). Each layers on top of the top-level keys.
	pub languages: BTreeMap<String, PartialOptions>,
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
	}

	/// The overrides for `key`, if the file has that section.
	pub fn for_language(&self, key: &str) -> Option<&PartialOptions> {
		self.languages.get(key)
	}
}

/// The config section name for an embedded host language. `.ts` and
/// `.tsx` share `[typescript]`, and `.js`/`.jsx` share `[javascript]` —
/// the split exists only because they need different grammars.
pub fn language_key(host: embed::Host) -> &'static str {
	match host {
		embed::Host::Rust => "rust",
		embed::Host::Go => "go",
		embed::Host::Python => "python",
		embed::Host::JavaScript => "javascript",
		embed::Host::TypeScript | embed::Host::Tsx => "typescript",
		embed::Host::Gleam => "gleam",
	}
}

/// Every name [`language_key`] can return, for validating sections.
pub const LANGUAGE_KEYS: [&str; 6] =
	["rust", "go", "python", "javascript", "typescript", "gleam"];

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
pub fn parse_config(text: &str, path: &Path) -> Result<PartialOptions, String> {
	let located = |offset: usize, message: &str| {
		let (line, _) = crate::line_col(text, offset);
		format!("{}:{line}: {message}", path.display())
	};
	let table = DeTable::parse(text).map_err(|parse_err| {
		let offset = parse_err.span().map_or(text.len(), |span| span.start);
		located(offset, parse_err.message())
	})?;
	let mut options = PartialOptions::default();
	for (key, value) in table.get_ref() {
		let key_name: &str = key.get_ref().as_ref();
		let offset = key.span().start;
		// A table value is a language section; everything else is a key.
		let DeValue::Table(section) = value.get_ref() else {
			apply_key(
				&mut options,
				key_name,
				offset,
				value,
				Scope::TopLevel,
				&located,
			)?;
			continue;
		};
		// One section can name several languages: `["javascript,
		// typescript"]`. The quotes are TOML's, not ours — a bare table
		// header cannot hold a comma.
		let names: Vec<&str> = key_name.split(',').map(str::trim).collect();
		for name in &names {
			if name.is_empty() {
				return Err(located(
					offset,
					&format!("empty language name in section `[{key_name}]`"),
				));
			}
			if !LANGUAGE_KEYS.contains(name) {
				return Err(located(
					offset,
					&format!(
						"unknown section `[{name}]`; expected one of {}",
						LANGUAGE_KEYS.join(", ")
					),
				));
			}
		}
		let mut overrides = PartialOptions::default();
		for (key, value) in section {
			let key_name: &str = key.get_ref().as_ref();
			let offset = key.span().start;
			apply_key(
				&mut overrides,
				key_name,
				offset,
				value,
				Scope::Language,
				&located,
			)?;
		}
		for name in names {
			// Two sections claiming one language would silently make the
			// file order-dependent.
			if options.languages.insert(name.to_string(), overrides.clone()).is_some()
			{
				return Err(located(
					offset,
					&format!("`{name}` is configured by more than one section"),
				));
			}
		}
	}
	Ok(options)
}

/// Which table a key was found in: sections take the same keys as the
/// top level, minus the ones that are file-wide.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
	TopLevel,
	Language,
}

/// Apply one `key = value` pair to `options`.
fn apply_key(
	options: &mut PartialOptions,
	key_name: &str,
	offset: usize,
	value: &toml::Spanned<DeValue<'_>>,
	scope: Scope,
	located: &dyn Fn(usize, &str) -> String,
) -> Result<(), String> {
	let err = |message: String| located(offset, &message);
	match key_name {
		"dialect" | "indent" | "keyword-case" | "quote-idents" => {
			let DeValue::String(raw) = value.get_ref() else {
				return Err(err(format!("`{key_name}` expects a quoted string")));
			};
			let raw: &str = raw.as_ref();
			match key_name {
				"dialect" => {
					options.dialect = Some(parse_dialect(raw).map_err(&err)?);
				}
				"indent" => {
					options.indent_style = Some(parse_indent(raw).map_err(&err)?);
				}
				"keyword-case" => {
					options.keyword_case = Some(parse_keyword_case(raw).map_err(&err)?);
				}
				_ => options.quoting = Some(parse_quoting(raw).map_err(&err)?),
			}
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
		"frozen-fetch" if scope == Scope::Language => {
			return Err(err(
				"`frozen-fetch` applies to the whole file; put it at the top level"
					.into(),
			));
		}
		"frozen-fetch" => match value.get_ref() {
			DeValue::Boolean(flag) => options.frozen_fetch = Some(*flag),
			_ => return Err(err("`frozen-fetch` expects true or false".into())),
		},
		"frozen-ref" if scope == Scope::Language => {
			return Err(err(
				"`frozen-ref` applies to the whole file; put it at the top level"
					.into(),
			));
		}
		"frozen-ref" => {
			let DeValue::String(raw) = value.get_ref() else {
				return Err(err("`frozen-ref` expects a quoted string".into()));
			};
			let raw: &str = raw.as_ref();
			if raw.trim().is_empty() {
				return Err(err("`frozen-ref` expects a ref name".into()));
			}
			options.frozen_ref = Some(raw.to_string());
		}
		"ignore" | "frozen" if scope == Scope::Language => {
			return Err(err(format!(
				"`{key_name}` applies to the whole file; put it at the top level"
			)));
		}
		"ignore" | "frozen" => {
			let DeValue::Array(items) = value.get_ref() else {
				return Err(err(format!("`{key_name}` expects an array of strings")));
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
					item_err(format!(
						"invalid {key_name} pattern `{pattern}`: {glob_err}"
					))
				})?;
				patterns.push(pattern.to_string());
			}
			if key_name == "ignore" {
				options.ignore = patterns;
			} else {
				options.frozen = patterns;
			}
		}
		other => {
			if matches!(value.get_ref(), DeValue::Table(_)) {
				return Err(err(match scope {
					Scope::TopLevel => "sections are not supported".into(),
					Scope::Language => "language sections do not nest".to_string(),
				}));
			}
			return Err(err(format!("unknown key `{other}`")));
		}
	}
	Ok(())
}

/// The integer value, if the TOML value is an in-range integer.
fn as_integer(value: &DeValue) -> Option<i64> {
	match value {
		DeValue::Integer(n) => i64::from_str_radix(n.as_str(), n.radix()).ok(),
		_ => None,
	}
}
