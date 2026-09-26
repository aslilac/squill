//! Format SQL embedded in host-language files, located via tree-sitter
//! queries (TREE-100).
//!
//! The extraction query is the config surface: captures named
//! `@sql.postgres` / `@sql.sqlite` (or bare `@sql`, which uses the
//! caller's dialect) mark the strings whose contents are SQL. Snippets
//! that cannot be formatted with confidence are left byte-identical and
//! reported as [`Warning`]s — a host file is never a hard error.
//!
//! Two capture shapes, by grammar:
//!
//! - **Literal** (the original built-ins: Rust, Go, Python, JS/TS,
//!   Gleam): the capture is the whole string literal, decoded and
//!   re-encoded by a hand-written codec per host.
//! - **Content** (C++, C#, Java, Kotlin, and every wasm grammar): the
//!   capture is the string's *content* node, between the delimiters.
//!   It is taken verbatim, formatted only when it already spans lines
//!   (proof the syntax allows raw newlines), and never when it holds a
//!   backslash the grammar might treat as an escape.
//!
//! Either way, every rewrite is checked by re-parsing the host file:
//! an edit that adds a syntax error, or whose string no longer comes
//! back from the query as exactly what was written, is dropped.
//!
//! Predicate note: only `#eq?`, `#not-eq?`, and `#any-of?` are
//! supported, keeping the no-regex rule — `#match?` is rejected up
//! front.

// A build with only some grammars leaves codec paths unused; the full
// build keeps every lint strict.
#![cfg_attr(
	not(all(
		feature = "rust",
		feature = "go",
		feature = "python",
		feature = "javascript",
		feature = "typescript",
		feature = "gleam",
		feature = "cpp",
		feature = "csharp",
		feature = "java",
		feature = "kotlin"
	)),
	allow(dead_code, unused_imports, unused_variables, unreachable_patterns)
)]

use formatter::Options;
use parser::Dialect;
use streaming_iterator::StreamingIterator;
use tree_sitter::Language;
use tree_sitter::Parser as TsParser;
use tree_sitter::Query;
use tree_sitter::QueryCursor;
use tree_sitter::QueryPredicateArg;
use tree_sitter::Tree;

/// The host languages with built-in grammars.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Host {
	#[cfg(feature = "rust")]
	Rust,
	#[cfg(feature = "go")]
	Go,
	#[cfg(feature = "python")]
	Python,
	#[cfg(feature = "javascript")]
	JavaScript,
	#[cfg(feature = "typescript")]
	TypeScript,
	/// TypeScript with JSX (`.tsx`) — a distinct grammar, same codec.
	#[cfg(feature = "typescript")]
	Tsx,
	#[cfg(feature = "gleam")]
	Gleam,
	#[cfg(feature = "cpp")]
	Cpp,
	#[cfg(feature = "csharp")]
	CSharp,
	#[cfg(feature = "java")]
	Java,
	#[cfg(feature = "kotlin")]
	Kotlin,
}

impl Host {
	/// Every built-in grammar this build carries.
	pub const ALL: &[Host] = &[
		#[cfg(feature = "rust")]
		Host::Rust,
		#[cfg(feature = "go")]
		Host::Go,
		#[cfg(feature = "python")]
		Host::Python,
		#[cfg(feature = "javascript")]
		Host::JavaScript,
		#[cfg(feature = "typescript")]
		Host::TypeScript,
		#[cfg(feature = "typescript")]
		Host::Tsx,
		#[cfg(feature = "gleam")]
		Host::Gleam,
		#[cfg(feature = "cpp")]
		Host::Cpp,
		#[cfg(feature = "csharp")]
		Host::CSharp,
		#[cfg(feature = "java")]
		Host::Java,
		#[cfg(feature = "kotlin")]
		Host::Kotlin,
	];

