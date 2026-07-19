//! squill: format SQL files.
//!
//! Thin CLI over the formatter library, suitable for pre-commit and CI.
//! `squill fmt <paths...>` writes in place; `--check` diffs and exits 1;
//! `--stdin`/`--stdout` stream. Options come from the nearest
//! `squill.toml` (see `config`), overridden by explicit flags.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use formatter::Options;
use rayon::prelude::*;

mod config;
use config::PartialOptions;

const USAGE: &str = "\
squill — a SQL formatter

Usage: squill fmt [OPTIONS] [PATHS...]

Formats the given .sql files in place (directories are searched
recursively). Reads stdin when --stdin is given.

Options:
  --check                 Don't write; print diffs and exit 1 if any file
                          would change
  --stdout                Print formatted output instead of writing files
  --stdin                 Read from stdin, write to stdout
  --strict                Exit 1 when statements could not be parsed and
                          passed through verbatim
  --dialect <D>           postgres (default) | sqlite
  --indent <STYLE>        tab (default) | spaces
  --indent-width <N>      Indent width (and tab measure), default 2
  --keyword-case <CASE>   lower (default) | upper
  --quote-idents <MODE>   unquote-safe (default) | always
  --at-params             Treat sqlc-style @name as parameters (Postgres)
  --no-config             Ignore squill.toml files
  --embedded              Also format SQL embedded in host files (.rs,
                          .go, .py, .js/.ts/.tsx, .gleam) when recursing
                          directories (explicit host paths always format)
  --embedded-query <SCM>  Override the tree-sitter extraction query
  -h, --help              Show this help

Configuration: the nearest squill.toml at or above each formatted file
supplies defaults (keys: dialect, indent, indent-width, keyword-case,
quote-idents, at-params). Explicit flags override the config.
";

struct Args {
    check: bool,
    stdout_mode: bool,
    stdin_mode: bool,
    strict: bool,
    no_config: bool,
    /// Include host-language files when recursing directories.
    embed: bool,
    /// Override the tree-sitter extraction query (.scm source).
    embed_query: Option<String>,
    overrides: PartialOptions,
    paths: Vec<PathBuf>,
}

fn parse_args() -> Result<Args, String> {
    let mut argv = std::env::args().skip(1).peekable();
    match argv.next().as_deref() {
        Some("fmt") => {}
        Some("-h" | "--help") | None => return Err(USAGE.to_string()),
        Some(other) => return Err(format!("unknown command `{other}`\n\n{USAGE}")),
    }
    let mut args = Args {
        check: false,
        stdout_mode: false,
        stdin_mode: false,
        strict: false,
        no_config: false,
        embed: false,
        embed_query: None,
        overrides: PartialOptions::default(),
        paths: Vec::new(),
    };
    let value = |argv: &mut dyn Iterator<Item = String>, flag: &str| {
        argv.next().ok_or_else(|| format!("{flag} needs a value"))
    };
    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "--check" => args.check = true,
            "--stdout" => args.stdout_mode = true,
            "--stdin" => args.stdin_mode = true,
            "--strict" => args.strict = true,
            "--no-config" => args.no_config = true,
            "--embedded" => args.embed = true,
            "--embedded-query" => {
                let path = value(&mut argv, "--embedded-query")?;
                args.embed_query = Some(
                    std::fs::read_to_string(&path)
                        .map_err(|err| format!("--embedded-query {path}: {err}"))?,
                );
            }
            "--at-params" => args.overrides.at_params = Some(true),
            "--dialect" => {
                args.overrides.dialect =
                    Some(config::parse_dialect(&value(&mut argv, "--dialect")?)?)
            }
            "--indent" => {
                args.overrides.indent_style =
                    Some(config::parse_indent(&value(&mut argv, "--indent")?)?)
            }
            "--indent-width" => {
                args.overrides.indent_width = Some(
                    value(&mut argv, "--indent-width")?
                        .parse()
                        .map_err(|_| "--indent-width needs a number".to_string())?,
                )
            }
            "--keyword-case" => {
                args.overrides.keyword_case = Some(config::parse_keyword_case(&value(
                    &mut argv,
                    "--keyword-case",
                )?)?)
            }
            "--quote-idents" => {
                args.overrides.quoting =
                    Some(config::parse_quoting(&value(&mut argv, "--quote-idents")?)?)
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            flag if flag.starts_with('-') => {
                return Err(format!("unknown flag `{flag}`\n\n{USAGE}"));
            }
            path => args.paths.push(PathBuf::from(path)),
        }
    }
    if args.stdin_mode && !args.paths.is_empty() {
        return Err("--stdin cannot be combined with paths".to_string());
    }
    if !args.stdin_mode && args.paths.is_empty() {
        return Err(format!("no input files\n\n{USAGE}"));
    }
    Ok(args)
}

