//! The docs-site playground module: format SQL (plain or embedded in a
//! host language) behind a tiny JSON-over-C-ABI surface, compiled to
//! `wasm32-wasip1` and loaded in the browser with a minimal WASI stub.
//! No wasm-bindgen — the whole interface is three exported functions.
//!
//! Built once per grammar (see the crate's features), so a request for a
//! host this build doesn't carry is an unknown host.

use serde::Deserialize;
use serde::Serialize;

#[derive(Deserialize)]
struct Request {
	source: String,
	/// `"sql"`, or an embed host by its config name (`rust`, `tsx`, …).
	host: String,
	/// A tree-sitter query to find the SQL with, instead of the host's
	/// default (a docs recipe's own query file).
	#[cfg_attr(not(feature = "embedded"), allow(dead_code))]
	query: Option<String>,
	#[serde(default)]
	options: RequestOptions,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RequestOptions {
	dialect: Option<String>,
	indent: Option<String>,
	indent_width: Option<u8>,
	max_width: Option<u16>,
	keyword_case: Option<String>,
	quote_idents: Option<String>,
	trailing_semicolons: Option<String>,
	at_params: Option<bool>,
	question_params: Option<bool>,
	colon_params: Option<bool>,
	pyformat_params: Option<bool>,
}

#[derive(Serialize)]
struct Response {
	/// The formatted result. On a hard error this is absent and only
	/// diagnostics are populated.
	output: Option<String>,
	/// Human-readable notes: parse errors, verbatim fallbacks.
	diagnostics: Vec<String>,
	/// Where the SQL is in `output`, as `[start, end)` byte offsets: the
	/// whole text for plain SQL, each string the query captures for a
	/// host. What the docs highlight as SQL.
	spans: Vec<[usize; 2]>,
}

fn build_options(req: &RequestOptions) -> formatter::Options {
	let mut options = formatter::Options::default();
	if let Some(dialect) = req.dialect.as_deref() {
		options.dialect = match dialect {
			"sqlite" => parser::Dialect::Sqlite,
			_ => parser::Dialect::Postgres,
		};
	}
	if let Some(indent) = req.indent.as_deref() {
		options.indent_style = match indent {
			"spaces" => formatter::IndentStyle::Spaces,
			_ => formatter::IndentStyle::Tab,
		};
	}
	if let Some(width) = req.indent_width {
		options.indent_width = width.clamp(1, 16);
	}
	if let Some(width) = req.max_width {
		options.max_width = width.clamp(20, 500);
	}
	if let Some(case) = req.keyword_case.as_deref() {
		options.keyword_case = match case {
			"upper" => formatter::KeywordCase::Upper,
			_ => formatter::KeywordCase::Lower,
		};
	}
	if let Some(quoting) = req.quote_idents.as_deref() {
		options.quoting = match quoting {
			"always" => formatter::IdentQuoting::AlwaysQuoted,
			_ => formatter::IdentQuoting::UnquotedWhenSafe,
		};
	}
	if let Some(mode) = req.trailing_semicolons.as_deref() {
		options.trailing_semicolons = match mode {
			"none" => formatter::TrailingSemicolons::None,
			_ => formatter::TrailingSemicolons::Always,
		};
	}
	if let Some(at_params) = req.at_params {
		options.at_params = at_params;
	}
	if let Some(question_params) = req.question_params {
		options.question_params = question_params;
	}
	if let Some(colon_params) = req.colon_params {
		options.colon_params = colon_params;
	}
	if let Some(pyformat_params) = req.pyformat_params {
		options.pyformat_params = pyformat_params;
	}
	options
}

#[cfg(feature = "embedded")]
fn embed_host(name: &str) -> Option<(embed::Host, &'static str)> {
	let host = embed::Host::from_name(name)?;
	Some((host, host.default_query()))
}

/// 1-based line and column for a byte offset.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
	let clamped = offset.min(source.len());
	let line = source[..clamped].matches('\n').count() + 1;
	let start = source[..clamped].rfind('\n').map_or(0, |pos| pos + 1);
	(line, source[start..clamped].chars().count() + 1)
}

fn format_sql(source: &str, options: &formatter::Options) -> Response {
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
	diagnostics.extend(result.diagnostics.iter().map(|body| {
		let (line, col) = line_col(source, body.start);
		format!("{line}:{col}: {}", body.message)
	}));
	if result.fallback_statements > 0 {
		diagnostics.push(format!(
			"{} statement(s) passed through verbatim (formatter self-check)",
			result.fallback_statements
		));
	}
	let spans = vec![[0, result.text.len()]];
	Response { output: Some(result.text), diagnostics, spans }
}

#[cfg(feature = "embedded")]
fn format_host(
	source: &str,
	host: embed::Host,
	query: &str,
	options: &formatter::Options,
	indent: embed::Indent,
) -> Response {
	match embed::format_embedded(source, &host.into(), query, options, indent) {
		Ok(output) => {
			let diagnostics = output
				.warnings
				.iter()
				.map(|warning| {
					let (line, col) = line_col(source, warning.offset);
					format!("{line}:{col}: {}", warning.message)
				})
				.collect();
			// Where the SQL landed, for highlighting: found the same way as
			// before formatting, in the new text.
			let spans =
				embed::locate_sql(&output.text, &host.into(), query, options.dialect)
					.map(|found| {
						found
							.iter()
							.map(|located| [located.range.start, located.range.end])
							.collect()
					})
					.unwrap_or_default();
			Response { output: Some(output.text), diagnostics, spans }
		}
		Err(err) => Response {
			output: None,
			diagnostics: vec![err.to_string()],
			spans: Vec::new(),
		},
	}
}