	/// The name a config file uses for this grammar.
	pub fn name(self) -> &'static str {
		match self {
			#[cfg(feature = "rust")]
			Host::Rust => "rust",
			#[cfg(feature = "go")]
			Host::Go => "go",
			#[cfg(feature = "python")]
			Host::Python => "python",
			#[cfg(feature = "javascript")]
			Host::JavaScript => "javascript",
			#[cfg(feature = "typescript")]
			Host::TypeScript => "typescript",
			#[cfg(feature = "typescript")]
			Host::Tsx => "tsx",
			#[cfg(feature = "gleam")]
			Host::Gleam => "gleam",
			#[cfg(feature = "cpp")]
			Host::Cpp => "cpp",
			#[cfg(feature = "csharp")]
			Host::CSharp => "csharp",
			#[cfg(feature = "java")]
			Host::Java => "java",
			#[cfg(feature = "kotlin")]
			Host::Kotlin => "kotlin",
		}
	}

	pub fn from_name(name: &str) -> Option<Host> {
		Host::ALL.iter().copied().find(|host| host.name() == name)
	}

	/// File extensions conventionally written in this language.
	pub fn extensions(self) -> &'static [&'static str] {
		match self {
			#[cfg(feature = "rust")]
			Host::Rust => &["rs"],
			#[cfg(feature = "go")]
			Host::Go => &["go"],
			#[cfg(feature = "python")]
			Host::Python => &["py"],
			#[cfg(feature = "javascript")]
			Host::JavaScript => &["js", "mjs", "cjs", "jsx"],
			#[cfg(feature = "typescript")]
			Host::TypeScript => &["ts", "mts", "cts"],
			#[cfg(feature = "typescript")]
			Host::Tsx => &["tsx"],
			#[cfg(feature = "gleam")]
			Host::Gleam => &["gleam"],
			#[cfg(feature = "cpp")]
			Host::Cpp => &["cc", "cpp", "cxx", "hh", "hpp", "hxx"],
			#[cfg(feature = "csharp")]
			Host::CSharp => &["cs"],
			#[cfg(feature = "java")]
			Host::Java => &["java"],
			#[cfg(feature = "kotlin")]
			Host::Kotlin => &["kt", "kts"],
		}
	}

	/// The extraction query used when a config names none.
	pub fn default_query(self) -> &'static str {
		match self {
			#[cfg(feature = "rust")]
			Host::Rust => RUST_SQLX_QUERY,
			#[cfg(feature = "go")]
			Host::Go => GO_DB_QUERY,
			#[cfg(feature = "python")]
			Host::Python => PYTHON_DB_QUERY,
			#[cfg(feature = "javascript")]
			Host::JavaScript => JS_SQL_QUERY,
			#[cfg(feature = "typescript")]
			Host::TypeScript | Host::Tsx => JS_SQL_QUERY,
			#[cfg(feature = "gleam")]
			Host::Gleam => GLEAM_SQL_QUERY,
			#[cfg(feature = "cpp")]
			Host::Cpp => CPP_SQL_QUERY,
			#[cfg(feature = "csharp")]
			Host::CSharp => CSHARP_SQL_QUERY,
			#[cfg(feature = "java")]
			Host::Java => JAVA_SQL_QUERY,
			#[cfg(feature = "kotlin")]
			Host::Kotlin => KOTLIN_SQL_QUERY,
		}
	}

	fn language(self) -> Language {
		match self {
			#[cfg(feature = "rust")]
			Host::Rust => tree_sitter_rust::LANGUAGE.into(),
			#[cfg(feature = "go")]
			Host::Go => tree_sitter_go::LANGUAGE.into(),
			#[cfg(feature = "python")]
			Host::Python => tree_sitter_python::LANGUAGE.into(),
			#[cfg(feature = "javascript")]
			Host::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
			#[cfg(feature = "typescript")]
			Host::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
			#[cfg(feature = "typescript")]
			Host::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
			#[cfg(feature = "gleam")]
			Host::Gleam => tree_sitter_gleam::LANGUAGE.into(),
			#[cfg(feature = "cpp")]
			Host::Cpp => tree_sitter_cpp::LANGUAGE.into(),
			#[cfg(feature = "csharp")]
			Host::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
			#[cfg(feature = "java")]
			Host::Java => tree_sitter_java::LANGUAGE.into(),
			#[cfg(feature = "kotlin")]
			Host::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
		}
	}

	/// Does code in this language conventionally write JDBC-style `?`
	/// placeholders? Callers use it as the default for
	/// `Options::question_params` when nothing is configured.
	pub fn uses_question_params(self) -> bool {
		match self {
			#[cfg(feature = "java")]
			Host::Java => true,
			#[cfg(feature = "kotlin")]
			Host::Kotlin => true,
			_ => false,
		}
	}

	/// Does code in this language conventionally write psycopg-style
	/// `%s` / `%(name)s` placeholders? Callers use it as the default for
	/// `Options::pyformat_params` when nothing is configured.
	pub fn uses_pyformat_params(self) -> bool {
		match self {
			#[cfg(feature = "python")]
			Host::Python => true,
			_ => false,
		}
	}

	fn codec(self) -> Codec {
		match self {
			#[cfg(feature = "rust")]
			Host::Rust => Codec::Literal,
			#[cfg(feature = "go")]
			Host::Go => Codec::Literal,
			#[cfg(feature = "python")]
			Host::Python => Codec::Literal,
			#[cfg(feature = "javascript")]
			Host::JavaScript => Codec::Literal,
			#[cfg(feature = "typescript")]
			Host::TypeScript | Host::Tsx => Codec::Literal,
			#[cfg(feature = "gleam")]
			Host::Gleam => Codec::Literal,
			// Raw strings: a backslash is just a backslash.
			#[cfg(feature = "cpp")]
			Host::Cpp => Codec::Content { backslash_escapes: false },
			#[cfg(feature = "csharp")]
			Host::CSharp => Codec::Content { backslash_escapes: false },
			#[cfg(feature = "kotlin")]
			Host::Kotlin => Codec::Content { backslash_escapes: false },
			// Text blocks process escapes.
			#[cfg(feature = "java")]
			Host::Java => Codec::Content { backslash_escapes: true },
		}
	}
}

/// How captured strings are read and written back.
#[derive(Clone, Copy)]
#[cfg_attr(
	not(any(
		feature = "cpp",
		feature = "csharp",
		feature = "java",
		feature = "kotlin",
		feature = "wasm"
	)),
	allow(dead_code)
)]
enum Codec {
	/// The capture is a whole literal; decoded by the host's codec.
	Literal,
	/// The capture is the content between the delimiters, verbatim.
	/// When the grammar may treat backslashes as escapes, a snippet
	/// holding one is left alone.
	Content { backslash_escapes: bool },
}

/// A tree-sitter grammar to find SQL with: built in, or (with the
/// `wasm` feature) loaded from a `.wasm` file at runtime.
#[derive(Clone)]
pub enum Grammar {
	Builtin(Host),
	#[cfg(feature = "wasm")]
	Wasm(wasm::WasmGrammar),
}

impl From<Host> for Grammar {
	fn from(host: Host) -> Self {
		Grammar::Builtin(host)
	}
}

impl Grammar {
	/// The grammar's name, for messages.
	pub fn name(&self) -> &str {
		match self {
			Grammar::Builtin(host) => host.name(),
			#[cfg(feature = "wasm")]
			Grammar::Wasm(grammar) => grammar.name(),
		}
	}

	fn codec(&self) -> Codec {
		match self {
			Grammar::Builtin(host) => host.codec(),
			// Nothing is known about a loaded grammar's strings.
			#[cfg(feature = "wasm")]
			Grammar::Wasm(_) => Codec::Content { backslash_escapes: true },
		}
	}

	fn host(&self) -> Option<Host> {
		match self {
			Grammar::Builtin(host) => Some(*host),
			#[cfg(feature = "wasm")]
			Grammar::Wasm(_) => None,
		}
	}

	/// Run `f` with a parser set to this grammar.
	fn with_parser<R>(
		&self,
		f: impl FnOnce(&mut TsParser, &Language) -> Result<R, EmbedError>,
	) -> Result<R, EmbedError> {
		match self {
			Grammar::Builtin(host) => {
				let language = host.language();
				let mut parser = TsParser::new();
				parser
					.set_language(&language)
					.map_err(|err| EmbedError::Grammar(err.to_string()))?;
				f(&mut parser, &language)
			}
			#[cfg(feature = "wasm")]
			Grammar::Wasm(grammar) => grammar.with_parser(f),
		}
	}
}

#[cfg(feature = "wasm")]
pub mod wasm {
	//! Grammars compiled to wasm (`tree-sitter build --wasm`), run by
	//! tree-sitter's wasmtime integration.

	use std::cell::RefCell;
	use std::collections::HashMap;
	use std::path::Path;
	use std::sync::Arc;
	use std::sync::OnceLock;

	use tree_sitter::Language;
	use tree_sitter::Parser as TsParser;
	use tree_sitter::WasmStore;
	use tree_sitter::wasmtime::Engine;

	use super::EmbedError;

