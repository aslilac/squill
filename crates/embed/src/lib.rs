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
//! - **Content** (C++, C#, Java, Kotlin, Swift, and every wasm grammar): the
//!   capture is the string's *content* node, between the delimiters.
//!   It is taken verbatim. What the string syntax allows, its query
//!   pattern promises with `#set!` properties, the built-in queries
//!   included: `squill.multiline` (raw line breaks, so a one-line string
//!   can be formatted onto several; unpromised, only a string that
//!   already spans lines is formatted) and `squill.raw` (no backslash
//!   escapes; unpromised, a string holding a backslash is left alone).
//!   A string that takes escapes says which with `squill.escape`
//!   (`"whitespace"`, `"\xHH"`, …, see [`escapes`]); the grammar's
//!   `escape_sequence` nodes, and `@squill.escape` captures, say where
//!   they are. Each is read to read the SQL and written back as it was
//!   spelled. `squill.promote-to-raw-syntax` names raw syntaxes a string
//!   spanning lines is rewritten in, when every escape in it was layout.
//!   And `@squill.skip` excludes any SQL capture it overlaps: a template
//!   hole in the string, or a region the string sits in.
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
		feature = "cxx",
		feature = "csharp",
		feature = "java",
		feature = "kotlin",
		feature = "swift"
	)),
	allow(dead_code, unused_imports, unused_variables, unreachable_patterns)
)]

mod escapes;

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
	#[cfg(feature = "csharp")]
	CSharp,
	#[cfg(feature = "cxx")]
	Cxx,
	#[cfg(feature = "gleam")]
	Gleam,
	#[cfg(feature = "go")]
	Go,
	#[cfg(feature = "javascript")]
	JavaScript,
	#[cfg(feature = "java")]
	Java,
	#[cfg(feature = "kotlin")]
	Kotlin,
	#[cfg(feature = "python")]
	Python,
	#[cfg(feature = "rust")]
	Rust,
	#[cfg(feature = "swift")]
	Swift,
	#[cfg(feature = "typescript")]
	TypeScript,
	/// TypeScript with JSX (`.tsx`) — a distinct grammar, same codec.
	#[cfg(feature = "typescript")]
	Tsx,
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
		#[cfg(feature = "cxx")]
		Host::Cxx,
		#[cfg(feature = "csharp")]
		Host::CSharp,
		#[cfg(feature = "java")]
		Host::Java,
		#[cfg(feature = "kotlin")]
		Host::Kotlin,
		#[cfg(feature = "swift")]
		Host::Swift,
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
			#[cfg(feature = "cxx")]
			Host::Cxx => "c++",
			#[cfg(feature = "csharp")]
			Host::CSharp => "c#",
			#[cfg(feature = "java")]
			Host::Java => "java",
			#[cfg(feature = "kotlin")]
			Host::Kotlin => "kotlin",
			#[cfg(feature = "swift")]
			Host::Swift => "swift",
		}
	}

	pub fn from_name(name: &str) -> Option<Host> {
		Host::ALL.iter().copied().find(|host| host.name() == name)
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
			#[cfg(feature = "cxx")]
			Host::Cxx => CXX_SQL_QUERY,
			#[cfg(feature = "csharp")]
			Host::CSharp => CSHARP_SQL_QUERY,
			#[cfg(feature = "java")]
			Host::Java => JAVA_SQL_QUERY,
			#[cfg(feature = "kotlin")]
			Host::Kotlin => KOTLIN_SQL_QUERY,
			#[cfg(feature = "swift")]
			Host::Swift => SWIFT_SQL_QUERY,
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
			#[cfg(feature = "cxx")]
			Host::Cxx => tree_sitter_cpp::LANGUAGE.into(),
			#[cfg(feature = "csharp")]
			Host::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
			#[cfg(feature = "java")]
			Host::Java => tree_sitter_java::LANGUAGE.into(),
			#[cfg(feature = "kotlin")]
			Host::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
			#[cfg(feature = "swift")]
			Host::Swift => tree_sitter_swift::LANGUAGE.into(),
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
			// What their strings allow, their default queries say, with
			// the same `#set!` properties any query can use.
			#[cfg(feature = "cxx")]
			Host::Cxx => Codec::Content,
			#[cfg(feature = "csharp")]
			Host::CSharp => Codec::Content,
			#[cfg(feature = "kotlin")]
			Host::Kotlin => Codec::Content,
			#[cfg(feature = "swift")]
			Host::Swift => Codec::Content,
			#[cfg(feature = "java")]
			Host::Java => Codec::Content,
		}
	}
}

/// How captured strings are read and written back.
#[derive(Clone, Copy)]
#[cfg_attr(
	not(any(
		feature = "cxx",
		feature = "csharp",
		feature = "java",
		feature = "kotlin",
		feature = "swift",
		feature = "external-grammars"
	)),
	allow(dead_code)
)]
enum Codec {
	/// The capture is a whole literal; decoded by the host's codec.
	Literal,
	/// The capture is the content between the delimiters, verbatim.
	/// What the string syntax allows comes from the query's pattern
	/// ([`StringSyntax`]).
	Content,
}

