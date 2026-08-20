//! squill.toml discovery and parsing.
//!
//! The config surface is exactly the CLI's option surface — nothing new sneaks
//! in through config. The format is a deliberately flat TOML subset:
//! `key = value` lines, `#` comments, no sections. The only array-valued key
//! is `ignore` (quoted glob patterns; the array may span lines). Parse errors
//! and unknown keys are hard errors with file:line.

use std::path::Path;
use std::path::PathBuf;

use formatter::IdentQuoting;
use formatter::IndentStyle;
use formatter::KeywordCase;
use parser::Dialect;

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
	/// relative to the config file's directory.
	pub ignore: Vec<String>,
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
pub fn discover(dir: &Path) -> Option<PathBuf> {
	let mut dir = Some(dir);
	while let Some(current) = dir {
		for candidate in
			[current.join("squill.toml"), current.join(".config/squill.toml")]
		{
			if candidate.is_file() {
				return Some(candidate);
			}
		}
		dir = current.parent();
	}
	None
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
	let mut options = PartialOptions::default();
	let mut seen: Vec<String> = Vec::new();
	let lines: Vec<&str> = text.lines().collect();
	let mut index = 0;
	while index < lines.len() {
		let lineno = index + 1;
		let err =
			|message: String| format!("{}:{lineno}: {message}", path.display());
		let line = lines[index].trim();
		index += 1;
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
		if seen.iter().any(|k| k == key) {
			return Err(err(format!("duplicate key `{key}`")));
		}
		seen.push(key.to_string());
		if key == "ignore" {
			// Array value, possibly spanning lines: accumulate until the
			// closing `]` (strings cannot contain newlines).
			let mut buffer = strip_comment(value).map_err(&err)?;
			if !buffer.trim_start().starts_with('[') {
				return Err(err("`ignore` expects an array of strings".to_string()));
			}
			while !array_closed(&buffer) {
				let Some(next) = lines.get(index) else {
					return Err(err("unterminated array".to_string()));
				};
				index += 1;
				buffer.push(' ');
				buffer.push_str(&strip_comment(next).map_err(&err)?);
			}
			let patterns = parse_string_array(&buffer).map_err(&err)?;
			for pattern in &patterns {
				globset::Glob::new(pattern).map_err(|glob_err| {
					err(format!("invalid ignore pattern `{pattern}`: {glob_err}"))
				})?;
			}
			options.ignore = patterns;
			continue;
		}
		let value = parse_value(value).map_err(&err)?;
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
			"max-width" => match value {
				Value::Integer(n) if (20..=500).contains(&n) => {
					options.max_width = Some(n as u16);
				}
				_ => {
					return Err(err(
						"`max-width` expects an integer from 20 to 500".into(),
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

/// Cut a `# comment` off a physical line, respecting quoted strings.
fn strip_comment(line: &str) -> Result<String, String> {
	let mut in_string = false;
	for (offset, ch) in line.char_indices() {
		match ch {
			'"' => in_string = !in_string,
			'#' if !in_string => return Ok(line[..offset].to_string()),
			_ => {}
		}
	}
	if in_string {
		return Err("unterminated string".to_string());
	}
	Ok(line.to_string())
}

/// Whether accumulated array text contains its closing `]` outside of
/// any quoted string.
fn array_closed(text: &str) -> bool {
	let mut in_string = false;
	for ch in text.chars() {
		match ch {
			'"' => in_string = !in_string,
			']' if !in_string => return true,
			_ => {}
		}
	}
	false
}

/// Parse `[ "a", "b", ]` (comments already stripped): quoted strings,
/// comma-separated, trailing comma allowed.
fn parse_string_array(raw: &str) -> Result<Vec<String>, String> {
	let raw = raw.trim();
	let Some(mut rest) = raw.strip_prefix('[') else {
		return Err("`ignore` expects an array of strings".to_string());
	};
	rest = rest.trim_start();
	let mut items = Vec::new();
	loop {
		if let Some(tail) = rest.strip_prefix(']') {
			if !tail.trim().is_empty() {
				return Err(format!("unexpected trailing `{}`", tail.trim()));
			}
			return Ok(items);
		}
		let Some(after_quote) = rest.strip_prefix('"') else {
			return Err("`ignore` expects an array of quoted strings".to_string());
		};
		let Some(end) = after_quote.find('"') else {
			return Err("unterminated string".to_string());
		};
		let inner = &after_quote[..end];
		if inner.contains('\\') {
			return Err("string escapes are not supported".to_string());
		}
		if inner.is_empty() {
			return Err("empty ignore pattern".to_string());
		}
		items.push(inner.to_string());
		rest = after_quote[end + 1..].trim_start();
		if let Some(after_comma) = rest.strip_prefix(',') {
			rest = after_comma.trim_start();
		} else if !rest.starts_with(']') {
			return Err("expected `,` or `]`".to_string());
		}
	}
}