	/// A wasm grammar's bytes and the name its exports carry.
	#[derive(Clone)]
	pub struct WasmGrammar {
		name: String,
		/// Identifies the grammar in the per-thread parser cache.
		key: String,
		bytes: Arc<[u8]>,
	}

	fn engine() -> &'static Engine {
		static ENGINE: OnceLock<Engine> = OnceLock::new();
		ENGINE.get_or_init(Engine::default)
	}

	thread_local! {
		/// A store compiles its grammar once, so each thread keeps a
		/// ready parser per grammar rather than recompiling per file.
		static PARSERS: RefCell<HashMap<String, (TsParser, Language)>> =
			RefCell::new(HashMap::new());
	}

	impl WasmGrammar {
		/// Load a grammar built by `tree-sitter build --wasm`. The
		/// language name comes from the file name, the way that command
		/// writes it: `tree-sitter-c_sharp.wasm` holds `c_sharp`.
		pub fn load(path: &Path) -> Result<Self, EmbedError> {
			let bytes = std::fs::read(path).map_err(|err| {
				EmbedError::Grammar(format!("{}: {err}", path.display()))
			})?;
			let stem =
				path.file_stem().and_then(|stem| stem.to_str()).ok_or_else(|| {
					EmbedError::Grammar(format!(
						"{}: cannot name a grammar from this file name",
						path.display()
					))
				})?;
			let name =
				stem.strip_prefix("tree-sitter-").unwrap_or(stem).replace('-', "_");
			let grammar = WasmGrammar {
				name,
				key: path.display().to_string(),
				bytes: bytes.into(),
			};
			// Fail at load time, not on the first file that uses it.
			grammar.with_parser(|_, _| Ok(())).map_err(|err| match err {
				EmbedError::Grammar(message) => {
					EmbedError::Grammar(format!("{}: {message}", path.display()))
				}
				other => other,
			})?;
			Ok(grammar)
		}

		pub fn name(&self) -> &str {
			&self.name
		}

		pub(super) fn with_parser<R>(
			&self,
			f: impl FnOnce(&mut TsParser, &Language) -> Result<R, EmbedError>,
		) -> Result<R, EmbedError> {
			PARSERS.with(|parsers| {
				let mut parsers = parsers.borrow_mut();
				if !parsers.contains_key(&self.key) {
					let mut store = WasmStore::new(engine())
						.map_err(|err| EmbedError::Grammar(err.message))?;
					let language = store
						.load_language(&self.name, &self.bytes)
						.map_err(|err| EmbedError::Grammar(err.message))?;
					let mut parser = TsParser::new();
					parser
						.set_wasm_store(store)
						.map_err(|err| EmbedError::Grammar(err.to_string()))?;
					parser
						.set_language(&language)
						.map_err(|err| EmbedError::Grammar(err.to_string()))?;
					parsers.insert(self.key.clone(), (parser, language));
				}
				let (parser, language) =
					parsers.get_mut(&self.key).expect("inserted above");
				f(parser, language)
			})
		}
	}
}

/// Default extraction query for Rust: the string-literal argument of
/// sqlx's `query!` / `query_as!` / `query_scalar!` / `query_unchecked!`
/// macros, and the first argument of its `query` / `query_as` /
/// `query_scalar` functions (any path whose last segment matches).
#[cfg(feature = "rust")]
pub const RUST_SQLX_QUERY: &str = r#"
((macro_invocation
   macro: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
   ]
   (token_tree
     [(string_literal) (raw_string_literal)] @sql))
 (#any-of? @_name "query" "query_as" "query_scalar" "query_unchecked"))

((call_expression
   function: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
     (generic_function function: [
       (identifier) @_name
       (scoped_identifier name: (identifier) @_name)
     ])
   ]
   arguments: (arguments . [(string_literal) (raw_string_literal)] @sql))
 (#any-of? @_name "query" "query_as" "query_scalar"))
"#;

/// Default extraction query for Go: string arguments of `.Query`-family
/// method calls (`database/sql` style).
#[cfg(feature = "go")]
pub const GO_DB_QUERY: &str = r#"
((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list
     [(raw_string_literal) (interpreted_string_literal)] @sql))
 (#any-of? @_method
   "Query" "QueryRow" "Exec"
   "QueryContext" "QueryRowContext" "ExecContext"))
"#;

/// Default extraction query for Python: the first string argument of
/// `.execute`-family method calls (sqlite3 / psycopg / asyncpg style)
/// and of SQLAlchemy's `text(...)`.
#[cfg(feature = "python")]
pub const PYTHON_DB_QUERY: &str = r#"
((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list . (string) @sql))
 (#any-of? @_method
   "execute" "executemany" "executescript"
   "fetch" "fetchrow" "fetchval"))