/// What a content capture's string syntax allows, as its query pattern
/// promises with `#set!` properties. Unpromised, squill assumes the
/// worst: backslashes may be escapes, and line breaks may be illegal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct StringSyntax {
	/// `(#set! squill.raw)`: a backslash is just a backslash, so a
	/// string holding one can be formatted. Checked: formatting must keep
	/// what follows every backslash, so a string that did take escapes
	/// can't have one altered.
	raw: bool,
	/// `(#set! squill.multiline)`: the syntax takes raw line breaks, so a
	/// string on one line can be formatted onto several. Checked only as
	/// strictly as the grammar is: the host re-parse catches a syntax
	/// that doesn't take them when the grammar rejects the line break,
	/// and many (Java's, Kotlin's, Swift's) don't.
	multiline: bool,
	/// `(#set! squill.escape "whitespace")`, once per kind: the escapes
	/// squill may read. An escape no kind covers leaves the string alone.
	escapes: Vec<escapes::Group>,
	/// `(#set! squill.promote-to-raw-syntax "r#\"{}\"#")`, in order: raw
	/// syntaxes to rewrite a string spanning lines in, its content where
	/// `{}` is. The first whose closing delimiter the content doesn't hold
	/// wins.
	promote: Vec<String>,
}

impl StringSyntax {
	/// The syntax a query pattern's properties describe.
	fn of(query: &Query, pattern: usize) -> Self {
		let mut syntax = StringSyntax::default();
		for property in query.property_settings(pattern) {
			let value = property.value.as_deref().unwrap_or_default();
			match property.key.as_ref() {
				"squill.raw" => syntax.raw = true,
				"squill.multiline" => syntax.multiline = true,
				"squill.escape" => {
					syntax.escapes.extend(escapes::Group::from_name(value));
				}
				"squill.promote-to-raw-syntax" => {
					syntax.promote.push(value.to_string());
				}
				_ => {}
			}
		}
		syntax
	}
}

/// A tree-sitter grammar to find SQL with: built in, or (with the
/// `external-grammars` feature) loaded from a `.wasm` file at runtime.
#[derive(Clone)]
pub enum Grammar {
	Builtin(Host),
	#[cfg(feature = "external-grammars")]
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
			#[cfg(feature = "external-grammars")]
			Grammar::Wasm(grammar) => grammar.name(),
		}
	}

	fn codec(&self) -> Codec {
		match self {
			Grammar::Builtin(host) => host.codec(),
			#[cfg(feature = "external-grammars")]
			Grammar::Wasm(_) => Codec::Content,
		}
	}

	fn host(&self) -> Option<Host> {
		match self {
			Grammar::Builtin(host) => Some(*host),
			#[cfg(feature = "external-grammars")]
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
			#[cfg(feature = "external-grammars")]
			Grammar::Wasm(grammar) => grammar.with_parser(f),
		}
	}
}

