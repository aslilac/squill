//! Format SQL embedded in host-language files, located via tree-sitter
//! queries (TREE-100).
//!
//! The extraction query is the config surface: captures named
//! `@sql.postgres` / `@sql.sqlite` (or bare `@sql`, which uses the
//! caller's dialect) mark string literals whose contents are SQL.
//! Snippets that fail to parse cleanly are left byte-identical — a host
//! file is never a hard error. Edits apply back-to-front by byte range.
//!
//! Predicate note: only `#eq?` and `#any-of?` are supported, keeping the
//! no-regex rule — `#match?` is rejected up front.

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser as TsParser, Query, QueryCursor, QueryPredicateArg};

use formatter::Options;
use parser::Dialect;

/// The host languages with built-in grammars and string codecs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Rust,
    Go,
}

impl Host {
    fn language(self) -> Language {
        match self {
            Host::Rust => tree_sitter_rust::LANGUAGE.into(),
            Host::Go => tree_sitter_go::LANGUAGE.into(),
        }
    }
}

/// Default extraction query for Rust: the string-literal argument of
/// `sqlx::query!` / `query_as!` / `query_scalar!` / `query_unchecked!`
/// macro invocations (any path whose last segment matches).
pub const RUST_SQLX_QUERY: &str = r#"
((macro_invocation
   macro: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
   ]
   (token_tree
     [(string_literal) (raw_string_literal)] @sql.postgres))
 (#any-of? @_name "query" "query_as" "query_scalar" "query_unchecked"))
"#;

/// Default extraction query for Go: string arguments of `.Query`-family
/// method calls (`database/sql` style).
pub const GO_DB_QUERY: &str = r#"
((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list
     [(raw_string_literal) (interpreted_string_literal)] @sql.postgres))
 (#any-of? @_method
   "Query" "QueryRow" "Exec"
   "QueryContext" "QueryRowContext" "ExecContext"))
"#;

#[derive(Debug)]
pub enum EmbedError {
    /// The host source did not parse with the tree-sitter grammar.
    HostParse,
    /// The extraction query is invalid (or uses an unsupported predicate
    /// such as `#match?`).
    Query(String),
}

impl std::fmt::Display for EmbedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbedError::HostParse => f.write_str("host file did not parse"),
            EmbedError::Query(message) => write!(f, "invalid extraction query: {message}"),
        }
    }
}

impl std::error::Error for EmbedError {}

/// Format every SQL snippet the query captures in `source`, returning the
/// rewritten host file. Unparseable or unsafe snippets stay byte-exact.
pub fn format_embedded(
    source: &str,
    host: Host,
    query_source: &str,
    options: &Options,
) -> Result<String, EmbedError> {
    // Reject regex predicates up front (the no-regex rule). The
    // tree-sitter binding would happily evaluate them, so refuse by
    // inspection before building the query.
    if query_source.contains("#match?") || query_source.contains("#not-match?") {
        return Err(EmbedError::Query(
            "regex predicates (#match?) are not supported; use #eq? / #any-of?".to_string(),
        ));
    }
    let language = host.language();
    let query =
        Query::new(&language, query_source).map_err(|err| EmbedError::Query(err.to_string()))?;
    // Anything the binding does not evaluate natively would be silently
    // ignored: reject unknown custom predicates too.
    for pattern in 0..query.pattern_count() {
        for predicate in query.general_predicates(pattern) {
            match predicate.operator.as_ref() {
                "eq?" | "any-of?" => {}
                other => {
                    return Err(EmbedError::Query(format!(
                        "unsupported predicate `#{other}?`"
                    )));
                }
            }
        }
    }

    let mut ts = TsParser::new();
    ts.set_language(&language)
        .map_err(|err| EmbedError::Query(err.to_string()))?;
    let tree = ts.parse(source, None).ok_or(EmbedError::HostParse)?;

    // Collect (byte range, replacement) edits.
    let mut edits: Vec<(std::ops::Range<usize>, String)> = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source.as_bytes());
    while let Some(query_match) = matches.next() {
        if !predicates_hold(&query, query_match, source) {
            continue;
        }
        for capture in query_match.captures {
            let name = &query.capture_names()[capture.index as usize];
            let Some(dialect) = sql_dialect(name, options.dialect) else {
                continue;
            };
            let node = capture.node;
            let literal = &source[node.byte_range()];
            if let Some(replacement) =
                rewrite_literal(source, node.start_byte(), literal, host, dialect, options)
                && replacement != literal
            {
                edits.push((node.byte_range(), replacement));
            }
        }
    }

    // Back-to-front so earlier ranges stay valid; drop duplicates and
    // overlaps.
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    edits.dedup_by(|a, b| a.0 == b.0);
    let mut out = source.to_string();
    let mut last_start = usize::MAX;
    for (range, replacement) in edits {
        if range.end > last_start {
            continue; // overlapping capture; keep the edit already applied
        }
        last_start = range.start;
        out.replace_range(range, &replacement);
    }
    Ok(out)
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
    query
        .general_predicates(query_match.pattern_index)
        .iter()
        .all(|predicate| {
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
                "any-of?" => args
                    .any(|arg| matches!(arg, QueryPredicateArg::String(s) if s.as_ref() == text)),
                _ => false,
            }
        })
}