((call
   function: (identifier) @_fn
   arguments: (argument_list . (string) @sql))
 (#eq? @_fn "text"))
"#;

/// Default extraction query for JavaScript/TypeScript: the first string
/// or template-literal argument of `.query` / `.execute` / `.prepare`
/// method calls (pg, mysql2, better-sqlite3 style), plus `sql`-tagged
/// template literals (postgres.js style).
#[cfg(any(feature = "javascript", feature = "typescript"))]
pub const JS_SQL_QUERY: &str = r#"
((call_expression
   function: (member_expression property: (property_identifier) @_method)
   arguments: (arguments . [(string) (template_string)] @sql))
 (#any-of? @_method "query" "execute" "prepare"))

((call_expression
   function: (identifier) @_tag
   arguments: (template_string) @sql)
 (#eq? @_tag "sql"))
"#;

/// Default extraction query for Gleam: the first string argument of
/// `query` / `exec` / `execute` calls, module-qualified (`sqlight.query`,
/// `pog.query`) or bare. `sqlight` is a SQLite library, so its calls
/// carry that dialect; everything else uses the session dialect.
#[cfg(feature = "gleam")]
pub const GLEAM_SQL_QUERY: &str = r#"
((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql.sqlite)))
 (#eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql)))
 (#not-eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((function_call
   function: (identifier) @_fn
   arguments: (arguments . (argument value: (string) @sql)))
 (#any-of? @_fn "query" "exec" "execute"))
"#;

/// Default extraction query for C++: raw-string (`R"(...)"`) arguments
/// of sqlite3, libpq, and libpqxx calls. The C APIs name their dialect;
/// pqxx-style `exec`/`query` calls use the configured one.
#[cfg(feature = "cpp")]
pub const CPP_SQL_QUERY: &str = r#"
((call_expression
   function: (identifier) @_fn
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql.sqlite)))
 (#any-of? @_fn
   "sqlite3_prepare" "sqlite3_prepare_v2" "sqlite3_prepare_v3" "sqlite3_exec"))

((call_expression
   function: (identifier) @_fn
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql.postgres)))
 (#any-of? @_fn "PQexec" "PQexecParams" "PQprepare" "PQsendQuery"))

((call_expression
   function: [
     (field_expression field: (field_identifier) @_fn)
     (qualified_identifier name: (identifier) @_fn)
   ]
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql)))
 (#any-of? @_fn
   "exec" "exec0" "exec1" "exec_n" "exec_params" "exec_params0"
   "exec_params1" "exec_prepared" "prepare" "query" "query_value"))
"#;

/// Default extraction query for C#: raw-string (`"""`) arguments of EF
/// Core migrations and raw-SQL calls, ADO.NET's `CommandText`, and
/// Dapper's query/execute family. Interpolated raw strings (`$"""`) are
/// a different node and never match.
#[cfg(feature = "csharp")]
pub const CSHARP_SQL_QUERY: &str = r#"
((invocation_expression
   function: (member_access_expression name: (identifier) @_method)
   arguments: (argument_list
     (argument (raw_string_literal (raw_string_content) @sql))))
 (#any-of? @_method
   "Sql" "ExecuteSql" "ExecuteSqlAsync" "ExecuteSqlRaw" "ExecuteSqlRawAsync"
   "FromSql" "FromSqlRaw" "SqlQuery" "SqlQueryRaw"
   "Query" "QueryAsync" "QueryFirst" "QueryFirstAsync"
   "QueryFirstOrDefault" "QueryFirstOrDefaultAsync"
   "QuerySingle" "QuerySingleAsync"
   "QuerySingleOrDefault" "QuerySingleOrDefaultAsync"
   "QueryMultiple" "QueryMultipleAsync"
   "Execute" "ExecuteAsync" "ExecuteScalar" "ExecuteScalarAsync"
   "ExecuteReader" "ExecuteReaderAsync"))

((assignment_expression
   left: (member_access_expression name: (identifier) @_prop)
   right: (raw_string_literal (raw_string_content) @sql))
 (#eq? @_prop "CommandText"))
"#;

/// Default extraction query for Java: text-block (`"""`) arguments of
/// JDBC, JPA, and Spring `JdbcTemplate` calls. The whole literal is
/// captured — the grammar has no node spanning a text block's content —
/// and one holding an escape is reported, not rewritten.
#[cfg(feature = "java")]
pub const JAVA_SQL_QUERY: &str = r#"
((method_invocation
   name: (identifier) @_method
   arguments: (argument_list (string_literal) @sql))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "executeLargeUpdate" "execute" "addBatch"
   "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "queryForRowSet" "queryForStream" "update" "batchUpdate"))
"#;

/// Default extraction query for Kotlin: raw-string (`"""`) arguments of
/// JDBC, Spring, and Exposed calls. A raw string with `$` templates has
/// several content nodes and never matches.
#[cfg(feature = "kotlin")]
pub const KOTLIN_SQL_QUERY: &str = r#"
((call_expression
   [
     (identifier) @_method
     (navigation_expression (identifier) @_method .)
   ]
   (value_arguments
     (value_argument
       (multiline_string_literal . (string_content) @sql .))))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "execute" "addBatch" "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "update" "batchUpdate" "exec"))
"#;

/// Where an embedded snippet takes its indent character from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indent {
	/// Match the host file's own indentation, so a spaces-indented file
	/// never gains tabs. The default when nothing is configured.
	#[default]
	FromHost,
	/// Use `Options::indent_style` as given — the caller configured an
	/// indent style explicitly and means it.
	Configured,
}

#[derive(Debug)]
pub enum EmbedError {
	/// The host source did not parse with the tree-sitter grammar.
	HostParse,
	/// The extraction query is invalid (or uses an unsupported predicate
	/// such as `#match?`).
	Query(String),
	/// A grammar could not be loaded.
	Grammar(String),
}

impl std::fmt::Display for EmbedError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			EmbedError::HostParse => f.write_str("host file did not parse"),
			EmbedError::Query(message) => {
				write!(f, "invalid extraction query: {message}")
			}
			EmbedError::Grammar(message) => {
				write!(f, "cannot load grammar: {message}")
			}
		}
	}
}

impl std::error::Error for EmbedError {}

/// A host file with its embedded SQL formatted.
#[derive(Debug)]
pub struct Embedded {
	pub text: String,
	/// Captured strings left untouched for a reason worth reporting.
	pub warnings: Vec<Warning>,
}

/// A captured string that was left byte-identical, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
	/// Byte offset of the capture in the original source.
	pub offset: usize,
	pub message: String,
}

/// One planned rewrite: the capture's byte range in the original
/// source and what it becomes.
struct Edit {
	range: std::ops::Range<usize>,
	replacement: String,
}

/// Build an extraction query, refusing what squill does not evaluate.
fn compile_query(
	language: &Language,
	query_source: &str,
) -> Result<Query, EmbedError> {
	// Reject regex predicates up front (the no-regex rule). The
	// tree-sitter binding would happily evaluate them, so refuse by
	// inspection before building the query.
	if query_source.contains("#match?") || query_source.contains("#not-match?") {
		return Err(EmbedError::Query(
			"regex predicates (#match?) are not supported; use #eq? / #any-of?"
				.to_string(),
		));
	}
	let query = Query::new(language, query_source)
		.map_err(|err| EmbedError::Query(err.to_string()))?;
	// Anything the binding does not evaluate natively would be silently
	// ignored: reject unknown custom predicates too.
	for pattern in 0..query.pattern_count() {
		for predicate in query.general_predicates(pattern) {
			match predicate.operator.as_ref() {
				"eq?" | "not-eq?" | "any-of?" => {}
				other => {
					return Err(EmbedError::Query(format!(
						"unsupported predicate `#{other}?`"
					)));
				}
			}
		}
	}
	Ok(query)
}

/// How many SQL strings the query finds in `source`, without formatting
/// any — for surveying a project (`squill init`).
pub fn count_sql(
	source: &str,
	grammar: &Grammar,
	query_source: &str,
) -> Result<usize, EmbedError> {
	grammar.with_parser(|ts, language| {
		let query = compile_query(language, query_source)?;
		let tree = ts.parse(source, None).ok_or(EmbedError::HostParse)?;
		let extraction = Extraction {
			query: &query,
			default_dialect: Dialect::default(),
			codec: grammar.codec(),
		};
		Ok(extraction.captures(&tree, source).len())
	})
}

/// Format every SQL snippet the query captures in `source`, returning the
/// rewritten host file. Unparsable or unsafe snippets stay byte-exact
/// and come back as warnings. `indent` decides whether
/// `options.indent_style` applies or the host file's own indentation
/// wins.
pub fn format_embedded(
	source: &str,
	grammar: &Grammar,
	query_source: &str,
	options: &Options,
	indent: Indent,
) -> Result<Embedded, EmbedError> {
	grammar.with_parser(|ts, language| {
		let query = compile_query(language, query_source)?;
		let tree = ts.parse(source, None).ok_or(EmbedError::HostParse)?;

		let mut edits: Vec<Edit> = Vec::new();
		let mut warnings: Vec<Warning> = Vec::new();
		let extraction = Extraction {
			query: &query,
			default_dialect: options.dialect,
			codec: grammar.codec(),
		};
		for (node_range, dialect) in extraction.captures(&tree, source) {
			let snippet = Snippet {
				source,
				range: node_range.clone(),
				grammar,
				dialect,
				options,
				indent,
			};
			match snippet.rewrite() {
				Rewrite::Skip => {}
				Rewrite::Warn(message) => {
					warnings.push(Warning { offset: node_range.start, message });
				}
				Rewrite::Replace(replacement) => {
					if replacement != source[node_range.clone()] {
						edits.push(Edit { range: node_range, replacement });
					}
				}
			}
		}

		// Duplicates (two patterns, one string) and overlaps (a capture
		// inside another) keep the first, outermost edit.
		edits.sort_by_key(|edit| {
			(edit.range.start, std::cmp::Reverse(edit.range.end))
		});
		let mut kept: Vec<Edit> = Vec::with_capacity(edits.len());
		for edit in edits {
			if kept.last().is_some_and(|last| edit.range.start < last.range.end) {
				continue;
			}
			kept.push(edit);
		}
		let mut edits = kept;

		// The host-side safety check: re-parse and make sure every edit
		// reads back as written, without new syntax errors. An edit that
		// fails is dropped and the rest are checked again.
		let baseline_errors = error_count(&tree);
		loop {
			let text = apply(source, &edits);
			let failed =
				extraction.verify(ts, &text, &edits, baseline_errors, source)?;
			if failed.is_empty() {
				warnings.sort_by_key(|warning| warning.offset);
				return Ok(Embedded { text, warnings });
			}
			for index in failed.into_iter().rev() {
				let edit = edits.remove(index);
				warnings.push(Warning {
					offset: edit.range.start,
					message: format!(
						"formatting this string would change how the {} file \
						 parses; left unformatted",
						grammar.name()
					),
				});
			}
		}
	})
}

/// How to find SQL in a parsed host file.
struct Extraction<'a> {
	query: &'a Query,
	/// The dialect of a bare `@sql` capture.
	default_dialect: Dialect,
	codec: Codec,
}

impl Extraction<'_> {
	/// Every SQL capture in `tree`: the byte range to rewrite, and the
	/// dialect.
	fn captures(
		&self,
		tree: &Tree,
		source: &str,
	) -> Vec<(std::ops::Range<usize>, Dialect)> {
		let query = self.query;
		let mut out = Vec::new();
		let mut cursor = QueryCursor::new();
		let mut matches =
			cursor.matches(query, tree.root_node(), source.as_bytes());
		while let Some(query_match) = matches.next() {
			if !predicates_hold(query, query_match, source) {
				continue;
			}
			for capture in query_match.captures {
				let name = &query.capture_names()[capture.index as usize];
				if let Some(dialect) = sql_dialect(name, self.default_dialect) {
					let range = match self.codec {
						Codec::Literal => capture.node.byte_range(),
						Codec::Content { .. } => content_range(capture.node),
					};
					out.push((range, dialect));
				}
			}
		}
		out
	}

	/// Indices of the `edits` (to `source`) that do not survive a
	/// re-parse of `text`, their result.
	fn verify(
		&self,
		ts: &mut TsParser,
		text: &str,
		edits: &[Edit],
		baseline_errors: usize,
		source: &str,
	) -> Result<Vec<usize>, EmbedError> {
		if edits.is_empty() {
			return Ok(Vec::new());
		}
		let tree = ts.parse(text, None).ok_or(EmbedError::HostParse)?;
		let captures: std::collections::HashSet<std::ops::Range<usize>> =
			self.captures(&tree, text).into_iter().map(|(range, _)| range).collect();
		let mut failed = Vec::new();
		let mut shift: isize = 0;
		for (index, edit) in edits.iter().enumerate() {
			let start = (edit.range.start as isize + shift) as usize;
			let end = start + edit.replacement.len();
			if !captures.contains(&(start..end)) {
				failed.push(index);
			}
			shift += edit.replacement.len() as isize - edit.range.len() as isize;
		}
		if failed.is_empty() && error_count(&tree) > baseline_errors {
			// A new syntax error somewhere: find which edits cause one on
			// their own. Rare, so one re-parse per edit is fine.
			for (index, edit) in edits.iter().enumerate() {
				let alone = apply(source, std::slice::from_ref(edit));
				let tree = ts.parse(&alone, None).ok_or(EmbedError::HostParse)?;
				if error_count(&tree) > baseline_errors {
					failed.push(index);
				}
			}
			if failed.is_empty() {
				// Only the combination errs; trust none of it.
				failed.extend(0..edits.len());
			}
		}
		Ok(failed)
	}
}