#[cfg(feature = "external-grammars")]
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

	/// The language a wasm grammar file holds, from its name the way
	/// `tree-sitter build --wasm` writes it: `tree-sitter-c_sharp.wasm`
	/// holds `c_sharp`.
	pub fn language_name(file_name: &str) -> Option<String> {
		let stem = file_name.strip_suffix(".wasm")?;
		let stem = stem.strip_prefix("tree-sitter-").unwrap_or(stem);
		(!stem.is_empty()).then(|| stem.replace('-', "_"))
	}

	impl WasmGrammar {
		/// Load a grammar built by `tree-sitter build --wasm`. The
		/// language name comes from the file name, the way that command
		/// writes it: `tree-sitter-c_sharp.wasm` holds `c_sharp`.
		pub fn load(path: &Path) -> Result<Self, EmbedError> {
			let bytes = std::fs::read(path).map_err(|err| {
				EmbedError::Grammar(format!("{}: {err}", path.display()))
			})?;
			let file_name =
				path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
			let name = language_name(file_name).ok_or_else(|| {
				EmbedError::Grammar(format!(
					"{}: cannot name a grammar from this file name",
					path.display()
				))
			})?;
			Self::from_bytes(name, path.display().to_string(), bytes)
		}

		/// A grammar from its wasm bytes, named `name`. `key` identifies it
		/// (a path or URL) in messages and the per-thread parser cache.
		pub fn from_bytes(
			name: String,
			key: String,
			bytes: Vec<u8>,
		) -> Result<Self, EmbedError> {
			let grammar = WasmGrammar { name, key, bytes: bytes.into() };
			// Fail at load time, not on the first file that uses it.
			grammar.with_parser(|_, _| Ok(())).map_err(|err| match err {
				EmbedError::Grammar(message) => {
					EmbedError::Grammar(format!("{}: {message}", grammar.key))
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
/// template literals (postgres.js style). TypeScript reads a generic call
/// right after `await` (`await pool.query<Row>(…)`) as a call of the whole
/// `await` expression, so that shape is matched too.
#[cfg(any(feature = "javascript", feature = "typescript"))]
pub const JS_SQL_QUERY: &str = r#"
((call_expression
   function: [
     (member_expression property: (property_identifier) @_method)
     (await_expression
       (member_expression property: (property_identifier) @_method))
   ]
   arguments: (arguments . [(string) (template_string)] @sql))
 (#any-of? @_method "query" "execute" "prepare"))

((call_expression
   function: (identifier) @_tag
   arguments: (template_string) @sql)
 (#eq? @_tag "sql"))
"#;

/// Default extraction query for Gleam: the first string argument of
/// `query` / `exec` / `execute` calls, module-qualified (`sqlight.query`,
/// `pog.query`) or bare, or a string piped into one (`"…" |> pog.query`,
/// `"…" |> sqlight.query(on: db)`). `sqlight` is a SQLite library, so its
/// calls carry that dialect; everything else uses the session dialect.
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

((binary_expression
   left: (string) @sql.sqlite
   operator: "|>"
   right: [
     (field_access record: (identifier) @_mod field: (label) @_fn)
     (function_call
       function: (field_access record: (identifier) @_mod field: (label) @_fn))
   ])
 (#eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((binary_expression
   left: (string) @sql
   operator: "|>"
   right: [
     (field_access record: (identifier) @_mod field: (label) @_fn)
     (function_call
       function: (field_access record: (identifier) @_mod field: (label) @_fn))
   ])
 (#not-eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((binary_expression
   left: (string) @sql
   operator: "|>"
   right: [
     (identifier) @_fn
     (function_call function: (identifier) @_fn)
   ])
 (#any-of? @_fn "query" "exec" "execute"))
"#;

/// Default extraction query for C++: raw-string (`R"(...)"`) arguments
/// of sqlite3, libpq, and libpqxx calls. The C APIs name their dialect;
/// pqxx-style `exec`/`query` calls, templated (`tx.query<int>`) or not,
/// use the configured one. Raw strings take line breaks and no escapes.
#[cfg(feature = "cxx")]
pub const CXX_SQL_QUERY: &str = r#"
((call_expression
   function: (identifier) @_fn
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql.sqlite)))
 (#any-of? @_fn
   "sqlite3_prepare" "sqlite3_prepare_v2" "sqlite3_prepare_v3" "sqlite3_exec")
 (#set! squill.raw) (#set! squill.multiline))

((call_expression
   function: (identifier) @_fn
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql.postgres)))
 (#any-of? @_fn "PQexec" "PQexecParams" "PQprepare" "PQsendQuery")
 (#set! squill.raw) (#set! squill.multiline))

((call_expression
   function: [
     (field_expression field: [
       (field_identifier) @_fn
       (template_method name: (field_identifier) @_fn)
     ])
     (qualified_identifier name: (identifier) @_fn)
   ]
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql)))
 (#any-of? @_fn
   "exec" "exec0" "exec1" "exec_n" "exec_params" "exec_params0"
   "exec_params1" "exec_prepared" "prepare" "query" "query1" "query01"
   "query_n" "query_value" "for_query" "stream")
 (#set! squill.raw) (#set! squill.multiline))
"#;

/// Default extraction query for C#: raw-string (`"""`) arguments of EF
/// Core migrations and raw-SQL calls, ADO.NET's `CommandText`, and
/// Dapper's query/execute family, generic (`QueryAsync<Order>`) or not.
/// Interpolated raw strings (`$"""`) are a different node and never
/// match. Raw strings take no escapes, and a one-line `"""…"""` becomes
/// a multi-line one in squill's layout: its content on lines of its own.
#[cfg(feature = "csharp")]
pub const CSHARP_SQL_QUERY: &str = r#"
((invocation_expression
   function: (member_access_expression name: [
     (identifier) @_method
     (generic_name (identifier) @_method)
   ])
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
   "ExecuteReader" "ExecuteReaderAsync")
 (#set! squill.raw) (#set! squill.multiline))

((assignment_expression
   left: (member_access_expression name: (identifier) @_prop)
   right: (raw_string_literal (raw_string_content) @sql))
 (#eq? @_prop "CommandText")
 (#set! squill.raw) (#set! squill.multiline))
"#;

/// Default extraction query for Java: text-block (`"""`) arguments of
/// JDBC, JPA, and Spring `JdbcTemplate` calls. The whole literal is
/// captured — the grammar has no node spanning a text block's content —
/// and one holding an escape is reported, not rewritten. Neither
/// property holds: text blocks take escapes, and the capture can't tell
/// a text block from a `"..."` string, which can't take a line break.
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
/// JDBC, Spring, and Exposed calls, bare or `.trimIndent()`ed. A raw
/// string with `$` templates has several content nodes and never matches.
/// Raw strings take line breaks and no escapes.
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
   "update" "batchUpdate" "exec")
 (#set! squill.raw) (#set! squill.multiline))

((call_expression
   [
     (identifier) @_method
     (navigation_expression (identifier) @_method .)
   ]
   (value_arguments
     (value_argument
       (call_expression
         (navigation_expression
           (multiline_string_literal . (string_content) @sql .)
           (identifier) @_trim)))))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "execute" "addBatch" "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "update" "batchUpdate" "exec")
 (#eq? @_trim "trimIndent")
 (#set! squill.raw) (#set! squill.multiline))
"#;

/// Default extraction query for Swift: multi-line (`"""`) string
/// arguments. One labeled `sql:` is GRDB's, and SQLite; the first,
/// unlabeled argument of `run` / `execute` / `prepare` / `scalar`
/// (SQLite.swift), `query` (PostgresNIO), or `raw` (SQLKit) uses the
/// configured dialect. A string with a `\(…)` interpolation or an escape
/// has several content nodes and never matches; raw strings (`#"""`)
/// aren't taken. Multi-line strings take escapes, so aren't `raw`.
#[cfg(feature = "swift")]
pub const SWIFT_SQL_QUERY: &str = r#"
((value_argument
   name: (value_argument_label (simple_identifier) @_label)
   value: (multi_line_string_literal . (multi_line_str_text) @sql.sqlite .))
 (#eq? @_label "sql")
 (#set! squill.multiline))

((call_expression
   [
     (simple_identifier) @_fn
     (navigation_expression
       suffix: (navigation_suffix suffix: (simple_identifier) @_fn))
   ]
   (call_suffix
     (value_arguments
       .
       (value_argument
         !name
         value: (multi_line_string_literal . (multi_line_str_text) @sql .)))))
 (#any-of? @_fn "run" "execute" "prepare" "scalar" "query" "raw")
 (#set! squill.multiline))
"#;

/// Which indent options the caller configured. Whatever it didn't comes
/// from the host file, so embedded SQL is indented like the code around
/// it: a spaces-indented file never gains tabs, and a 4-space file
/// indents its SQL by 4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Indent {
	/// Use `Options::indent_style` as given, not the host file's indent
	/// character.
	pub configured_style: bool,
	/// Use `Options::indent_width` as given, not the host file's indent
	/// step.
	pub configured_width: bool,
}

impl Indent {
	/// Nothing configured: the host file decides both. The default.
	pub const FROM_HOST: Self =
		Self { configured_style: false, configured_width: false };
	/// Both configured: the options apply as given.
	pub const CONFIGURED: Self =
		Self { configured_style: true, configured_width: true };
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
	/// Where the capture ends (exclusive).
	pub end: usize,
	pub message: String,
}

/// One planned rewrite: the capture's byte range in the original
/// source and what it becomes.
struct Edit {
	range: std::ops::Range<usize>,
	replacement: String,
	/// Where in `replacement` the SQL is: all of it, unless the edit
	/// rewrote a whole string. A re-parse must capture the edit as either.
	sql: std::ops::Range<usize>,
}

impl Edit {
	/// An edit whose replacement is all SQL.
	fn content(range: std::ops::Range<usize>, replacement: String) -> Self {
		let sql = 0..replacement.len();
		Edit { range, replacement, sql }
	}

	/// Does a re-parse read this edit, applied at `start`, back as SQL?
	fn reads_back(
		&self,
		start: usize,
		captured: &std::collections::HashSet<std::ops::Range<usize>>,
	) -> bool {
		captured.contains(&(start..start + self.replacement.len()))
			|| captured.contains(&(start + self.sql.start..start + self.sql.end))
	}
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
	// A dialect squill doesn't know (or a typo of one) would match
	// nothing, silently: refuse the query instead.
	for name in query.capture_names() {
		if name.starts_with("sql.")
			&& sql_dialect(name, Dialect::default()).is_none()
		{
			return Err(EmbedError::Query(format!(
				"unknown capture `@{name}`: SQL is captured as `@sql` (in the \
				 configured dialect), `@sql.postgres`, or `@sql.sqlite`"
			)));
		}
		if name.starts_with("squill.")
			&& !matches!(*name, "squill.skip" | "squill.escape")
		{
			return Err(EmbedError::Query(format!(
				"unknown capture `@{name}`: squill reads `@squill.skip` and \
				 `@squill.escape`"
			)));
		}
	}
	// Likewise a `squill.` property squill doesn't know, or a value it
	// can't use.
	for pattern in 0..query.pattern_count() {
		let mut raw = false;
		let mut escape = false;
		for property in query.property_settings(pattern) {
			let key = property.key.as_ref();
			let value = property.value.as_deref();
			match key {
				"squill.raw" => raw = true,
				"squill.multiline" => {}
				"squill.escape" => {
					escape = true;
					if value.and_then(escapes::Group::from_name).is_none() {
						let known: Vec<String> = escapes::Group::NAMES
							.iter()
							.map(|(name, _)| format!("\"{name}\""))
							.collect();
						return Err(EmbedError::Query(format!(
							"`squill.escape` takes one of {}",
							known.join(", ")
						)));
					}
				}
				"squill.promote-to-raw-syntax" => {
					if value.is_none_or(|value| value.matches("{}").count() != 1) {
						return Err(EmbedError::Query(
							"`squill.promote-to-raw-syntax` takes a raw string with `{}` \
							 where its content goes, like \"r#\\\"{}\\\"#\""
								.to_string(),
						));
					}
				}
				_ if key.starts_with("squill.") => {
					return Err(EmbedError::Query(format!(
						"unknown property `{key}`: squill reads `squill.raw`, \
						 `squill.multiline`, `squill.escape`, and \
						 `squill.promote-to-raw-syntax`"
					)));
				}
				_ => {}
			}
		}
		if raw && escape {
			return Err(EmbedError::Query(
				"a pattern can't set both `squill.raw` (no escapes) and \
				 `squill.escape`"
					.to_string(),
			));
		}
	}
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

/// One string the query captured as SQL: where its contents are (inside
/// the delimiters), and which dialect they're in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
	pub range: std::ops::Range<usize>,
	pub dialect: Dialect,
	/// The query named the dialect (`@sql.sqlite`), over the configured
	/// one.
	pub pinned_dialect: bool,
}

/// Every SQL string the query captures in `source`, in order, whether or
/// not squill would rewrite it — for tools that want to know where the
/// SQL is (`squill locate`) rather than change it.
pub fn locate_sql(
	source: &str,
	grammar: &Grammar,
	query_source: &str,
	default_dialect: Dialect,
) -> Result<Vec<Located>, EmbedError> {
	grammar.with_parser(|ts, language| {
		let query = compile_query(language, query_source)?;
		let tree = ts.parse(source, None).ok_or(EmbedError::HostParse)?;
		let codec = grammar.codec();
		let extraction = Extraction { query: &query, default_dialect, codec };
		let mut found: Vec<Located> = extraction
			.captures(&tree, source)
			.into_iter()
			.map(|Captured { range, dialect, pinned, .. }| {
				let range = match codec {
					Codec::Literal => {
						let inner = literal_content(&source[range.clone()]);
						range.start + inner.start..range.start + inner.end
					}
					Codec::Content => range,
				};
				Located { range, dialect, pinned_dialect: pinned }
			})
			.collect();
		found.sort_by_key(|located| (located.range.start, located.range.end));
		found.dedup_by(|a, b| a.range == b.range);
		Ok(found)
	})
}

/// Where a whole literal's contents are: past its prefix letters (`r`,
/// `b`, Python's `rb`…), raw-string hashes, and opening quotes, and
/// before the matching close. A shape it doesn't recognize is all
/// content.
fn literal_content(literal: &str) -> std::ops::Range<usize> {
	let bytes = literal.as_bytes();
	let mut open = bytes.iter().take_while(|b| b.is_ascii_alphabetic()).count();
	let hashes = bytes[open..].iter().take_while(|&&b| b == b'#').count();
	open += hashes;
	let Some(&quote) =
		bytes.get(open).filter(|b| matches!(b, b'"' | b'\'' | b'`'))
	else {
		return 0..literal.len();
	};
	let mut quotes =
		bytes[open..].iter().take(3).take_while(|&&b| b == quote).count();
	// `""` is an empty string, not an unclosed triple quote.
	if quotes == 2 {
		quotes = 1;
	}
	let start = open + quotes;
	let end = literal.len().saturating_sub(quotes + hashes).max(start);
	start..end
}

/// Format every SQL snippet the query captures in `source`, returning the
/// rewritten host file. Unparsable or unsafe snippets stay byte-exact
/// and come back as warnings. `indent` says which of the indent options
/// apply as given; the host file's own indentation decides the rest.
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
		let captures = extraction.captures(&tree, source);
		let mut options = *options;
		if !indent.configured_width {
			let strings: Vec<_> =
				captures.iter().map(|captured| captured.range.clone()).collect();
			if let Some(width) = host_indent_width(source, &strings) {
				options.indent_width = width;
			}
		}
		let options = &options;
		for Captured {
			range: node_range,
			literal,
			escapes,
			dialect,
			pinned,
			syntax,
		} in captures
		{
			let snippet = Snippet {
				source,
				range: node_range.clone(),
				literal: literal.clone(),
				escapes,
				grammar,
				dialect,
				pinned,
				syntax,
				options,
				indent,
				body_warning: std::cell::RefCell::new(None),
			};
			let rewrite = snippet.rewrite();
			if let Some(message) = snippet.body_warning.take() {
				warnings.push(Warning {
					offset: node_range.start,
					end: node_range.end,
					message,
				});
			}
			match rewrite {
				Rewrite::Skip => {}
				Rewrite::Warn(message) => {
					warnings.push(Warning {
						offset: node_range.start,
						end: node_range.end,
						message,
					});
				}
				Rewrite::Replace(replacement) => {
					if replacement != source[node_range.clone()] {
						edits.push(Edit::content(node_range, replacement));
					}
				}
				Rewrite::ReplaceString(replacement, sql) => {
					edits.push(Edit { range: literal, replacement, sql });
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
					end: edit.range.end,
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

/// One SQL capture: where, in which dialect, and whether the query
/// pinned that dialect rather than taking the configured one.
struct Captured {
	/// What to rewrite: the content, for a content capture.
	range: std::ops::Range<usize>,
	/// The whole string, delimiters and all, as far as the capture says:
	/// the captured node, which for a content node is `range` again.
	literal: std::ops::Range<usize>,
	/// The escapes inside `range`, sorted: `escape_sequence` nodes in the
	/// captured one, and `@squill.escape` captures.
	escapes: Vec<std::ops::Range<usize>>,
	dialect: Dialect,
	pinned: bool,
	syntax: StringSyntax,
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
	/// dialect — pinned, when the capture names one (`@sql.sqlite`).
	///
	/// A capture overlapping any `@squill.skip` capture is left out:
	/// a hole in it, or it inside something the query excludes.
	fn captures(&self, tree: &Tree, source: &str) -> Vec<Captured> {
		let query = self.query;
		let mut out = Vec::new();
		let mut skips = Vec::new();
		let mut escapes = Vec::new();
		let mut cursor = QueryCursor::new();
		let mut matches =
			cursor.matches(query, tree.root_node(), source.as_bytes());
		while let Some(query_match) = matches.next() {
			if !predicates_hold(query, query_match, source) {
				continue;
			}
			for capture in query_match.captures {
				let name = &query.capture_names()[capture.index as usize];
				match *name {
					"squill.skip" => skips.push(capture.node.byte_range()),
					"squill.escape" => escapes.push(capture.node.byte_range()),
					_ => {}
				}
				let Some(dialect) = sql_dialect(name, self.default_dialect) else {
					continue;
				};
				let range = match self.codec {
					Codec::Literal => capture.node.byte_range(),
					Codec::Content => content_range(capture.node),
				};
				let mut found = Vec::new();
				escape_sequences(capture.node, &range, &mut found);
				out.push(Captured {
					range,
					literal: capture.node.byte_range(),
					escapes: found,
					dialect,
					pinned: *name != "sql",
					syntax: StringSyntax::of(query, query_match.pattern_index),
				});
			}
		}
		let overlaps = |a: &std::ops::Range<usize>, b: &std::ops::Range<usize>| {
			a.start < b.end && b.start < a.end
		};
		out.retain(|captured| {
			!skips.iter().any(|skip| overlaps(skip, &captured.literal))
		});
		for captured in &mut out {
			captured.escapes.extend(
				escapes
					.iter()
					.filter(|escape| {
						captured.range.start <= escape.start
							&& escape.end <= captured.range.end
					})
					.cloned(),
			);
			captured.escapes.sort_by_key(|escape| (escape.start, escape.end));
			captured.escapes.dedup();
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
		// A string promoted to raw syntax was rewritten whole: it reads
		// back as the whole string, or as its content.
		let captures: std::collections::HashSet<std::ops::Range<usize>> = self
			.captures(&tree, text)
			.into_iter()
			.flat_map(|captured| [captured.range, captured.literal])
			.collect();
		let mut failed = Vec::new();
		let mut shift: isize = 0;
		for (index, edit) in edits.iter().enumerate() {
			let start = (edit.range.start as isize + shift) as usize;
			if !edit.reads_back(start, &captures) {
				failed.push(index);
			}
			shift += edit.replacement.len() as isize - edit.range.len() as isize;
		}
		if failed.len() > 1 {
			// One bad edit can take the rest of the file with it (a
			// heredoc that no longer ends): blame only the edits that fail
			// on their own, unless none does.
			let mut alone = Vec::new();
			for &index in &failed {
				if self.fails_alone(ts, source, &edits[index], baseline_errors)? {
					alone.push(index);
				}
			}
			if !alone.is_empty() {
				failed = alone;
			}
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

impl Extraction<'_> {
	/// Does `edit`, the only one applied to `source`, fail the re-parse:
	/// a new syntax error, or its string not reading back as written?
	fn fails_alone(
		&self,
		ts: &mut TsParser,
		source: &str,
		edit: &Edit,
		baseline_errors: usize,
	) -> Result<bool, EmbedError> {
		let text = apply(source, std::slice::from_ref(edit));
		let tree = ts.parse(&text, None).ok_or(EmbedError::HostParse)?;
		let captures = self
			.captures(&tree, &text)
			.into_iter()
			.flat_map(|captured| [captured.range, captured.literal])
			.collect();
		Ok(
			error_count(&tree) > baseline_errors
				|| !edit.reads_back(edit.range.start, &captures),
		)
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

/// The `escape_sequence` nodes in `node` that lie inside `content`.
fn escape_sequences(
	node: tree_sitter::Node<'_>,
	content: &std::ops::Range<usize>,
	out: &mut Vec<std::ops::Range<usize>>,
) {
	if node.end_byte() <= content.start || content.end <= node.start_byte() {
		return;
	}
	if node.kind() == "escape_sequence" {
		let range = node.byte_range();
		if content.start <= range.start && range.end <= content.end {
			out.push(range);
		}
		return;
	}
	let mut cursor = node.walk();
	for child in node.children(&mut cursor) {
		escape_sequences(child, content, out);
	}
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
	/// New content for the capture.
	Replace(String),
	/// A new whole string, delimiters and all, and where in it the SQL
	/// is: promoted to raw syntax.
	ReplaceString(String, std::ops::Range<usize>),
}

/// One captured string and everything needed to rewrite it.
struct Snippet<'a> {
	source: &'a str,
	range: std::ops::Range<usize>,
	/// The whole string (see [`Captured::literal`]).
	literal: std::ops::Range<usize>,
	/// Its escapes, sorted.
	escapes: Vec<std::ops::Range<usize>>,
	grammar: &'a Grammar,
	dialect: Dialect,
	/// Did the query set `dialect` (`@sql.sqlite`), over the configured
	/// one?
	pinned: bool,
	/// What a content capture's string syntax allows.
	syntax: StringSyntax,
	options: &'a Options,
	indent: Indent,
	/// A procedural body in the SQL that didn't parse (so was left as
	/// written), reported alongside whatever else happens to the string.
	body_warning: std::cell::RefCell<Option<String>>,
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
				// multiple lines, so SQL that takes more than one gets the
				// vertical shape. A plain Rust or Go string joins them once
				// it already
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
				// What the quotes and prefix take on the line, for SQL that
				// stays on it.
				let delimiters =
					text.chars().count().saturating_sub(decoded.content.chars().count());
				let Anchored { text: anchored, line_ending_crs } =
					match self.format(&decoded.content, false, delimiters) {
						Ok(anchored) => anchored,
						Err(rewrite) => return rewrite,
					};
				match encode(&decoded, &anchored, &line_ending_crs) {
					Some(literal) => Rewrite::Replace(literal),
					None => Rewrite::Warn(
						"formatted SQL cannot be written back into this string \
						 literal; left unformatted"
							.to_string(),
					),
				}
			}
			Codec::Content => {
				if text.trim().is_empty() {
					return Rewrite::Skip;
				}
				let backslashes = text.contains('\\');
				// The SQL the string spells, its escapes read. A raw string
				// has none: its backslashes are just backslashes.
				let read = if self.syntax.raw || !backslashes {
					None
				} else {
					let escapes: Vec<_> = self
						.escapes
						.iter()
						.map(|escape| {
							escape.start - self.range.start..escape.end - self.range.start
						})
						.collect();
					match escapes::read(text, &escapes, &self.syntax.escapes) {
						Ok(read) => Some(read),
						Err(escapes::Unreadable::Backslash) => {
							return Rewrite::Warn(
								"string holds a backslash, which may be an escape; left \
								 unformatted (a query can promise the string is raw with \
								 `(#set! squill.raw)`)"
									.to_string(),
							);
						}
						Err(escapes::Unreadable::Escape(escape)) => {
							return Rewrite::Warn(format!(
								"string holds a backslash escape squill doesn't read \
								 (`{escape}`); left unformatted (a query says which \
								 escapes a string takes with `(#set! squill.escape …)`)",
								escape = escape.escape_debug(),
							));
						}
					}
				};
				let sql = read.as_ref().map_or(text, |read| read.sql.as_str());
				// A string that already spans lines, as written or escaped,
				// proves the syntax takes line breaks of some spelling;
				// otherwise the query must promise it.
				let spans_lines = text.contains('\n') || sql.contains('\n');
				if !(spans_lines || self.syntax.multiline) {
					return Rewrite::Skip;
				}
				// A closing delimiter at the margin may have to stay there
				// (a bare Ruby heredoc's terminator), so it does.
				let close_at_margin = text.ends_with('\n');
				let anchored = match self.format(sql, close_at_margin, 0) {
					Ok(anchored) => anchored.text,
					Err(rewrite) => return rewrite,
				};
				// Every token spelled as it was, escapes and all.
				let anchored = match &read {
					Some(read) => match read.respell(
						text,
						&anchored,
						self.dialect,
						self.options.lex_options(),
					) {
						Some(respelled) => respelled,
						None => {
							return Rewrite::Warn(
								"formatting would change SQL spelled with a backslash \
								 escape; left unformatted"
									.to_string(),
							);
						}
					},
					None => anchored,
				};
				if anchored.contains('\n') {
					// Raw syntax, when the string asks for it and every escape
					// was layout (squill never respells one).
					if !self.syntax.raw
						&& self.literal != self.range
						&& !anchored.contains('\\')
						&& let Some((string, sql)) = self.promote(&anchored)
					{
						return Rewrite::ReplaceString(string, sql);
					}
					// Line breaks only an escape spelled: the syntax may not
					// take them raw.
					if !(text.contains('\n') || self.syntax.multiline) {
						return Rewrite::Warn(
							"formatted SQL spans lines, which this string can't hold \
							 without escapes; left unformatted"
								.to_string(),
						);
					}
				}
				// Raw, as promised: then whatever follows each backslash is
				// untouched, since formatting only changes whitespace
				// between tokens. If it changed, the string may take
				// escapes after all (a line continuation squill moved).
				if read.is_none() && backslashes && escapes(text).ne(escapes(&anchored))
				{
					return Rewrite::Warn(
						"formatting would change what follows a backslash, which \
						 this string may treat as an escape; left unformatted"
							.to_string(),
					);
				}
				Rewrite::Replace(anchored)
			}
		}
	}

	/// `content` in the first of the syntax's raw forms that can hold it:
	/// one whose closing delimiter it doesn't contain, without a control
	/// character but the line endings squill wrote.
	fn promote(&self, content: &str) -> Option<(String, std::ops::Range<usize>)> {
		let crs: Vec<usize> = content
			.match_indices("\r\n")
			.map(|(at, _)| at)
			.filter(|_| self.options.line_ending == formatter::LineEnding::Crlf)
			.collect();
		if has_unwritable_control(content, &crs) {
			return None;
		}
		self.syntax.promote.iter().find_map(|form| {
			let (open, close) = form.split_once("{}")?;
			(!close.is_empty() && !content.contains(close)).then(|| {
				(
					format!("{open}{content}{close}"),
					open.len()..open.len() + content.len(),
				)
			})
		})
	}

	/// Format `sql`, the string's content. A string written on one line
	/// stays on it when the SQL formats to one line and still fits, with
	/// `delimiters` columns of a literal's quotes and prefix (a content
	/// capture's are outside its range) and whatever is glued to its end
	/// (see `glued`). Otherwise it
	/// takes the vertical shape: SQL starting on the line after the
	/// opening quote, each line anchored to the host statement's
	/// indentation, and the closing quote on its own line at that indent,
	/// or at the margin with `close_at_margin`. A string that already
	/// spans lines keeps the vertical shape.
	fn format(
		&self,
		sql: &str,
		close_at_margin: bool,
		delimiters: usize,
	) -> Result<Anchored, Rewrite> {
		let one_line = !self.source[self.range.clone()].contains('\n');
		// The host statement's own indentation: the anchor every SQL line
		// hangs off, and — unless an indent style was configured — the
		// indent character too, so continuation lines don't mix tabs into
		// a spaces-indented file (or vice versa).
		let host_indent = line_indent(self.source, self.range.start);
		let mut format_options = *self.options;
		format_options.dialect = self.dialect;
		// LF from the formatter; the line endings are settled below,
		// where the lines are anchored.
		format_options.line_ending = formatter::LineEnding::Lf;
		if !self.indent.configured_style {
			format_options.indent_style =
				host_indent_style(self.source, &host_indent);
		}
		// Every line starts at the anchor, so the SQL gets the width left
		// after it — but never less than half, or a deeply nested string
		// would break at every opportunity.
		let tab = u16::from(format_options.indent_width);
		let anchor: u16 = host_indent
			.chars()
			.map(|c| if c == '\t' { tab } else { 1 })
			.fold(0, u16::saturating_add);
		let max_width = self.options.max_width;
		format_options.max_width =
			max_width.saturating_sub(anchor).max(max_width / 2);

		let lex_options = format_options.lex_options();
		let tokens = parser::lexer::lex_with(sql, self.dialect, lex_options);
		let parse = parser::parser::parse(&tokens, self.dialect);
		if let Some(diagnostic) = parse.diagnostics.first() {
			let dialect = match self.dialect {
				Dialect::Postgres => "Postgres",
				Dialect::Sqlite => "SQLite",
			};
			// A dialect the query chose, not the config: say so, or the
			// configured one would look ignored.
			let chosen =
				if self.pinned { ", the dialect its query sets" } else { "" };
			return Err(Rewrite::Warn(format!(
				"embedded SQL did not parse as {dialect}{chosen} ({}); left \
				 unformatted",
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
		if let Some(body) = formatted.body_diagnostics.first() {
			self
				.body_warning
				.replace(Some(format!("embedded SQL: {}", body.message)));
		}
		let sql = formatted.text.trim_end();

		if one_line && !sql.contains('\n') {
			let line_start =
				self.source[..self.range.start].rfind('\n').map_or(0, |at| at + 1);
			let column: usize = self.source[line_start..self.range.start]
				.chars()
				.map(|c| if c == '\t' { usize::from(tab) } else { 1 })
				.sum();
			// The closing delimiter, when the capture is the whole string,
			// and whatever is glued to the string's end, up to a space, `,`
			// or `;`: a content node's closing delimiter (`)"`, `"""`), and
			// a `)` closing the call, which can't break from it either.
			let closing = self.literal.end.saturating_sub(self.range.end);
			let end = self.literal.end.max(self.range.end);
			let glued = self.source[end..]
				.chars()
				.take_while(|c| !c.is_whitespace() && *c != ',' && *c != ';')
				.count();
			let width = column + delimiters + sql.chars().count() + closing + glued;
			if width <= usize::from(max_width) {
				return Ok(Anchored { text: sql.to_string(), line_ending_crs: vec![] });
			}
		}

		// Lines that start inside a value (a string spanning lines), or in
		// a body that didn't parse, are data: never indented, and the line
		// endings before them aren't ours to choose. Every other one is,
		// and with CRLF gets a CR, noted so it isn't taken for one the
		// string spelled.
		let verbatim_lines = formatter::verbatim_line_starts(sql, &format_options);
		let crlf = self.options.line_ending == formatter::LineEnding::Crlf;
		let mut anchored =
			Anchored { text: String::new(), line_ending_crs: vec![] };
		let end_line = |anchored: &mut Anchored| {
			if crlf {
				anchored.line_ending_crs.push(anchored.text.len());
				anchored.text.push('\r');
			}
			anchored.text.push('\n');
		};
		let mut line_start = 0;
		for (index, line) in sql.split('\n').enumerate() {
			if index == 0 || !verbatim_lines.contains(&line_start) {
				end_line(&mut anchored);
			} else {
				anchored.text.push('\n');
			}
			if !line.trim_end_matches('\r').is_empty()
				&& !verbatim_lines.contains(&line_start)
			{
				anchored.text.push_str(&host_indent);
			}
			anchored.text.push_str(line);
			line_start += line.len() + 1;
		}
		end_line(&mut anchored);
		if !close_at_margin {
			anchored.text.push_str(&host_indent);
		}
		Ok(anchored)
	}
}

/// SQL laid out for its host string, and where in it squill put a CR to
/// end a line in CRLF.
struct Anchored {
	text: String,
	line_ending_crs: Vec<usize>,
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

/// The indent step a spaces-indented host file uses: the most common
/// increase in indentation from one line to the next (ties go to the
/// smaller), not counting lines inside the SQL strings themselves. `None`
/// when the file never steps in with spaces.
fn host_indent_width(
	source: &str,
	strings: &[std::ops::Range<usize>],
) -> Option<u8> {
	let mut steps = [0usize; 9];
	let mut previous: Option<usize> = None;
	let mut offset = 0;
	for line in source.split('\n') {
		let start = offset;
		offset += line.len() + 1;
		if line.trim().is_empty()
			|| strings.iter().any(|string| string.start < start && start < string.end)
		{
			continue;
		}
		let indent = &line[..line.len() - line.trim_start().len()];
		if indent.contains('\t') {
			previous = None;
			continue;
		}
		let width = indent.len();
		// A step of one is a comment's ` * `, not an indent.
		if let Some(step) =
			previous.and_then(|previous| width.checked_sub(previous))
			&& (2..steps.len()).contains(&step)
		{
			steps[step] += 1;
		}
		previous = Some(width);
	}
	let (step, count) =
		steps.iter().enumerate().rev().max_by_key(|&(_, count)| count)?;
	(*count > 0).then_some(step as u8)
}

/// What follows each backslash in `text`, in order: the characters a
/// string that takes escapes would read as escape sequences.
fn escapes(text: &str) -> impl Iterator<Item = Option<char>> + '_ {
	text.match_indices('\\').map(|(at, _)| text[at + 1..].chars().next())
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

/// `line_ending_crs` are the CRs squill wrote to end lines in CRLF, as
/// asked: line endings, not characters only an escape can spell.
fn encode(
	decoded: &Decoded,
	content: &str,
	line_ending_crs: &[usize],
) -> Option<String> {
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
			if has_unwritable_control(content, line_ending_crs) {
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
			if content.contains('`')
				|| has_unwritable_control(content, line_ending_crs)
			{
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
/// hold faithfully, but for the CRs squill wrote to end lines (at the
/// byte offsets `line_ending_crs`, in order): a CR the string spelled
/// `\r` stays spelled out, by leaving the string as it was.
fn has_unwritable_control(content: &str, line_ending_crs: &[usize]) -> bool {
	content.char_indices().any(|(at, c)| {
		c.is_control()
			&& c != '\n'
			&& c != '\t'
			&& line_ending_crs.binary_search(&at).is_err()
	})
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

#[cfg(all(test, feature = "cxx"))]
mod tests {
	use super::*;

	#[test]
	fn an_edit_that_breaks_the_rest_of_the_file_is_blamed_alone() {
		// The first edit opens a raw string that runs to the end of the file,
		// so the second string no longer reads back either. Only the
		// first is at fault.
		let source = "void f() {\n  txn.exec(R\"(select 1)\");\n  txn.exec(R\"(select 2)\");\n}\n";
		let grammar = Grammar::from(Host::Cxx);
		grammar
			.with_parser(|ts, language| {
				let query = compile_query(language, CXX_SQL_QUERY)?;
				let tree = ts.parse(source, None).ok_or(EmbedError::HostParse)?;
				let extraction = Extraction {
					query: &query,
					default_dialect: Dialect::Postgres,
					codec: grammar.codec(),
				};
				let edits: Vec<Edit> = extraction
					.captures(&tree, source)
					.into_iter()
					.zip(["select 1)\"); R\"x(", "select  2"])
					.map(|(captured, replacement)| {
						Edit::content(captured.range, replacement.to_string())
					})
					.collect();
				let text = apply(source, &edits);
				let failed =
					extraction.verify(ts, &text, &edits, error_count(&tree), source)?;
				assert_eq!(failed, vec![0]);
				Ok(())
			})
			.unwrap();
	}
}
