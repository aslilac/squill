//! squill.toml (or squill.yaml) discovery and parsing.
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
use formatter::TrailingSemicolons;
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
	pub trailing_semicolons: Option<TrailingSemicolons>,
	pub at_params: Option<bool>,
	pub question_params: Option<bool>,
	pub colon_params: Option<bool>,
	pub pyformat_params: Option<bool>,
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
		if let Some(value) = self.trailing_semicolons {
			options.trailing_semicolons = value;
		}
		if let Some(value) = self.at_params {
			options.at_params = value;
		}
		if let Some(value) = self.question_params {
			options.question_params = value;
		}
		if let Some(value) = self.colon_params {
			options.colon_params = value;
		}
		if let Some(value) = self.pyformat_params {
			options.pyformat_params = value;
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
	/// A `.wasm` grammar at an https URL, locked in `squill.lock`.
	Url(String),
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

pub fn parse_trailing_semicolons(
	value: &str,
) -> Result<TrailingSemicolons, String> {
	match value {
		"always" => Ok(TrailingSemicolons::Always),
		"none" => Ok(TrailingSemicolons::None),
		other => Err(format!("unknown trailing-semicolons mode `{other}`")),
	}
}

pub fn parse_quoting(value: &str) -> Result<IdentQuoting, String> {
	match value {
		"as-needed" => Ok(IdentQuoting::UnquotedWhenSafe),
		"always" => Ok(IdentQuoting::AlwaysQuoted),
		other => Err(format!("unknown quoting mode `{other}`")),
	}
}

/// The names a config file can have: TOML, the default, or YAML.
pub const CONFIG_NAMES: [&str; 3] =
	["squill.toml", "squill.yaml", "squill.yml"];

/// Find the nearest config at or above `dir`: one of [`CONFIG_NAMES`],
/// or the same under `.config/` when none is at that level. More than
/// one at a level is an error.
///
/// The walk stops at the boundaries a config file has no business
/// crossing, checking each directory before deciding whether to leave
/// it: a git repository root, a mount point, and a symlinked directory
/// (whose lexical parent is not where it actually lives).
pub fn discover(dir: &Path) -> Result<Option<PathBuf>, String> {
	// Absolute first: `Path::parent` on a relative path runs out at the
	// working directory, which would cut the walk short of the repo
	// root. Lexical, not canonical — resolving symlinks here would erase
	// the very boundary we mean to stop at.
	let Ok(start) = std::path::absolute(dir) else {
		return Ok(None);
	};
	let mut current = start.as_path();
	loop {
		// The directory itself, then its `.config/`. Two configs in one
		// place is a mistake to point out, not a precedence to guess.
		for place in [current.to_path_buf(), current.join(".config")] {
			let found: Vec<PathBuf> = CONFIG_NAMES
				.iter()
				.map(|name| place.join(name))
				.filter(|candidate| candidate.is_file())
				.collect();
			match found.as_slice() {
				[] => {}
				[only] => return Ok(Some(only.clone())),
				several => {
					let names: Vec<String> =
						several.iter().map(|path| path.display().to_string()).collect();
					return Err(format!(
						"{} are both configs for the same directory; keep one",
						names.join(" and ")
					));
				}
			}
		}
		// A repository root: `.git` is a directory in a normal checkout
		// and a file in a worktree or submodule.
		if current.join(".git").exists() {
			return Ok(None);
		}
		// A symlinked directory: `parent()` would walk the path we came
		// in by, not the tree this directory really sits in.
		if std::fs::symlink_metadata(current)
			.is_ok_and(|meta| meta.file_type().is_symlink())
		{
			return Ok(None);
		}
		let Some(parent) = current.parent() else {
			return Ok(None);
		};
		// A directory that doesn't exist yet (an unsaved editor buffer's,
		// via --stdin-filepath) is no mount point: walk on up.
		if current.exists() && !same_device(current, parent) {
			return Ok(None);
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
/// the config file's directory, or its parent for `.config/squill.toml`
/// (or `.yaml`).
pub fn anchor_dir(config_path: &Path) -> &Path {
	let dir = config_path.parent().unwrap_or(Path::new("."));
	if dir.file_name().is_some_and(|name| name == ".config") {
		dir.parent().unwrap_or(dir)
	} else {
		dir
	}
}

/// Parse a config file, TOML or (by its `.yaml` / `.yml` extension)
/// YAML. Errors carry `path:line:` prefixes.
pub fn parse_config(text: &str, path: &Path) -> Result<Config, String> {
	let syntax = Syntax::of(path);
	let located = |line: usize, message: &str| {
		format!("{}:{line}: {message}", path.display())
	};
	let root = match syntax {
		Syntax::Toml => toml_tree(text, &located)?,
		Syntax::Yaml => yaml_tree(text, &located)?,
	};
	let anchor = anchor_dir(path);
	let context = Context { syntax, anchor, located: &located };
	let mut config = Config::default();
	for (key, value) in &root {
		match key.name.as_str() {
			"files" | "embedded" => {
				let rules_hint = || {
					located(
						key.line,
						&format!(
							"`{}` is a list of rules; {}",
							key.name,
							syntax.list_hint(&key.name)
						),
					)
				};
				let Value::Array(items) = &value.value else {
					return Err(rules_hint());
				};
				for item in items {
					let Value::Table(rule) = &item.value else {
						return Err(rules_hint());
					};
					let scope =
						if key.name == "files" { Scope::Files } else { Scope::Embedded };
					match parse_rule(rule, scope, item.line, &context)? {
						Rule::Files(rule) => config.files.push(rule),
						Rule::Embedded(rule) => config.embedded.push(rule),
					}
				}
			}
			_ => apply_key(&mut config, None, key, value, &context)?,
		}
	}
	Ok(config)
}

/// The two config formats.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Syntax {
	Toml,
	Yaml,
}

impl Syntax {
	fn of(path: &Path) -> Syntax {
		match path.extension().and_then(|ext| ext.to_str()) {
			Some("yaml" | "yml") => Syntax::Yaml,
			_ => Syntax::Toml,
		}
	}

	/// A rule of `kind`, the way this format writes one.
	fn rule(self, kind: &str) -> String {
		match self {
			Syntax::Toml => format!("[[{kind}]]"),
			Syntax::Yaml => format!("a `{kind}` rule"),
		}
	}

	/// The name of a `kind` rule, the way this format writes it.
	fn name(self, kind: &str) -> String {
		match self {
			Syntax::Toml => format!("[[{kind}]]"),
			Syntax::Yaml => format!("`{kind}`"),
		}
	}

	/// How to write a list of `kind` rules.
	fn list_hint(self, kind: &str) -> String {
		match self {
			Syntax::Toml => format!("write each as `[[{kind}]]`"),
			Syntax::Yaml => {
				format!("write them as a list under `{kind}:`, each `- include: [...]`")
			}
		}
	}
}

/// What the key checks need besides the value: the format (for the
/// wording of errors), the directory paths are relative to, and how to
/// locate an error.
struct Context<'a> {
	syntax: Syntax,
	anchor: &'a Path,
	located: &'a dyn Fn(usize, &str) -> String,
}

/// A config value from either format, with the line it's on.
struct Node {
	line: usize,
	value: Value,
}

enum Value {
	String(String),
	Bool(bool),
	Integer(i64),
	Array(Vec<Node>),
	Table(Vec<(Key, Node)>),
	/// Anything else (a float, a date, a null): only ever a type error.
	Other,
}

struct Key {
	name: String,
	line: usize,
}

/// A TOML document as a tree of [`Node`]s.
fn toml_tree(
	text: &str,
	located: &dyn Fn(usize, &str) -> String,
) -> Result<Vec<(Key, Node)>, String> {
	let line = |offset: usize| crate::line_col(text, offset).0;
	let table = DeTable::parse(text).map_err(|parse_err| {
		let offset = parse_err.span().map_or(text.len(), |span| span.start);
		located(line(offset), parse_err.message())
	})?;
	fn table_of(
		table: &DeTable<'_>,
		line: &dyn Fn(usize) -> usize,
	) -> Vec<(Key, Node)> {
		table
			.iter()
			.map(|(key, value)| {
				let key =
					Key { name: key.get_ref().to_string(), line: line(key.span().start) };
				(key, node_of(value, line))
			})
			.collect()
	}
	fn node_of(
		value: &toml::Spanned<DeValue<'_>>,
		line: &dyn Fn(usize) -> usize,
	) -> Node {
		let value_of = match value.get_ref() {
			DeValue::String(text) => Value::String(text.to_string()),
			DeValue::Boolean(flag) => Value::Bool(*flag),
			DeValue::Integer(n) => i64::from_str_radix(n.as_str(), n.radix())
				.map_or(Value::Other, Value::Integer),
			DeValue::Array(items) => {
				Value::Array(items.iter().map(|item| node_of(item, line)).collect())
			}
			DeValue::Table(table) => Value::Table(table_of(table, line)),
			_ => Value::Other,
		};
		Node { line: line(value.span().start), value: value_of }
	}
	Ok(table_of(table.get_ref(), &line))
}

/// A YAML document as a tree of [`Node`]s. The top level must be a
/// mapping (an empty document is an empty config).
fn yaml_tree(
	text: &str,
	located: &dyn Fn(usize, &str) -> String,
) -> Result<Vec<(Key, Node)>, String> {
	use saphyr::LoadableYamlNode;
	use saphyr::MarkedYaml;
	use saphyr::Scalar;
	use saphyr::YamlData;
	let documents = MarkedYaml::load_from_str(text)
		.map_err(|err| located(err.marker().line(), err.info()))?;
	if documents.len() > 1 {
		return Err(located(
			documents[1].span.start.line(),
			"a config file holds one YAML document",
		));
	}
	fn node_of(
		yaml: &MarkedYaml<'_>,
		located: &dyn Fn(usize, &str) -> String,
	) -> Result<Node, String> {
		let line = yaml.span.start.line();
		let value = match &yaml.data {
			YamlData::Value(Scalar::String(text)) => Value::String(text.to_string()),
			YamlData::Value(Scalar::Boolean(flag)) => Value::Bool(*flag),
			YamlData::Value(Scalar::Integer(n)) => Value::Integer(*n),
			YamlData::Representation(text, ..) => Value::String(text.to_string()),
			YamlData::Sequence(items) => Value::Array(
				items
					.iter()
					.map(|item| node_of(item, located))
					.collect::<Result<_, _>>()?,
			),
			YamlData::Mapping(entries) => {
				let mut table = Vec::new();
				for (key, value) in entries {
					let name = match &key.data {
						YamlData::Value(Scalar::String(name)) => name.to_string(),
						YamlData::Representation(name, ..) => name.to_string(),
						_ => {
							return Err(located(
								key.span.start.line(),
								"keys must be strings",
							));
						}
					};
					let key = Key { name, line: key.span.start.line() };
					table.push((key, node_of(value, located)?));
				}
				Value::Table(table)
			}
			_ => Value::Other,
		};
		Ok(Node { line, value })
	}
	let Some(document) = documents.first() else {
		return Ok(Vec::new());
	};
	match node_of(document, located)?.value {
		Value::Table(table) => Ok(table),
		_ => Err(located(
			document.span.start.line(),
			"the top level must be a mapping of keys",
		)),
	}
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
	table: &[(Key, Node)],
	scope: Scope,
	line: usize,
	context: &Context<'_>,
) -> Result<Rule, String> {
	let mut target = Config::default();
	let mut parts = RuleParts::default();
	for (key, value) in table {
		apply_key(&mut target, Some((scope, &mut parts)), key, value, context)?;
	}
	let kind = if scope == Scope::Files { "files" } else { "embedded" };
	let include = parts.include.ok_or_else(|| {
		(context.located)(
			line,
			&format!("{} needs an `include` list", context.syntax.rule(kind)),
		)
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

/// Apply one key and its value: to the config's top level, or (with
/// `rule`) to a rule being built.
fn apply_key(
	config: &mut Config,
	mut rule: Option<(Scope, &mut RuleParts)>,
	key: &Key,
	value: &Node,
	context: &Context<'_>,
) -> Result<(), String> {
	let key_name = key.name.as_str();
	let err = |message: String| (context.located)(key.line, &message);
	let scope = rule.as_ref().map_or(Scope::TopLevel, |(scope, _)| *scope);
	let anchor = context.anchor;
	let syntax = context.syntax;
	let options = &mut config.options;
	let string = |what: &str| -> Result<String, String> {
		match &value.value {
			Value::String(raw) => Ok(raw.clone()),
			_ => Err(err(format!("`{what}` expects a string"))),
		}
	};
	let boolean = |what: &str| -> Result<bool, String> {
		match &value.value {
			Value::Bool(flag) => Ok(*flag),
			_ => Err(err(format!("`{what}` expects true or false"))),
		}
	};
	let integer = || match &value.value {
		Value::Integer(n) => Some(*n),
		_ => None,
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
		"trailing-semicolons" => {
			options.trailing_semicolons =
				Some(parse_trailing_semicolons(&string(key_name)?).map_err(&err)?)
		}
		"indent-width" => match integer() {
			Some(n) if (1..=16).contains(&n) => {
				options.indent_width = Some(n as u8);
			}
			_ => {
				return Err(err(
					"`indent-width` expects an integer from 1 to 16".into(),
				));
			}
		},
		"max-width" => match integer() {
			Some(n) if (20..=500).contains(&n) => {
				options.max_width = Some(n as u16);
			}
			_ => {
				return Err(err(
					"`max-width` expects an integer from 20 to 500".into(),
				));
			}
		},
		"at-params" => options.at_params = Some(boolean(key_name)?),
		"pyformat-params" => options.pyformat_params = Some(boolean(key_name)?),
		"question-params" => options.question_params = Some(boolean(key_name)?),
		"colon-params" => options.colon_params = Some(boolean(key_name)?),
		"include" if scope != Scope::TopLevel => {
			let patterns = glob_list(key_name, value, context)?;
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
				"`{key_name}` belongs in an {} rule; {} rules are plain SQL",
				syntax.name("embedded"),
				syntax.name("files"),
			)));
		}
		"include" | "grammar" | "query" => {
			return Err(err(format!(
				"`{key_name}` belongs in a {} or {} rule",
				syntax.name("files"),
				syntax.name("embedded")
			)));
		}
		"ignore" | "frozen" | "frozen-ref" | "frozen-fetch"
			if scope != Scope::TopLevel =>
		{
			return Err(err(format!(
				"`{key_name}` applies to the whole file; put it at the top level"
			)));
		}
		"frozen-fetch" => config.frozen_fetch = Some(boolean(key_name)?),
		"frozen-ref" => {
			let raw = string(key_name)?;
			if raw.trim().is_empty() {
				return Err(err("`frozen-ref` expects a ref name".into()));
			}
			config.frozen_ref = Some(raw);
		}
		"ignore" => config.ignore = glob_list(key_name, value, context)?,
		"frozen" => config.frozen = glob_list(key_name, value, context)?,
		other => {
			if matches!(value.value, Value::Table(_)) {
				return Err(err(match (scope, syntax) {
					(Scope::TopLevel, Syntax::Toml) => format!(
						"unknown section `[{other}]`; path-scoped settings go in \
						 [[files]] or [[embedded]] rules"
					),
					(Scope::TopLevel, Syntax::Yaml) => format!(
						"unknown section `{other}`; path-scoped settings go in \
						 `files` or `embedded` rules"
					),
					_ => "rules do not nest".to_string(),
				}));
			}
			return Err(err(format!("unknown key `{other}`")));
		}
	}
	Ok(())
}

/// A `grammar` value: a built-in name, or a path or https URL ending in
/// `.wasm`.
fn parse_grammar(raw: &str, anchor: &Path) -> Result<GrammarSpec, String> {
	if raw.starts_with("http://") {
		return Err(format!("`{raw}`: grammar URLs must use https"));
	}
	if raw.starts_with("https://") {
		if !cfg!(feature = "external-grammars") {
			return Err(format!(
				"`{raw}`: this squill was built without wasm grammar support"
			));
		}
		if url_file_name(raw).is_none_or(|name| !name.ends_with(".wasm")) {
			return Err(format!(
				"`{raw}`: a grammar URL must name a `.wasm` file, like \
				 `tree-sitter-lua.wasm`: the language is named after it"
			));
		}
		return Ok(GrammarSpec::Url(raw.to_string()));
	}
	if raw.ends_with(".wasm") {
		if cfg!(feature = "external-grammars") {
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

/// The last path segment of a URL, without any query or fragment.
pub fn url_file_name(url: &str) -> Option<&str> {
	let path = url.split(['?', '#']).next()?;
	path.rsplit('/').next().filter(|name| !name.is_empty())
}

/// An array of glob patterns, each checked to compile.
fn glob_list(
	key_name: &str,
	value: &Node,
	context: &Context<'_>,
) -> Result<Vec<String>, String> {
	let Value::Array(items) = &value.value else {
		return Err((context.located)(
			value.line,
			&format!("`{key_name}` expects an array of strings"),
		));
	};
	let mut patterns = Vec::new();
	for item in items {
		let item_err = |message: String| (context.located)(item.line, &message);
		let Value::String(pattern) = &item.value else {
			return Err(item_err(format!(
				"`{key_name}` expects an array of strings"
			)));
		};
		if pattern.is_empty() {
			return Err(item_err(format!("empty {key_name} pattern")));
		}
		globset::Glob::new(pattern).map_err(|glob_err| {
			item_err(format!("invalid {key_name} pattern `{pattern}`: {glob_err}"))
		})?;
		patterns.push(pattern.clone());
	}
	Ok(patterns)
}