/// The JSON entry point: a [`Request`] in, a [`Response`] out.
pub fn format_request(json: &str) -> String {
	let response = match serde_json::from_str::<Request>(json) {
		Ok(request) => {
			let options = build_options(&request.options);
			if request.host == "sql" {
				format_sql(&request.source, &options)
			} else {
				format_embedded(&request, options)
			}
		}
		Err(err) => Response {
			output: None,
			diagnostics: vec![format!("bad request: {err}")],
			spans: Vec::new(),
		},
	};
	serde_json::to_string(&response).unwrap_or_else(|_| {
		r#"{"output":null,"diagnostics":["response serialization failed"],"spans":[]}"#
			.into()
	})
}

fn unknown_host(request: &Request) -> Response {
	Response {
		output: None,
		diagnostics: vec![format!("unknown host `{}`", request.host)],
		spans: Vec::new(),
	}
}

#[cfg(not(feature = "embedded"))]
fn format_embedded(request: &Request, _: formatter::Options) -> Response {
	unknown_host(request)
}

#[cfg(feature = "embedded")]
fn format_embedded(request: &Request, options: formatter::Options) -> Response {
	match embed_host(&request.host) {
		Some((host, default_query)) => {
			let query = request.query.as_deref().unwrap_or(default_query);
			// Same rule as the CLI: indent options the caller named win,
			// otherwise the host file's own indentation does.
			let indent = embed::Indent {
				configured_style: request.options.indent.is_some(),
				configured_width: request.options.indent_width.is_some(),
			};
			// And no final `;` unless asked for.
			let trailing_semicolons = match request.options.trailing_semicolons {
				Some(_) => options.trailing_semicolons,
				None => formatter::TrailingSemicolons::None,
			};
			let options = formatter::Options { trailing_semicolons, ..options };
			format_host(&request.source, host, query, &options, indent)
		}
		None => unknown_host(request),
	}
}

/// The C ABI the browser talks to. The workspace denies `unsafe_code`;
/// this FFI boundary is the one place it is inherent.
mod ffi {
	#![allow(unsafe_code)]

	/// Allocate `len` bytes the host can write a request into.
	#[unsafe(no_mangle)]
	pub extern "C" fn squill_alloc(len: usize) -> *mut u8 {
		let mut buf = Vec::with_capacity(len);
		let ptr = buf.as_mut_ptr();
		std::mem::forget(buf);
		ptr
	}

	/// Free a buffer previously returned by [`squill_alloc`] or as the
	/// response half of [`squill_format`].
	#[unsafe(no_mangle)]
	pub extern "C" fn squill_dealloc(ptr: *mut u8, len: usize) {
		unsafe {
			drop(Vec::from_raw_parts(ptr, len, len));
		}
	}

	/// Format a JSON request (`ptr`, `len`); returns `pointer << 32 | length`
	/// of a JSON response the caller must read and then `squill_dealloc`.
	#[unsafe(no_mangle)]
	pub extern "C" fn squill_format(ptr: *const u8, len: usize) -> u64 {
		let json = unsafe { std::slice::from_raw_parts(ptr, len) };
		let out = match std::str::from_utf8(json) {
			Ok(json) => super::format_request(json),
			Err(_) => {
				r#"{"output":null,"diagnostics":["request was not UTF-8"]}"#.into()
			}
		};
		let bytes = out.into_bytes();
		let out_len = bytes.len();
		let out_ptr = bytes.as_ptr();
		std::mem::forget(bytes);
		((out_ptr as u64) << 32) | out_len as u64
	}
}

#[cfg(test)]
mod tests {
	use super::format_request;

	#[test]
	fn plain_sql_formats() {
		let response =
			format_request(r#"{"source":"SELECT   1;","host":"sql","options":{}}"#);
		assert!(response.contains("select 1;"), "{response}");
		assert!(response.contains(r#""diagnostics":[]"#), "{response}");
	}

	#[test]
	fn options_apply() {
		let response = format_request(
			r#"{"source":"select 1;","host":"sql","options":{"keyword_case":"upper"}}"#,
		);
		assert!(response.contains("SELECT 1;"), "{response}");
	}

	#[cfg(feature = "rust")]
	#[test]
	fn embedded_rust_formats() {
		let request = serde_json::json!({
			"source": "fn main() {\n    sqlx::query!(r#\"select id,name from users where org = $1 order by name\"#);\n}\n",
			"host": "rust",
		});
		let response = format_request(&request.to_string());
		assert!(response.contains("from users"), "{response}");
	}

	#[cfg(feature = "rust")]
	#[test]
	fn spans_say_where_the_sql_is() {
		let request = serde_json::json!({
			"source": "fn main() {\n    run(\"SELECT   1\");\n}\n",
			"host": "rust",
			"query": "(call_expression function: (identifier) @_f arguments: (arguments (string_literal (string_content) @sql)) (#eq? @_f \"run\"))",
		});
		let response: serde_json::Value =
			serde_json::from_str(&format_request(&request.to_string())).unwrap();
		let output = response["output"].as_str().unwrap();
		let [start, end] =
			[0, 1].map(|i| response["spans"][0][i].as_u64().unwrap() as usize);
		assert_eq!(&output[start..end], "SELECT   1", "{response}");

		let response: serde_json::Value = serde_json::from_str(&format_request(
			r#"{"source":"SELECT   1;","host":"sql"}"#,
		))
		.unwrap();
		assert_eq!(response["spans"], serde_json::json!([[0, 10]]));
	}

	#[test]
	fn errors_become_diagnostics() {
		let response =
			format_request(r#"{"source":"FROBNICATE;","host":"sql","options":{}}"#);
		assert!(response.contains("verbatim"), "{response}");
	}
}
