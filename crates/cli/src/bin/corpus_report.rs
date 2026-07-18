//! Corpus coverage harness.
//!
//! Runs every `.sql` file under `corpus/` through lex -> parse -> emit and
//! reports pass/fail per file and per pipeline stage, plus statement-level
//! parse coverage (SELECT-ish statements tracked separately until DML/DDL
//! land with TREE-98).
//!
//! Usage: `corpus-report [--summary] [CORPUS_DIR]`
//!
//! Always exits 0 when the corpus was read; this is a report, not a gate.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use parser::Dialect;
use parser::lexer::LexOptions;
use parser::syntax::SyntaxKind;

/// The coder corpus is sqlc SQL: `@name` params enabled.
const LEX_OPTIONS: LexOptions = LexOptions { at_params: true };

/// Pipeline stages, in order.
const STAGES: [&str; 3] = ["lex", "parse", "emit"];

#[derive(Default)]
struct StmtStats {
    total: usize,
    ok: usize,
    select_total: usize,
    select_ok: usize,
}

struct FileResult {
    stages_passed: usize,
    error: Option<String>,
    stmts: StmtStats,
}

/// Run one file through the pipeline.
///
/// Lex passes when the token stream round-trips byte-for-byte with no
/// error tokens; parse passes when no statement lands as ErrorStatement;
/// emit passes when the formatter renders the tree.
fn run_file(source: &str, show_diagnostics: bool) -> FileResult {
    let fail = |stages_passed: usize, error: String| FileResult {
        stages_passed,
        error: Some(error),
        stmts: StmtStats::default(),
    };

    let tokens = parser::lexer::lex_with(source, Dialect::Postgres, LEX_OPTIONS);
    let rebuilt: String = tokens.iter().map(|t| t.text).collect();
    if rebuilt != source {
        return fail(0, "token texts do not round-trip to the input".into());
    }
    if let Some(first) = tokens.iter().find(|t| t.kind == SyntaxKind::Error) {
        let snippet: String = first.text.chars().take(20).collect();
        return fail(0, format!("error token: {snippet:?}"));
    }

    let parse = parser::parser::parse(&tokens, Dialect::Postgres);
    if parse.cst.text() != source {
        return fail(1, "parse tree does not round-trip to the input".into());
    }
    let mut stmts = StmtStats::default();
    for child in parse.cst.root().children() {
        match child.kind() {
            SyntaxKind::ErrorStatement => {
                stmts.total += 1;
                let tokens: Vec<_> = child
                    .children_with_tokens()
                    .filter_map(|element| element.into_token())
                    .filter(|token| !token.kind().is_trivia())
                    .collect();
                let starts_selectish = tokens.first().is_some_and(|token| {
                    token.kind() == SyntaxKind::LParen
                        || ["select", "with", "values", "table"]
                            .iter()
                            .any(|kw| token.text().eq_ignore_ascii_case(kw))
                });
                // Statements that involve DML — top-level (`WITH ...
                // UPDATE`) or in a data-modifying CTE (`AS (UPDATE ...)`)
                // — are TREE-98's problem, not SELECT failures. Careful
                // not to trip on `FOR [NO KEY] UPDATE` locking clauses.
                let mut depth = 0i32;
                let mut prev = String::new();
                let mut has_dml = false;
                for token in &tokens {
                    if token.kind() == SyntaxKind::LParen {
                        depth += 1;
                    } else if token.kind() == SyntaxKind::RParen {
                        depth -= 1;
                    } else if token.kind() == SyntaxKind::Ident {
                        let text = token.text().to_ascii_lowercase();
                        let is_dml_kw =
                            ["insert", "update", "delete", "merge"].contains(&text.as_str());
                        let after_lock_kws = ["for", "key", "no"].contains(&prev.as_str());
                        if is_dml_kw && (prev == "(" || (depth == 0 && !after_lock_kws)) {
                            has_dml = true;
                            break;
                        }
                    }
                    prev = token.text().to_ascii_lowercase();
                }
                if starts_selectish && !has_dml {
                    stmts.select_total += 1;
                    if show_diagnostics {
                        let snippet: String = child
                            .to_string()
                            .split_whitespace()
                            .collect::<Vec<_>>()
                            .join(" ")
                            .chars()
                            .take(120)
                            .collect();
                        println!("  select-ish failure: {snippet}");
                    }
                }
            }
            SyntaxKind::EmptyStmt => {}
            _ => {
                stmts.total += 1;
                stmts.ok += 1;
                stmts.select_total += 1;
                stmts.select_ok += 1;
            }
        }
    }
    if stmts.ok < stmts.total {
        let first = parse.diagnostics.first().expect("diagnostic per error");
        return FileResult {
            stages_passed: 1,
            error: Some(format!(
                "{}/{} statements failed; first at {}..{}: {}",
                stmts.total - stmts.ok,
                stmts.total,
                first.start,
                first.end,
                first.message
            )),
            stmts,
        };
    }

    let format_options = formatter::Options {
        at_params: true,
        ..formatter::Options::default()
    };
    let formatted = formatter::format_cst(&parse.cst, &format_options);
    if formatted.fallback_statements > 0 {
        return FileResult {
            stages_passed: 2,
            error: Some(format!(
                "{} statement(s) fell back to verbatim",
                formatted.fallback_statements
            )),
            stmts,
        };
    }
    FileResult {
        stages_passed: 3,
        error: None,
        stmts,
    }
}

