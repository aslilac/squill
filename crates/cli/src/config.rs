//! squill.toml discovery and parsing.
//!
//! The config surface is exactly the CLI's option surface — nothing new
//! sneaks in through config. Files are parsed with the `toml` crate's
//! spanned API, but the accepted shape stays deliberately flat: one
//! top-level table, no sections, and `ignore` is the only array. Unknown
//! keys and type mismatches are hard errors with file:line.

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
		let err = |message: String| located(key.span().start, &message);
		let key_name: &str = key.get_ref().as_ref();
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
			"ignore" => {
				let DeValue::Array(items) = value.get_ref() else {
					return Err(err("`ignore` expects an array of strings".into()));
				};
				let mut patterns = Vec::new();
				for item in items.iter() {
					let item_err = |message: String| located(item.span().start, &message);
					let DeValue::String(pattern) = item.get_ref() else {
						return Err(item_err(
							"`ignore` expects an array of quoted strings".into(),
						));
					};
					let pattern: &str = pattern.as_ref();
					if pattern.is_empty() {
						return Err(item_err("empty ignore pattern".into()));
					}
					globset::Glob::new(pattern).map_err(|glob_err| {
						item_err(format!("invalid ignore pattern `{pattern}`: {glob_err}"))
					})?;
					patterns.push(pattern.to_string());
				}
				options.ignore = patterns;
			}
			other => {
				if matches!(value.get_ref(), DeValue::Table(_)) {
					return Err(err("sections are not supported".into()));
				}
				return Err(err(format!("unknown key `{other}`")));
			}
		}
	}
	Ok(options)
}

/// The integer value, if the TOML value is an in-range integer.
fn as_integer(value: &DeValue) -> Option<i64> {
	match value {
		DeValue::Integer(n) => i64::from_str_radix(n.as_str(), n.radix()).ok(),
		_ => None,
	}
}