/// The content of a content-codec capture. A node whose first and last
/// children are anonymous tokens is a whole literal, and its content
/// lies between those delimiters (Java's text blocks have no node that
/// spans their content); any other node is the content itself.
fn content_range(node: tree_sitter::Node<'_>) -> std::ops::Range<usize> {
	let count = node.child_count();
	if count >= 2
		&& let (Some(first), Some(last)) =
			(node.child(0), node.child(count as u32 - 1))
		&& !first.is_named()
		&& !last.is_named()
	{
		return first.end_byte()..last.start_byte();
	}
	node.byte_range()
}

/// `source` with `edits` (sorted, non-overlapping) applied.
fn apply(source: &str, edits: &[Edit]) -> String {
	let mut out = String::with_capacity(source.len());
	let mut at = 0;
	for edit in edits {
		out.push_str(&source[at..edit.range.start]);
		out.push_str(&edit.replacement);
		at = edit.range.end;
	}
	out.push_str(&source[at..]);
	out
}

/// ERROR and MISSING nodes in `tree`.
fn error_count(tree: &Tree) -> usize {
	if !tree.root_node().has_error() {
		return 0;
	}
	let mut count = 0;
	let mut cursor = tree.walk();
	loop {
		let node = cursor.node();
		if node.is_error() || node.is_missing() {
			count += 1;
		}
		if node.has_error() && cursor.goto_first_child() {
			continue;
		}
		while !cursor.goto_next_sibling() {
			if !cursor.goto_parent() {
				return count;
			}
		}
	}
}

