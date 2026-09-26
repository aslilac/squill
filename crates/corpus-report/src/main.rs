//! Corpus coverage harness.
//!
//! Runs every file of every corpus under `corpus/` (see the `corpus`
//! crate for which files, and each one's dialect) through lex -> parse ->
//! emit and reports pass/fail per file and per pipeline stage, plus
//! statement-level parse coverage, overall and per corpus.
//!
//! Usage: `corpus-report [--summary] [CORPUS_DIR]`
//!
//! Always exits 0 when the corpus was read; this is a report, not a gate.

use std::path::PathBuf;
use std::process::ExitCode;

use corpus::CorpusFile;
use parser::Dialect;
use parser::syntax::SyntaxKind;

/// Pipeline stages, in order.
const STAGES: [&str; 3] = ["lex", "parse", "emit"];

#[derive(Default)]
struct StmtStats {
	total: usize,
	ok: usize,
	select_total: usize,
	select_ok: usize,
	/// PL/pgSQL (dollar-quoted) bodies, parsed with the body grammar.
	bodies_total: usize,
	bodies_ok: usize,
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
fn run_file(
	source: &str,
	file: &CorpusFile,
	show_diagnostics: bool,
) -> FileResult {
	let dialect = file.dialect;
	let lex_options = file.lex_options;
	let fail = |stages_passed: usize, error: String| FileResult {
		stages_passed,
		error: Some(error),
		stmts: StmtStats::default(),
	};

	let tokens = parser::lexer::lex_with(source, dialect, lex_options);
	let rebuilt: String = tokens.iter().map(|t| t.text).collect();
	if rebuilt != source {
		return fail(0, "token texts do not round-trip to the input".into());
	}
	if let Some(first) = tokens.iter().find(|t| t.kind == SyntaxKind::Error) {
		let snippet: String = first.text.chars().take(20).collect();
		return fail(0, format!("error token: {snippet:?}"));
	}

	let parse = parser::parser::parse(&tokens, dialect);
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

	// PL/pgSQL body coverage: parse every plpgsql dollar-quoted body.
	if dialect == Dialect::Postgres
		&& source.to_ascii_lowercase().contains("plpgsql")
	{
		for token in tokens.iter().filter(|t| t.kind == SyntaxKind::DollarString) {
			let Some(open) = token.text[1..].find('$').map(|i| i + 2) else {
				continue;
			};
			let tag = &token.text[..open];
			let Some(body) = token.text[tag.len()..].strip_suffix(tag) else {
				continue;
			};
			if body.trim().is_empty() {
				continue;
			}
			stmts.bodies_total += 1;
			let body_tokens = parser::lexer::lex_with(body, dialect, lex_options);
			let body_parse =
				parser::parser::parse_plpgsql_body(&body_tokens, dialect);
			if body_parse.diagnostics.is_empty() {
				stmts.bodies_ok += 1;
			}
		}
	}

	let format_options = formatter::Options {
		dialect,
		at_params: lex_options.at_params,
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
	FileResult { stages_passed: 3, error: None, stmts }
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

	let files = match corpus::files(&root) {
		Ok(files) => files,
		Err(err) => {
			eprintln!(
				"corpus-report: cannot read corpus at {}: {err}",
				root.display()
			);
			return ExitCode::from(2);
		}
	};
	if files.is_empty() {
		eprintln!("corpus-report: no corpus files under {}", root.display());
		return ExitCode::from(2);
	}

	// stage_passes[i] counts files that passed stage i.
	let mut stage_passes = [0usize; STAGES.len()];
	let mut totals = StmtStats::default();
	let mut unreadable = 0usize;
	// Per corpus, in first-seen order: (name, files, files through emit,
	// statements parsed, statements).
	let mut per_corpus: Vec<(&str, usize, usize, usize, usize)> = Vec::new();
	for file in &files {
		let path = &file.path;
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
		let result = run_file(&source, file, show_diagnostics);
		let at = match per_corpus.iter().position(|row| row.0 == file.corpus) {
			Some(at) => at,
			None => {
				per_corpus.push((file.corpus, 0, 0, 0, 0));
				per_corpus.len() - 1
			}
		};
		let row = &mut per_corpus[at];
		row.1 += 1;
		row.2 += usize::from(result.stages_passed == STAGES.len());
		row.3 += result.stmts.ok;
		row.4 += result.stmts.total;
		for count in stage_passes.iter_mut().take(result.stages_passed) {
			*count += 1;
		}
		totals.total += result.stmts.total;
		totals.ok += result.stmts.ok;
		totals.select_total += result.stmts.select_total;
		totals.select_ok += result.stmts.select_ok;
		totals.bodies_total += result.stmts.bodies_total;
		totals.bodies_ok += result.stmts.bodies_ok;
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
	println!(
		"pl bodies   {:>5}/{} parsed",
		totals.bodies_ok, totals.bodies_total
	);
	for (name, total, clean, ok, statements) in per_corpus {
		println!(
			"  {name:<12} {clean:>5}/{total} files clean, {ok:>5}/{statements} statements parsed"
		);
	}
	if unreadable > 0 {
		println!("unreadable: {unreadable}");
	}

	ExitCode::SUCCESS
}
