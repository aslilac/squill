//! Corpus coverage harness.
//!
//! Runs every `.sql` file under `corpus/` through lex -> parse -> emit and
//! reports pass/fail per file and per pipeline stage. Later issues use this
//! report to measure grammar coverage.
//!
//! Usage: `corpus-report [--summary] [CORPUS_DIR]`
//!
//! Always exits 0 when the corpus was read; this is a report, not a gate.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Pipeline stages, in order.
const STAGES: [&str; 3] = ["lex", "parse", "emit"];

/// Run one file through the pipeline. Returns the number of stages passed
/// (0..=3) and the error message of the first failing stage, if any.
fn run_file(source: &str) -> (usize, Option<String>) {
    let tokens = match parser::lexer::lex(source) {
        Ok(tokens) => tokens,
        Err(err) => return (0, Some(err.to_string())),
    };
    let cst = match parser::parser::parse(&tokens) {
        Ok(cst) => cst,
        Err(err) => return (1, Some(err.to_string())),
    };
    match formatter::emit(&cst) {
        Ok(_) => (3, None),
        Err(err) => (2, Some(err.to_string())),
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
    let mut root = PathBuf::from("corpus");
    for arg in std::env::args().skip(1) {
        match arg.as_str() {
            "--summary" => summary_only = true,
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
        let (passed, error) = run_file(&source);
        for count in stage_passes.iter_mut().take(passed) {
            *count += 1;
        }
        if !summary_only {
            match error {
                None => println!("ok          {}", path.display()),
                Some(msg) => {
                    println!("fail:{:<6} {} ({msg})", STAGES[passed], path.display());
                }
            }
        }
    }

    let total = files.len();
    println!("== corpus report: {} ({total} files) ==", root.display());
    for (stage, passes) in STAGES.iter().zip(stage_passes) {
        let pct = 100.0 * passes as f64 / total as f64;
        println!("{stage:<6} {passes:>5}/{total} ({pct:.1}%)");
    }
    if unreadable > 0 {
        println!("unreadable: {unreadable}");
    }

    ExitCode::SUCCESS
}