/// Format one captured literal; `None` leaves it untouched.
fn rewrite_literal(
    source: &str,
    literal_start: usize,
    literal: &str,
    host: Host,
    dialect: Dialect,
    options: &Options,
) -> Option<String> {
    let decoded = decode(host, literal)?;
    if decoded.content.trim().is_empty() {
        return None;
    }
    // Multiline-capable literal kinds (raw strings, Go backticks) always
    // reformat into the vertical shape — even when currently single-line.
    // Plain quoted strings only reformat if the author already made them
    // multi-line; single-line plain strings stay byte-identical.
    let multiline_capable = matches!(
        decoded.kind,
        LiteralKind::RustRaw { .. } | LiteralKind::GoRaw
    );
    if !multiline_capable && !decoded.content.contains('\n') {
        return None;
    }

    // Match the host file's indentation character so continuation lines
    // don't mix tabs into a spaces-indented file (or vice versa).
    let host_indent = line_indent(source, literal_start);
    let mut format_options = *options;
    format_options.dialect = dialect;
    // The author chose a multi-line literal: keep statements
    // clause-per-line, never collapsed onto one line.
    format_options.always_break_statements = true;
    format_options.indent_style = if host_indent.contains(' ') {
        formatter::IndentStyle::Spaces
    } else {
        formatter::IndentStyle::Tab
    };

    let lex_options = format_options.lex_options();
    let tokens = parser::lexer::lex_with(&decoded.content, dialect, lex_options);
    let parse = parser::parser::parse(&tokens, dialect);
    if !parse.diagnostics.is_empty() {
        return None; // not (entirely) SQL: leave byte-identical
    }
    let formatted = formatter::format_cst(&parse.cst, &format_options);
    if formatted.fallback_statements > 0 {
        return None;
    }
    let sql = formatted.text.trim_end().to_string();

    // Quotes on their own lines: the SQL starts on the line after the
    // opening quote, each line anchored to the host statement's
    // indentation, and the closing quote on its own line at that indent.
    let mut anchored = String::new();
    for line in sql.split('\n') {
        anchored.push('\n');
        if !line.is_empty() {
            anchored.push_str(&host_indent);
            anchored.push_str(line);
        }
    }
    anchored.push('\n');
    anchored.push_str(&host_indent);

    encode(&decoded, &anchored)
}

/// Leading whitespace of the line containing `offset`.
fn line_indent(source: &str, offset: usize) -> String {
    let line_start = source[..offset].rfind('\n').map_or(0, |pos| pos + 1);
    source[line_start..]
        .chars()
        .take_while(|&c| c == ' ' || c == '\t')
        .collect()
}

/// A decoded string literal: its SQL content plus enough shape to
/// re-encode.
struct Decoded {
    content: String,
    kind: LiteralKind,
}

enum LiteralKind {
    /// Rust `"..."` (escapes) — multi-line output allowed.
    RustPlain,
    /// Rust `r#"..."#` with N hashes — content is verbatim.
    RustRaw { hashes: usize },
    /// Go `` `...` `` — verbatim, but cannot contain a backtick.
    GoRaw,
    /// Go `"..."` (escapes) — single-line only.
    GoPlain,
}

fn decode(host: Host, literal: &str) -> Option<Decoded> {
    match host {
        Host::Rust => {
            if let Some(rest) = literal.strip_prefix('r') {
                let hashes = rest.chars().take_while(|&c| c == '#').count();
                let body = rest[hashes..]
                    .strip_prefix('"')?
                    .strip_suffix(&format!("\"{}", "#".repeat(hashes)))?;
                Some(Decoded {
                    content: body.to_string(),
                    kind: LiteralKind::RustRaw { hashes },
                })
            } else {
                let body = literal.strip_prefix('"')?.strip_suffix('"')?;
                Some(Decoded {
                    content: unescape(body, true)?,
                    kind: LiteralKind::RustPlain,
                })
            }
        }
        Host::Go => {
            if let Some(body) = literal.strip_prefix('`') {
                Some(Decoded {
                    content: body.strip_suffix('`')?.to_string(),
                    kind: LiteralKind::GoRaw,
                })
            } else {
                let body = literal.strip_prefix('"')?.strip_suffix('"')?;
                Some(Decoded {
                    content: unescape(body, false)?,
                    kind: LiteralKind::GoPlain,
                })
            }
        }
    }
}

/// Decode `\`-escapes shared by Rust and Go plain strings. Rust adds the
/// line-continuation escape (backslash-newline swallows leading
/// whitespace).
fn unescape(body: &str, rust: bool) -> Option<String> {
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
            'x' => {
                let hex: String = chars.by_ref().take(2).collect();
                out.push(u8::from_str_radix(&hex, 16).ok()? as char);
            }
            'u' if rust => {
                if chars.next()? != '{' {
                    return None;
                }
                let hex: String = chars.by_ref().take_while(|&c| c != '}').collect();
                out.push(char::from_u32(u32::from_str_radix(&hex, 16).ok()?)?);
            }
            '\n' if rust => {
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
            // Rust plain strings support literal newlines and tabs.
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
        LiteralKind::GoRaw => {
            if content.contains('`') {
                None // cannot be represented; leave untouched
            } else {
                Some(format!("`{content}`"))
            }
        }
        LiteralKind::GoPlain => {
            if content.contains('\n') {
                None // interpreted strings are single-line only
            } else {
                let mut out = String::with_capacity(content.len() + 2);
                out.push('"');
                for c in content.chars() {
                    match c {
                        '\\' => out.push_str("\\\\"),
                        '"' => out.push_str("\\\""),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
                Some(out)
            }
        }
    }
}

/// Fewest `#`s a Rust raw string needs to hold `content`.
fn min_raw_hashes(content: &str) -> usize {
    let mut needed = 0;
    let bytes = content.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let run = bytes[index + 1..]
                .iter()
                .take_while(|&&b| b == b'#')
                .count();
            needed = needed.max(run + 1);
            index += run + 1;
        } else {
            index += 1;
        }
    }
    needed
}
