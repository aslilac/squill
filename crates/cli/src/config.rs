//! squill.toml discovery and parsing.
//!
//! The config surface is exactly the CLI's option surface — nothing new sneaks
//! in through config. The format is a deliberately flat TOML subset:
//! `key = value` lines, `#` comments, no sections, no arrays. Parse errors and
//! unknown keys are hard errors with file:line.

use std::path::Path;
use std::path::PathBuf;

use formatter::IdentQuoting;
use formatter::IndentStyle;
use formatter::KeywordCase;
use parser::Dialect;

/// Options set explicitly (by config or flags); unset fields fall back.
#[derive(Debug, Default, Clone, Copy)]
pub struct PartialOptions {
	pub dialect: Option<Dialect>,
	pub indent_style: Option<IndentStyle>,
	pub indent_width: Option<u8>,
	pub keyword_case: Option<KeywordCase>,
	pub quoting: Option<IdentQuoting>,
	pub at_params: Option<bool>,
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

/// Find the nearest squill.toml at or above `dir`.
pub fn discover(dir: &Path) -> Option<PathBuf> {
	let mut dir = Some(dir);
	while let Some(current) = dir {
		let candidate = current.join("squill.toml");
		if candidate.is_file() {
			return Some(candidate);
		}
		dir = current.parent();
	}
	None
}

/// Parse a config file. Errors carry `path:line:` prefixes.
pub fn parse_config(text: &str, path: &Path) -> Result<PartialOptions, String> {
	let mut options = PartialOptions::default();
	let mut seen: Vec<String> = Vec::new();
	for (index, raw) in text.lines().enumerate() {
		let lineno = index + 1;
		let err =
			|message: String| format!("{}:{lineno}: {message}", path.display());
		let line = raw.trim();
		if line.is_empty() || line.starts_with('#') {
			continue;
		}
		if line.starts_with('[') {
			return Err(err("sections are not supported".to_string()));
		}
		let Some((key, value)) = line.split_once('=') else {
			return Err(err("expected `key = value`".to_string()));
		};
		let key = key.trim();
		let value = parse_value(value).map_err(&err)?;
		if seen.iter().any(|k| k == key) {
			return Err(err(format!("duplicate key `{key}`")));
		}
		seen.push(key.to_string());
		let string = |value: &Value| -> Result<String, String> {
			match value {
				Value::String(s) => Ok(s.clone()),
				_ => Err(err(format!("`{key}` expects a quoted string"))),
			}
		};
		match key {
			"dialect" => {
				options.dialect = Some(parse_dialect(&string(&value)?).map_err(&err)?);
			}
			"indent" => {
				options.indent_style =
					Some(parse_indent(&string(&value)?).map_err(&err)?);
			}
			"indent-width" => match value {
				Value::Integer(n) if (1..=16).contains(&n) => {
					options.indent_width = Some(n as u8);
				}
				_ => {
					return Err(err(
						"`indent-width` expects an integer from 1 to 16".into(),
					));
				}
			},
			"keyword-case" => {
				options.keyword_case =
					Some(parse_keyword_case(&string(&value)?).map_err(&err)?);
			}
			"quote-idents" => {
				options.quoting = Some(parse_quoting(&string(&value)?).map_err(&err)?);
			}
			"at-params" => match value {
				Value::Bool(b) => options.at_params = Some(b),
				_ => return Err(err("`at-params` expects true or false".into())),
			},
			other => return Err(err(format!("unknown key `{other}`"))),
		}
	}
	Ok(options)
}

enum Value {
	String(String),
	Integer(i64),
	Bool(bool),
}

/// Parse the value part of a `key = value` line, allowing a trailing
/// `# comment`.
fn parse_value(raw: &str) -> Result<Value, String> {
	let raw = raw.trim();
	if let Some(rest) = raw.strip_prefix('"') {
		let Some(end) = rest.find('"') else {
			return Err("unterminated string".to_string());
		};
		let inner = &rest[..end];
		if inner.contains('\\') {
			return Err("string escapes are not supported".to_string());
		}
		let tail = rest[end + 1..].trim();
		if !tail.is_empty() && !tail.starts_with('#') {
			return Err(format!("unexpected trailing `{tail}`"));
		}
		return Ok(Value::String(inner.to_string()));
	}
	let bare = raw.split('#').next().unwrap_or("").trim();
	match bare {
		"true" => Ok(Value::Bool(true)),
		"false" => Ok(Value::Bool(false)),
		_ => bare
			.parse::<i64>()
			.map(Value::Integer)
			.map_err(|_| format!("cannot parse value `{bare}`")),
	}
}