/// Resolve effective options for a file in `dir`: defaults, then the
/// nearest squill.toml (unless --no-config), then explicit flags.
fn resolve_options(
    dir: &Path,
    args: &Args,
    cache: &mut std::collections::HashMap<PathBuf, PartialOptions>,
) -> Result<Options, String> {
    let mut options = Options::default();
    if !args.no_config
        && let Some(config_path) = config::discover(dir)
    {
        let partial = match cache.get(&config_path) {
            Some(partial) => *partial,
            None => {
                let text = std::fs::read_to_string(&config_path)
                    .map_err(|err| format!("{}: {err}", config_path.display()))?;
                let partial = config::parse_config(&text, &config_path)?;
                cache.insert(config_path.clone(), partial);
                partial
            }
        };
        partial.apply(&mut options);
    }
    args.overrides.apply(&mut options);
    Ok(options)
}

/// The embed host for a path, by extension.
fn host_for(path: &Path) -> Option<embed::Host> {
    match path.extension()?.to_str()? {
        "rs" => Some(embed::Host::Rust),
        "go" => Some(embed::Host::Go),
        "py" => Some(embed::Host::Python),
        "js" | "mjs" | "cjs" | "jsx" => Some(embed::Host::JavaScript),
        "ts" | "mts" | "cts" => Some(embed::Host::TypeScript),
        "tsx" => Some(embed::Host::Tsx),
        "gleam" => Some(embed::Host::Gleam),
        _ => None,
    }
}

/// Default extraction query for a host.
fn default_query(host: embed::Host) -> &'static str {
    match host {
        embed::Host::Rust => embed::RUST_SQLX_QUERY,
        embed::Host::Go => embed::GO_DB_QUERY,
        embed::Host::Python => embed::PYTHON_DB_QUERY,
        embed::Host::JavaScript | embed::Host::TypeScript | embed::Host::Tsx => embed::JS_SQL_QUERY,
        embed::Host::Gleam => embed::GLEAM_SQL_QUERY,
    }
}

/// One formatted source, with human-readable diagnostics.
struct Outcome {
    formatted: String,
    /// `line:col: message` diagnostics (ErrorStatements + verbatim
    /// fallbacks).
    diagnostics: Vec<String>,
}

fn format_source(source: &str, options: &Options) -> Outcome {
    let tokens = parser::lexer::lex_with(source, options.dialect, options.lex_options());
    let parse = parser::parser::parse(&tokens, options.dialect);
    let result = formatter::format_cst(&parse.cst, options);
    let mut diagnostics: Vec<String> = parse
        .diagnostics
        .iter()
        .map(|d| {
            let (line, col) = line_col(source, d.start);
            format!(
                "{line}:{col}: {} (statement passed through verbatim)",
                d.message
            )
        })
        .collect();
    if result.fallback_statements > 0 {
        diagnostics.push(format!(
            "{} statement(s) passed through verbatim (formatter self-check)",
            result.fallback_statements
        ));
    }
    Outcome {
        formatted: result.text,
        diagnostics,
    }
}

/// 1-based line and column for a byte offset.
fn line_col(source: &str, offset: usize) -> (usize, usize) {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() + 1;
    let col = before.chars().rev().take_while(|&c| c != '\n').count() + 1;
    (line, col)
}

fn collect_files(path: &Path, embed: bool, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect_files(&entry, embed, out)?;
            } else if entry
                .extension()
                .is_some_and(|ext| ext == "sql" || (embed && (ext == "rs" || ext == "go")))
            {
                out.push(entry);
            }
        }
    } else {
        out.push(path.to_path_buf());
    }
    Ok(())
}

/// Above this many (estimated) changed lines, a fine-grained diff is
/// neither readable nor cheap — Myers is O(lines × edits) and goes
/// quadratic on a first format of a large file.
const FINE_DIFF_EDIT_LIMIT: usize = 1000;

fn print_diff(path: &str, before: &str, after: &str) {
    println!("--- {path}");
    println!("+++ {path} (formatted)");
    // The guard must be deterministic (a wall-clock deadline would make
    // --check output machine-dependent): estimate the edit volume from
    // line multisets in O(lines), and print a whole-file replacement
    // hunk when a fine diff is not worth computing.
    if estimated_edits(before, after) > FINE_DIFF_EDIT_LIMIT {
        let before_lines: Vec<&str> = before.lines().collect();
        let after_lines: Vec<&str> = after.lines().collect();
        println!("@@ -1,{} +1,{} @@", before_lines.len(), after_lines.len());
        for line in before_lines {
            println!("-{line}");
        }
        for line in after_lines {
            println!("+{line}");
        }
        return;
    }
    let diff = similar::TextDiff::from_lines(before, after);
    for hunk in diff.unified_diff().context_radius(2).iter_hunks() {
        print!("{hunk}");
    }
}

