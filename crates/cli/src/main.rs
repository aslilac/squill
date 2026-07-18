//! squill: format SQL files.
//!
//! Thin CLI over the formatter library, suitable for pre-commit and CI.
//! `squill fmt <paths...>` writes in place; `--check` diffs and exits 1;
//! `--stdin`/`--stdout` stream. Flags only — no config file yet.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use formatter::{IdentQuoting, IndentStyle, KeywordCase, Options};
use parser::Dialect;
use rayon::prelude::*;

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
  -h, --help              Show this help
";

struct Args {
    check: bool,
    stdout_mode: bool,
    stdin_mode: bool,
    strict: bool,
    options: Options,
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
        options: Options::default(),
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
            "--at-params" => args.options.at_params = true,
            "--dialect" => {
                args.options.dialect = match value(&mut argv, "--dialect")?.as_str() {
                    "postgres" => Dialect::Postgres,
                    "sqlite" => Dialect::Sqlite,
                    other => return Err(format!("unknown dialect `{other}`")),
                }
            }
            "--indent" => {
                args.options.indent_style = match value(&mut argv, "--indent")?.as_str() {
                    "tab" | "tabs" => IndentStyle::Tab,
                    "spaces" => IndentStyle::Spaces,
                    other => return Err(format!("unknown indent style `{other}`")),
                }
            }
            "--indent-width" => {
                args.options.indent_width = value(&mut argv, "--indent-width")?
                    .parse()
                    .map_err(|_| "--indent-width needs a number".to_string())?
            }
            "--keyword-case" => {
                args.options.keyword_case = match value(&mut argv, "--keyword-case")?.as_str() {
                    "lower" => KeywordCase::Lower,
                    "upper" => KeywordCase::Upper,
                    other => return Err(format!("unknown keyword case `{other}`")),
                }
            }
            "--quote-idents" => {
                args.options.quoting = match value(&mut argv, "--quote-idents")?.as_str() {
                    "unquote-safe" => IdentQuoting::UnquotedWhenSafe,
                    "always" => IdentQuoting::AlwaysQuoted,
                    other => return Err(format!("unknown quoting mode `{other}`")),
                }
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

fn collect_files(path: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if path.is_dir() {
        let mut entries: Vec<_> = std::fs::read_dir(path)?
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<_, _>>()?;
        entries.sort();
        for entry in entries {
            if entry.is_dir() {
                collect_files(&entry, out)?;
            } else if entry.extension().is_some_and(|ext| ext == "sql") {
                out.push(entry);
            }
        }
    } else {
        out.push(path.to_path_buf());
    }
    Ok(())
}

fn print_diff(path: &str, before: &str, after: &str) {
    println!("--- {path}");
    println!("+++ {path} (formatted)");
    let diff = similar::TextDiff::from_lines(before, after);
    for hunk in diff.unified_diff().context_radius(2).iter_hunks() {
        print!("{hunk}");
    }
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
        let outcome = format_source(&source, &args.options);
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
        if let Err(err) = collect_files(path, &mut files) {
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

    let results: Vec<Result<FileResult, String>> = files
        .par_iter()
        .map(|path| {
            let source = std::fs::read_to_string(path)
                .map_err(|err| format!("{}: {err}", path.display()))?;
            let outcome = format_source(&source, &args.options);
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