/// `sql` / `sql.postgres` / `sql.sqlite` capture names carry the dialect.
fn sql_dialect(capture_name: &str, default: Dialect) -> Option<Dialect> {
	match capture_name {
		"sql" => Some(default),
		"sql.postgres" => Some(Dialect::Postgres),
		"sql.sqlite" => Some(Dialect::Sqlite),
		_ => None,
	}
}

fn predicates_hold(
	query: &Query,
	query_match: &tree_sitter::QueryMatch<'_, '_>,
	source: &str,
) -> bool {
	let capture_text = |index: u32| {
		query_match
			.captures
			.iter()
			.find(|c| c.index == index)
			.map(|c| &source[c.node.byte_range()])
	};
	query.general_predicates(query_match.pattern_index).iter().all(|predicate| {
		let mut args = predicate.args.iter();
		let Some(QueryPredicateArg::Capture(capture)) = args.next() else {
			return false;
		};
		let Some(text) = capture_text(*capture) else {
			return false;
		};
		match predicate.operator.as_ref() {
			"eq?" => args.next().is_some_and(|arg| match arg {
				QueryPredicateArg::String(s) => s.as_ref() == text,
				QueryPredicateArg::Capture(other) => capture_text(*other) == Some(text),
			}),
			"not-eq?" => args.next().is_some_and(|arg| match arg {
				QueryPredicateArg::String(s) => s.as_ref() != text,
				QueryPredicateArg::Capture(other) => capture_text(*other) != Some(text),
			}),
			"any-of?" => args.any(
				|arg| matches!(arg, QueryPredicateArg::String(s) if s.as_ref() == text),
			),
			_ => false,
		}
	})
}

/// What to do with one captured string.
enum Rewrite {
	/// Not a candidate (single-line syntax, interpolation, empty):
	/// untouched, by design, without comment.
	Skip,
	/// A candidate that could not be formatted with confidence.
	Warn(String),
	Replace(String),
}

/// One captured string and everything needed to rewrite it.
struct Snippet<'a> {
	source: &'a str,
	range: std::ops::Range<usize>,
	grammar: &'a Grammar,
	dialect: Dialect,
	options: &'a Options,
	indent: Indent,
}

impl Snippet<'_> {
	fn rewrite(&self) -> Rewrite {
		let text = &self.source[self.range.clone()];
		match self.grammar.codec() {
			Codec::Literal => {
				let Some(host) = self.grammar.host() else {
					return Rewrite::Skip;
				};
				let decoded = match decode(host, text) {
					Ok(decoded) => decoded,
					Err(rewrite) => return rewrite,
				};
				if decoded.content.trim().is_empty() {
					return Rewrite::Skip;
				}
				// Only multiline string *syntaxes* are formatted: raw
				// strings (`r#"..."#`, Go backticks), Python triple quotes,
				// JS templates, and Gleam strings natively support
				// multiple lines, so they always take the vertical shape.
				// A plain Rust or Go string joins them once it already
				// holds a line break (written, escaped, or continued), and
				// is rewritten as a raw string; a single-line one never
				// reformats.
				let multiline = match decoded.kind {
					LiteralKind::RustRaw { .. }
					| LiteralKind::GoRaw
					| LiteralKind::PyTriple { .. }
					| LiteralKind::JsTemplate
					| LiteralKind::GleamString => true,
					LiteralKind::RustPlain | LiteralKind::GoPlain => {
						text.contains('\n') || decoded.content.contains('\n')
					}
				};
				if !multiline {
					return Rewrite::Skip;
				}
				let anchored = match self.format(&decoded.content) {
					Ok(anchored) => anchored,
					Err(rewrite) => return rewrite,
				};
				match encode(&decoded, &anchored) {
					Some(literal) => Rewrite::Replace(literal),
					None => Rewrite::Warn(
						"formatted SQL cannot be written back into this string \
						 literal; left unformatted"
							.to_string(),
					),
				}
			}
			Codec::Content { backslash_escapes } => {
				if text.trim().is_empty() || !text.contains('\n') {
					// Only a string that already spans lines proves the
					// syntax takes raw newlines.
					return Rewrite::Skip;
				}
				if backslash_escapes && text.contains('\\') {
					return Rewrite::Warn(
						"string holds a backslash escape; left unformatted".to_string(),
					);
				}
				match self.format(text) {
					Ok(anchored) => Rewrite::Replace(anchored),
					Err(rewrite) => rewrite,
				}
			}
		}
	}

	/// Format `sql` into the vertical shape: SQL starting on the line
	/// after the opening quote, each line anchored to the host
	/// statement's indentation, and the closing quote on its own line at
	/// that indent.
	fn format(&self, sql: &str) -> Result<String, Rewrite> {
		// The host statement's own indentation: the anchor every SQL line
		// hangs off, and — unless an indent style was configured — the
		// indent character too, so continuation lines don't mix tabs into
		// a spaces-indented file (or vice versa).
		let host_indent = line_indent(self.source, self.range.start);
		let mut format_options = *self.options;
		format_options.dialect = self.dialect;
		// The author chose a multi-line literal: keep statements
		// clause-per-line, never collapsed onto one line.
		format_options.always_break_statements = true;
		if self.indent == Indent::FromHost {
			format_options.indent_style =
				host_indent_style(self.source, &host_indent);
		}

		let lex_options = format_options.lex_options();
		let tokens = parser::lexer::lex_with(sql, self.dialect, lex_options);
		let parse = parser::parser::parse(&tokens, self.dialect);
		if let Some(diagnostic) = parse.diagnostics.first() {
			return Err(Rewrite::Warn(format!(
				"embedded SQL did not parse ({}); left unformatted",
				diagnostic.message
			)));
		}
		let formatted = formatter::format_cst(&parse.cst, &format_options);
		if formatted.fallback_statements > 0 {
			return Err(Rewrite::Warn(
				"embedded SQL failed the formatter's self-check; left unformatted"
					.to_string(),
			));
		}
		let sql = formatted.text.trim_end();

		// Lines that start inside a value (a string spanning lines) are
		// data: never indented.
		let verbatim_lines = formatter::verbatim_line_starts(sql, &format_options);
		let mut anchored = String::new();
		let mut line_start = 0;
		for line in sql.split('\n') {
			anchored.push('\n');
			if !line.is_empty() && !verbatim_lines.contains(&line_start) {
				anchored.push_str(&host_indent);
			}
			anchored.push_str(line);
			line_start += line.len() + 1;
		}
		anchored.push('\n');
		anchored.push_str(&host_indent);
		Ok(anchored)
	}
}