/// Recursively collect all `.sql` files under `dir`.
fn collect_sql_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_sql_files(&path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "sql") {
            out.push(path);
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    let mut summary_only = false;
    let mut show_diagnostics = false;
    let mut root = PathBuf::from("corpus");
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--summary" => summary_only = true,
            "--diagnostics" => show_diagnostics = true,
            other => root = PathBuf::from(other),
        }
    }

    let mut files = Vec::new();
    if let Err(err) = collect_sql_files(&root, &mut files) {
        eprintln!(
            "corpus-report: cannot read corpus at {}: {err}",
            root.display()
        );
        return ExitCode::from(2);
    }
    files.sort();
    if files.is_empty() {
        eprintln!("corpus-report: no .sql files under {}", root.display());
        return ExitCode::from(2);
    }

    // stage_passes[i] counts files that passed stage i.
    let mut stage_passes = [0usize; STAGES.len()];
    let mut totals = StmtStats::default();
    let mut unreadable = 0usize;
    for path in &files {
        let source = match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(err) => {
                unreadable += 1;
                if !summary_only {
                    println!("fail:read   {} ({err})", path.display());
                }
                continue;
            }
        };
        let result = run_file(&source, show_diagnostics);
        for count in stage_passes.iter_mut().take(result.stages_passed) {
            *count += 1;
        }
        totals.total += result.stmts.total;
        totals.ok += result.stmts.ok;
        totals.select_total += result.stmts.select_total;
        totals.select_ok += result.stmts.select_ok;
        if !summary_only {
            match result.error {
                None => println!("ok          {}", path.display()),
                Some(msg) => println!(
                    "fail:{:<6} {} ({msg})",
                    STAGES[result.stages_passed],
                    path.display()
                ),
            }
        }
    }

    let total = files.len();
    println!("== corpus report: {} ({total} files) ==", root.display());
    for (stage, passes) in STAGES.iter().zip(stage_passes) {
        let pct = 100.0 * passes as f64 / total as f64;
        println!("{stage:<6} {passes:>5}/{total} ({pct:.1}%)");
    }
    println!("statements  {:>5}/{} parsed", totals.ok, totals.total);
    println!(
        "select-ish  {:>5}/{} parsed",
        totals.select_ok, totals.select_total
    );
    if unreadable > 0 {
        println!("unreadable: {unreadable}");
    }

    ExitCode::SUCCESS
}