/// Lower bound on the diff's edit count: lines whose occurrence counts
/// differ between the two texts (order-insensitive, so cheap).
fn estimated_edits(before: &str, after: &str) -> usize {
    let mut counts: std::collections::HashMap<&str, isize> = std::collections::HashMap::new();
    for line in before.lines() {
        *counts.entry(line).or_default() += 1;
    }
    for line in after.lines() {
        *counts.entry(line).or_default() -= 1;
    }
    counts.values().map(|count| count.unsigned_abs()).sum()
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::from(2);
        }
    };

    if args.stdin_mode {
        let mut source = String::new();
        if let Err(err) = std::io::stdin().read_to_string(&mut source) {
            eprintln!("squill: cannot read stdin: {err}");
            return ExitCode::from(2);
        }
        let mut cache = std::collections::HashMap::new();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let options = match resolve_options(&cwd, &args, &mut cache) {
            Ok(options) => options,
            Err(message) => {
                eprintln!("squill: {message}");
                return ExitCode::from(2);
            }
        };
        let outcome = format_source(&source, &options);
        for diagnostic in &outcome.diagnostics {
            eprintln!("<stdin>:{diagnostic}");
        }
        if args.check {
            if outcome.formatted != source {
                print_diff("<stdin>", &source, &outcome.formatted);
                return ExitCode::from(1);
            }
        } else {
            print!("{}", outcome.formatted);
        }
        if args.strict && !outcome.diagnostics.is_empty() {
            return ExitCode::from(1);
        }
        return ExitCode::SUCCESS;
    }

    let mut files = Vec::new();
    for path in &args.paths {
        if let Err(err) = collect_files(path, args.embed, &mut files) {
            eprintln!("squill: {}: {err}", path.display());
            return ExitCode::from(2);
        }
    }
    files.sort();
    files.dedup();

    struct FileResult {
        path: PathBuf,
        source: String,
        outcome: Outcome,
    }

    // Resolve options per file up front (sequential, cached per config
    // path); a config error is a hard error before any file is touched.
    let mut cache = std::collections::HashMap::new();
    let mut per_file_options = Vec::with_capacity(files.len());
    for path in &files {
        let dir = path.parent().unwrap_or_else(|| Path::new("."));
        match resolve_options(dir, &args, &mut cache) {
            Ok(options) => per_file_options.push(options),
            Err(message) => {
                eprintln!("squill: {message}");
                return ExitCode::from(2);
            }
        }
    }

    let results: Vec<Result<FileResult, String>> = files
        .par_iter()
        .zip(per_file_options.par_iter())
        .map(|(path, options)| {
            let source = std::fs::read_to_string(path)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            let outcome = match host_for(path) {
                Some(host) => {
                    // SQL embedded in a host-language file (sqlx macros,
                    // database/sql calls) via the tree-sitter engine.
                    let query = args
                        .embed_query
                        .as_deref()
                        .unwrap_or_else(|| default_query(host));
                    let formatted = embed::format_embedded(&source, host, query, options)
                        .map_err(|err| format!("{}: {err}", path.display()))?;
                    Outcome {
                        formatted,
                        diagnostics: Vec::new(),
                    }
                }
                None => format_source(&source, options),
            };
            Ok(FileResult {
                path: path.clone(),
                source,
                outcome,
            })
        })
        .collect();

    let mut changed = 0usize;
    let mut diagnostics = 0usize;
    let mut io_error = false;
    // Deterministic output: results arrive in input order.
    for result in results {
        let result = match result {
            Ok(result) => result,
            Err(message) => {
                eprintln!("squill: {message}");
                io_error = true;
                continue;
            }
        };
        let path = result.path.display().to_string();
        for diagnostic in &result.outcome.diagnostics {
            eprintln!("{path}:{diagnostic}");
            diagnostics += 1;
        }
        if result.outcome.formatted != result.source {
            changed += 1;
            if args.check {
                print_diff(&path, &result.source, &result.outcome.formatted);
            } else if args.stdout_mode {
                print!("{}", result.outcome.formatted);
            } else if let Err(err) = std::fs::write(&result.path, &result.outcome.formatted) {
                eprintln!("squill: {path}: {err}");
                io_error = true;
            } else {
                println!("{path}");
            }
        } else if args.stdout_mode {
            print!("{}", result.outcome.formatted);
        }
    }

    if args.check {
        eprintln!(
            "{} file(s) checked, {changed} would be reformatted, {diagnostics} diagnostic(s)",
            files.len()
        );
    } else if !args.stdout_mode {
        eprintln!(
            "{} file(s) checked, {changed} reformatted, {diagnostics} diagnostic(s)",
            files.len()
        );
    }

    if io_error {
        ExitCode::from(2)
    } else if (args.check && changed > 0) || (args.strict && diagnostics > 0) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