/// The indent character a host file uses: the anchor line's, or — for
/// a string that opens at column 0 — the first indented line's in the
/// file. Tabs when the file has no indentation at all.
fn host_indent_style(
	source: &str,
	host_indent: &str,
) -> formatter::IndentStyle {
	let sample = if host_indent.is_empty() {
		source
			.lines()
			.find_map(|line| line.chars().next().filter(|c| *c == ' ' || *c == '\t'))
	} else {
		host_indent.chars().next()
	};
	match sample {
		Some(' ') => formatter::IndentStyle::Spaces,
		_ => formatter::IndentStyle::Tab,
	}
}

/// Leading whitespace of the line containing `offset`.
fn line_indent(source: &str, offset: usize) -> String {
	let line_start = source[..offset].rfind('\n').map_or(0, |pos| pos + 1);
	source[line_start..].chars().take_while(|&c| c == ' ' || c == '\t').collect()
}

/// A decoded string literal: its SQL content plus enough shape to
/// re-encode.
struct Decoded {
	content: String,
	kind: LiteralKind,
}

enum LiteralKind {
	/// Rust `"..."` (escapes) — rewritten as a raw string once
	/// multi-line.
	RustPlain,
	/// Rust `r#"..."#` with N hashes — content is verbatim.
	RustRaw { hashes: usize },
	/// Go `` `...` `` — verbatim, but cannot contain a backtick.
	GoRaw,
	/// Go `"..."` (escapes) — rewritten as a raw string once
	/// multi-line.
	GoPlain,
	/// Python `'''...'''` / `"""..."""`, optionally r-prefixed.
	PyTriple { raw: bool, quote: char },
	/// JS/TS `` `...` `` template literal without substitutions.
	JsTemplate,
	/// Gleam `"..."` — escapes, but literal newlines are allowed.
	GleamString,
}

/// Decode a captured literal. `Err` carries what to do instead: skip a
/// literal that is not a candidate by design (f-strings, `${}`
/// templates, single-quoted syntaxes), warn about one that is but
/// cannot be read with confidence (an escape the codec does not know).
fn decode(host: Host, literal: &str) -> Result<Decoded, Rewrite> {
	let unknown_escape = || {
		Rewrite::Warn(
			"string holds an escape sequence squill cannot decode; left \
			 unformatted"
				.to_string(),
		)
	};
	// A capture that is not the literal shape the codec expects (a
	// custom query aiming at the wrong node) is not ours to touch.
	let strip =
		|text: &'_ str, prefix: &str, suffix: &str| -> Result<String, Rewrite> {
			text
				.strip_prefix(prefix)
				.and_then(|rest| rest.strip_suffix(suffix))
				.map(str::to_string)
				.ok_or(Rewrite::Skip)
		};
	match host {
		#[cfg(feature = "rust")]
		Host::Rust => {
			if let Some(rest) = literal.strip_prefix('r') {
				let hashes = rest.chars().take_while(|&c| c == '#').count();
				let fence = "#".repeat(hashes);
				let body = strip(&rest[hashes..], "\"", &format!("\"{fence}"))?;
				Ok(Decoded { content: body, kind: LiteralKind::RustRaw { hashes } })
			} else {
				let body = strip(literal, "\"", "\"")?;
				Ok(Decoded {
					content: unescape(&body, EscapeMode::Rust)
						.ok_or_else(unknown_escape)?,
					kind: LiteralKind::RustPlain,
				})
			}
		}
		#[cfg(feature = "go")]
		Host::Go => {
			if literal.starts_with('`') {
				Ok(Decoded {
					content: strip(literal, "`", "`")?,
					kind: LiteralKind::GoRaw,
				})
			} else {
				let body = strip(literal, "\"", "\"")?;
				Ok(Decoded {
					content: unescape(&body, EscapeMode::Go)
						.ok_or_else(unknown_escape)?,
					kind: LiteralKind::GoPlain,
				})
			}
		}
		#[cfg(feature = "python")]
		Host::Python => {
			let prefix_len =
				literal.chars().take_while(|c| c.is_ascii_alphabetic()).count();
			let prefix = literal[..prefix_len].to_ascii_lowercase();
			if prefix.contains('f') || prefix.contains('b') {
				// f-strings interpolate (SQL with holes) and bytes
				// literals are not SQL text: never touched.
				return Err(Rewrite::Skip);
			}
			let raw = prefix.contains('r');
			let rest = &literal[prefix_len..];
			for fence in ["'''", "\"\"\""] {
				if let Ok(body) = strip(rest, fence, fence) {
					let content = if raw {
						body
					} else {
						unescape(&body, EscapeMode::Go).ok_or_else(unknown_escape)?
					};
					let quote = if fence.starts_with('\'') { '\'' } else { '"' };
					return Ok(Decoded {
						content,
						kind: LiteralKind::PyTriple { raw, quote },
					});
				}
			}
			// Single-quoted syntax: single-line territory, untouched.
			Err(Rewrite::Skip)
		}
		#[cfg(feature = "javascript")]
		Host::JavaScript => decode_js(literal),
		#[cfg(feature = "typescript")]
		Host::TypeScript | Host::Tsx => decode_js(literal),
		#[cfg(feature = "gleam")]
		Host::Gleam => {
			let body = strip(literal, "\"", "\"")?;
			Ok(Decoded {
				content: unescape(&body, EscapeMode::Gleam)
					.ok_or_else(unknown_escape)?,
				kind: LiteralKind::GleamString,
			})
		}
		// Content-codec grammars never decode a whole literal.
		#[allow(unreachable_patterns)]
		_ => Err(Rewrite::Skip),
	}
}

/// Decode a JS/TS literal. Only template literals; `${}` substitutions
/// are SQL with holes and stay byte-identical. Plain '...'/"..."
/// strings are single-line syntax, also untouched.
fn decode_js(literal: &str) -> Result<Decoded, Rewrite> {
	let Some(body) =
		literal.strip_prefix('`').and_then(|rest| rest.strip_suffix('`'))
	else {
		return Err(Rewrite::Skip);
	};
	if has_template_substitution(body) {
		return Err(Rewrite::Skip);
	}
	let content = unescape(body, EscapeMode::Js).ok_or_else(|| {
		Rewrite::Warn(
			"string holds an escape sequence squill cannot decode; left \
			 unformatted"
				.to_string(),
		)
	})?;
	Ok(Decoded { content, kind: LiteralKind::JsTemplate })
}

/// Does a template-literal body contain an unescaped `${`?
fn has_template_substitution(body: &str) -> bool {
	let mut chars = body.chars().peekable();
	while let Some(c) = chars.next() {
		match c {
			'\\' => {
				chars.next();
			}
			'$' if chars.peek() == Some(&'{') => return true,
			_ => {}
		}
	}
	false
}

/// Backslash-escape dialects across the host languages.
#[derive(Clone, Copy, PartialEq)]
enum EscapeMode {
	/// `\u{...}` and the line-continuation escape.
	Rust,
	/// The shared common escapes only (also used for Python, whose
	/// extras like `\u####` bail to leave-untouched).
	Go,
	/// Adds `` \` `` and `\$`; `\u{...}` or `\u####`.
	Js,
	/// `\u{...}`, no line continuation.
	Gleam,
}

/// Decode `\`-escapes. Any escape a mode does not know leaves the
/// literal untouched (`None`), never a guess.
fn unescape(body: &str, mode: EscapeMode) -> Option<String> {
	let mut out = String::with_capacity(body.len());
	let mut chars = body.chars().peekable();
	while let Some(c) = chars.next() {
		if c != '\\' {
			out.push(c);
			continue;
		}
		match chars.next()? {
			'n' => out.push('\n'),
			'r' => out.push('\r'),
			't' => out.push('\t'),
			'\\' => out.push('\\'),
			'"' => out.push('"'),
			'\'' => out.push('\''),
			'0' => out.push('\0'),
			'`' if mode == EscapeMode::Js => out.push('`'),
			'$' if mode == EscapeMode::Js => out.push('$'),
			'x' if mode != EscapeMode::Gleam => {
				let hex: String = chars.by_ref().take(2).collect();
				out.push(u8::from_str_radix(&hex, 16).ok()? as char);
			}
			'u'
				if matches!(
					mode,
					EscapeMode::Rust | EscapeMode::Gleam | EscapeMode::Js
				) =>
			{
				if chars.peek() == Some(&'{') {
					chars.next();
					let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
					out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
				} else if mode == EscapeMode::Js {
					let hex: String = chars.by_ref().take(4).collect();
					out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
				} else {
					return None;
				}
			}
			'\n' if mode == EscapeMode::Rust => {
				// Line continuation: skip following whitespace.
				while chars.peek().is_some_and(|c| c.is_whitespace()) {
					chars.next();
				}
			}
			_ => return None, // unknown escape: leave the literal alone
		}
	}
	Some(out)
}

fn encode(decoded: &Decoded, content: &str) -> Option<String> {
	match &decoded.kind {
		LiteralKind::RustRaw { hashes } => {
			// Keep the original hash count unless the content now needs
			// more (it never should — we only move whitespace).
			let needed = min_raw_hashes(content);
			let hashes = (*hashes).max(needed);
			let fence = "#".repeat(hashes);
			Some(format!("r{fence}\"{content}\"{fence}"))
		}
		LiteralKind::RustPlain => {
			// Rewritten as a raw string, so the SQL reads as written: no
			// escaped quotes or backslashes. Anything only an escape can
			// spell (a carriage return, a NUL) stays as it was.
			if has_unwritable_control(content) {
				return None;
			}
			let fence = "#".repeat(min_raw_hashes(content).max(1));
			Some(format!("r{fence}\"{content}\"{fence}"))
		}
		LiteralKind::GoRaw => {
			if content.contains('`') {
				None // cannot be represented; leave untouched
			} else {
				Some(format!("`{content}`"))
			}
		}
		LiteralKind::GoPlain => {
			// Interpreted strings are single-line: a multi-line query
			// becomes a raw string. Raw strings cannot hold a backtick,
			// and Go drops carriage returns from them.
			if content.contains('`') || has_unwritable_control(content) {
				None
			} else {
				Some(format!("`{content}`"))
			}
		}
		LiteralKind::PyTriple { raw, quote } => {
			let fence: String = std::iter::repeat_n(*quote, 3).collect();
			if content.contains(&fence) || (*raw && content.contains('\\')) {
				return None; // cannot be represented in this fence
			}
			let body =
				if *raw { content.to_string() } else { content.replace('\\', "\\\\") };
			let prefix = if *raw { "r" } else { "" };
			Some(format!("{prefix}{fence}{body}{fence}"))
		}
		LiteralKind::JsTemplate => {
			let mut out = String::with_capacity(content.len() + 2);
			out.push('`');
			let mut chars = content.chars().peekable();
			while let Some(c) = chars.next() {
				match c {
					'\\' => out.push_str("\\\\"),
					'`' => out.push_str("\\`"),
					'$' if chars.peek() == Some(&'{') => out.push_str("\\$"),
					_ => out.push(c),
				}
			}
			out.push('`');
			Some(out)
		}
		LiteralKind::GleamString => {
			let mut out = String::with_capacity(content.len() + 2);
			out.push('"');
			for c in content.chars() {
				match c {
					'\\' => out.push_str("\\\\"),
					'"' => out.push_str("\\\""),
					_ => out.push(c),
				}
			}
			out.push('"');
			Some(out)
		}
	}
}

/// Characters other than newline and tab that a raw string cannot
/// hold faithfully.
fn has_unwritable_control(content: &str) -> bool {
	content.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
}

/// Fewest `#`s a Rust raw string needs to hold `content`.
fn min_raw_hashes(content: &str) -> usize {
	let mut needed = 0;
	let bytes = content.as_bytes();
	let mut index = 0;
	while index < bytes.len() {
		if bytes[index] == b'"' {
			let run = bytes[index + 1..].iter().take_while(|&&b| b == b'#').count();
			needed = needed.max(run + 1);
			index += run + 1;
		} else {
			index += 1;
		}
	}
	needed
}
