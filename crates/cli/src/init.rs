//! `squill init`: write a starter squill.toml (or, with `--yaml`,
//! squill.yaml) for the project in the
//! working directory.
//!
//! The wizard lists every built-in grammar as a checkbox, pre-checking
//! the languages that already hold SQL — found by walking the tree the
//! way `fmt` does (honoring .gitignore, skipping hidden files) and
//! running each grammar's default query over its files. Then it asks for
//! the dialect of plain .sql files and of each chosen language, and which
//! placeholder styles the code writes (none checked: that depends on the
//! driver, not the language). A new project with nothing to find simply
//! starts with nothing checked.
//!
//! Without a terminal to ask on (or with `--yes`), every default is
//! taken: the languages found, in the `--dialect` dialect.

use std::io::IsTerminal;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;

use dialoguer::MultiSelect;
use dialoguer::Select;
use dialoguer::console::Term;
use dialoguer::theme::ColorfulTheme;
use parser::Dialect;
use rayon::prelude::*;

pub const USAGE: &str = "\
Usage: squill init [OPTIONS]

Writes a squill.toml (or squill.yaml) for the project in the working
directory. squill lists the languages it has grammars for — checking
the ones where it finds SQL already — and asks which to format, in
which dialect, and which placeholder styles your queries use.

Options:
  --dialect <D>   postgres (default) | sqlite: the dialect every answer
                  starts from
  --yaml          Write squill.yaml instead of squill.toml
  -y, --yes       Take every default without asking (also what happens
                  without a terminal)
  -h, --help      Show this help
";

pub struct InitArgs {
	yes: bool,
	dialect: Dialect,
	yaml: bool,
}

pub fn parse(
	mut argv: impl Iterator<Item = String>,
) -> Result<InitArgs, String> {
	let mut args =
		InitArgs { yes: false, dialect: Dialect::Postgres, yaml: false };
	while let Some(arg) = argv.next() {
		match arg.as_str() {
			"-y" | "--yes" => args.yes = true,
			"--yaml" => args.yaml = true,
			"--dialect" => {
				let value =
					argv.next().ok_or_else(|| "--dialect needs a value".to_string())?;
				args.dialect = crate::config::parse_dialect(&value)?;
			}
			other => return Err(format!("unknown argument `{other}`\n\n{USAGE}")),
		}
	}
	Ok(args)
}

/// What the survey found for one language.
struct Found {
	files: usize,
	strings: usize,
}

/// A language's name the way people write it.
fn display_name(host: embed::Host) -> &'static str {
	match host.name() {
		"rust" => "Rust",
		"go" => "Go",
		"python" => "Python",
		"javascript" => "JavaScript",
		"typescript" => "TypeScript",
		"tsx" => "TSX",
		"gleam" => "Gleam",
		"c++" => "C++",
		"csharp" => "C#",
		"java" => "Java",
		"kotlin" => "Kotlin",
		"swift" => "Swift",
		other => other,
	}
}

fn dialect_name(dialect: Dialect) -> &'static str {
	match dialect {
		Dialect::Postgres => "postgres",
		Dialect::Sqlite => "sqlite",
	}
}

fn host_for(path: &Path) -> Option<embed::Host> {
	let extension = path.extension()?.to_str()?;
	embed::Host::ALL
		.iter()
		.copied()
		.find(|host| host.extensions().contains(&extension))
}

/// Walk `root` and count the SQL each built-in grammar's default query
/// finds (in [`embed::Host::ALL`] order), plus the plain .sql files.
fn survey(root: &Path) -> (usize, Vec<Found>) {
	let mut sql_files = 0;
	let mut candidates: Vec<(PathBuf, embed::Host)> = Vec::new();
	for entry in ignore::WalkBuilder::new(root).build().flatten() {
		if !entry.file_type().is_some_and(|kind| kind.is_file()) {
			continue;
		}
		let path = entry.into_path();
		if path.extension().is_some_and(|ext| ext == "sql") {
			sql_files += 1;
		} else if let Some(host) = host_for(&path) {
			candidates.push((path, host));
		}
	}
	let counts: Vec<(embed::Host, usize)> = candidates
		.par_iter()
		.map(|(path, host)| {
			let strings = std::fs::read_to_string(path)
				.ok()
				.and_then(|source| {
					embed::count_sql(&source, &(*host).into(), host.default_query()).ok()
				})
				.unwrap_or(0);
			(*host, strings)
		})
		.collect();
	let found = embed::Host::ALL
		.iter()
		.map(|&host| {
			let mine: Vec<usize> = counts
				.iter()
				.filter(|(of, strings)| *of == host && *strings > 0)
				.map(|(_, strings)| *strings)
				.collect();
			Found { files: mine.len(), strings: mine.iter().sum() }
		})
		.collect();
	(sql_files, found)
}

fn plural(n: usize, one: &str, many: &str) -> String {
	match n {
		1 => format!("1 {one}"),
		n => format!("{n} {many}"),
	}
}

/// The answers the config is written from.
struct Answers {
	/// The top-level dialect: plain .sql files, and every rule that
	/// doesn't say otherwise.
	dialect: Dialect,
	/// The chosen languages, each with its SQL's dialect.
	hosts: Vec<(embed::Host, Dialect)>,
	/// The `*-params` keys to turn on.
	params: Vec<&'static str>,
}

/// The placeholder styles init offers: config key, what it looks like
/// and who writes it, and whether SQLite reads it without the option.
const PARAMS: &[(&str, &str, bool)] = &[
	("at-params", "@name (sqlc, Dapper, ADO.NET)", true),
	("question-params", "? and ?1 (JDBC, JPA, sqlx's Rebind)", true),
	(
		"colon-params",
		":name (SQLAlchemy, Spring, JPA, sqlx's named queries)",
		true,
	),
	("pyformat-params", "%s and %(name)s (psycopg, Python's DB-API)", false),
];

/// Every default: the languages that hold SQL, all in `dialect`.
fn defaults(dialect: Dialect, found: &[Found]) -> Answers {
	let hosts = embed::Host::ALL
		.iter()
		.zip(found)
		.filter(|(_, found)| found.strings > 0)
		.map(|(&host, _)| (host, dialect))
		.collect();
	Answers { dialect, hosts, params: Vec::new() }
}

/// Ask on the terminal. `None` when the user backs out (Esc or q).
fn ask(
	dialect: Dialect,
	sql_files: usize,
	found: &[Found],
) -> dialoguer::Result<Option<Answers>> {
	let theme = ColorfulTheme::default();
	let term = Term::stderr();

	let items: Vec<String> = embed::Host::ALL
		.iter()
		.zip(found)
		.map(|(&host, found)| {
			let name = display_name(host);
			if found.strings == 0 {
				name.to_string()
			} else {
				format!(
					"{name}  ({} in {})",
					plural(found.strings, "SQL string", "SQL strings"),
					plural(found.files, "file", "files"),
				)
			}
		})
		.collect();
	let checked: Vec<bool> =
		found.iter().map(|found| found.strings > 0).collect();
	let Some(chosen) = MultiSelect::with_theme(&theme)
		.with_prompt(
			"Format SQL embedded in which languages? (space to toggle, enter to confirm)",
		)
		.items(&items)
		.defaults(&checked)
		.interact_on_opt(&term)?
	else {
		return Ok(None);
	};

	let dialects = [Dialect::Postgres, Dialect::Sqlite];
	let pick = |prompt: String, default: Dialect| {
		Select::with_theme(&theme)
			.with_prompt(prompt)
			.items(dialects.map(dialect_name))
			.default(dialects.iter().position(|d| *d == default).unwrap_or(0))
			.interact_on_opt(&term)
			.map(|choice| choice.map(|index| dialects[index]))
	};
	let prompt = match sql_files {
		0 => "Dialect for .sql files".to_string(),
		n => {
			format!("Dialect for .sql files ({} found)", plural(n, "file", "files"))
		}
	};
	let Some(dialect) = pick(prompt, dialect)? else {
		return Ok(None);
	};
	let mut hosts = Vec::new();
	for index in chosen {
		let host = embed::Host::ALL[index];
		let prompt = format!("Dialect for SQL in {}", display_name(host));
		let Some(host_dialect) = pick(prompt, dialect)? else {
			return Ok(None);
		};
		hosts.push((host, host_dialect));
	}

	// SQLite reads `@name`, `?`, and `:name` as parameters already: only
	// offer those when something is Postgres.
	let postgres = dialect == Dialect::Postgres
		|| hosts.iter().any(|&(_, dialect)| dialect == Dialect::Postgres);
	let offered: Vec<&(&str, &str, bool)> = PARAMS
		.iter()
		.filter(|(_, _, sqlite_native)| postgres || !sqlite_native)
		.collect();
	let Some(chosen) = MultiSelect::with_theme(&theme)
		.with_prompt(
			"Which placeholders do your queries use? (space to toggle, enter to confirm)",
		)
		.items(offered.iter().map(|(_, label, _)| label))
		.interact_on_opt(&term)?
	else {
		return Ok(None);
	};
	let params = chosen.into_iter().map(|index| offered[index].0).collect();
	Ok(Some(Answers { dialect, hosts, params }))
}

/// The config file text for the answers given, as YAML.
fn render_yaml(answers: &Answers) -> String {
	let mut out = String::from(
		"# squill configuration: https://mckayla.dev/squill/docs/configuration/\n\n",
	);
	out.push_str(&format!("dialect: {}\n", dialect_name(answers.dialect)));
	for key in &answers.params {
		out.push_str(&format!("{key}: true\n"));
	}
	if !answers.hosts.is_empty() {
		out.push_str("\nembedded:\n");
	}
	for &(host, dialect) in &answers.hosts {
		let include: Vec<String> =
			host.extensions().iter().map(|ext| format!("\"**/*.{ext}\"")).collect();
		out.push_str(&format!(
			"  - include: [{}]\n    grammar: {}\n",
			include.join(", "),
			host.name()
		));
		if dialect != answers.dialect {
			out.push_str(&format!("    dialect: {}\n", dialect_name(dialect)));
		}
	}
	out
}

/// The config file text for the answers given. A rule names its dialect
/// only when it differs from the top level.
fn render(answers: &Answers) -> String {
	let mut out = String::from(
		"# squill configuration: https://mckayla.dev/squill/docs/configuration/\n\n",
	);
	out.push_str(&format!("dialect = \"{}\"\n", dialect_name(answers.dialect)));
	for key in &answers.params {
		out.push_str(&format!("{key} = true\n"));
	}
	for &(host, dialect) in &answers.hosts {
		let include: Vec<String> =
			host.extensions().iter().map(|ext| format!("\"**/*.{ext}\"")).collect();
		out.push_str(&format!(
			"\n[[embedded]]\ninclude = [{}]\ngrammar = \"{}\"\n",
			include.join(", "),
			host.name()
		));
		if dialect != answers.dialect {
			out.push_str(&format!("dialect = \"{}\"\n", dialect_name(dialect)));
		}
	}
	out
}

pub fn run(args: InitArgs) -> ExitCode {
	let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
	let existing =
		[cwd.clone(), cwd.join(".config")].into_iter().flat_map(|dir| {
			crate::config::CONFIG_NAMES.iter().map(move |name| dir.join(name))
		});
	for existing in existing {
		if existing.exists() {
			eprintln!(
				"squill: {} already exists; edit it instead (see `squill fmt --help`)",
				existing.display()
			);
			return ExitCode::from(2);
		}
	}

	let (sql_files, found) = survey(&cwd);
	let interactive = !args.yes
		&& std::io::stdin().is_terminal()
		&& std::io::stderr().is_terminal();
	let answers = if interactive {
		match ask(args.dialect, sql_files, &found) {
			Ok(Some(answers)) => answers,
			Ok(None) => {
				eprintln!("squill: cancelled; nothing written");
				return ExitCode::from(1);
			}
			Err(err) => {
				eprintln!("squill: {err}");
				return ExitCode::from(2);
			}
		}
	} else {
		let answers = defaults(args.dialect, &found);
		// Say what was decided, since nobody was asked.
		for &(host, _) in &answers.hosts {
			let found =
				&found[embed::Host::ALL.iter().position(|h| *h == host).unwrap_or(0)];
			eprintln!(
				"Formatting SQL in {}: found {} in {}.",
				display_name(host),
				plural(found.strings, "SQL string", "SQL strings"),
				plural(found.files, "file", "files"),
			);
		}
		answers
	};

	let (name, text) = if args.yaml {
		("squill.yaml", render_yaml(&answers))
	} else {
		("squill.toml", render(&answers))
	};
	let path = cwd.join(name);
	if let Err(err) = std::fs::write(&path, text) {
		eprintln!("squill: {}: {err}", path.display());
		return ExitCode::from(2);
	}
	eprintln!(
		"Wrote {name}. Run `squill fmt --check .` to see what would change."
	);
	ExitCode::SUCCESS
}

#[cfg(all(test, feature = "go", feature = "python"))]
mod tests {
	use super::*;

	#[test]
	fn rules_name_a_dialect_only_when_it_differs() {
		let answers = Answers {
			dialect: Dialect::Postgres,
			hosts: vec![
				(embed::Host::Go, Dialect::Postgres),
				(embed::Host::Python, Dialect::Sqlite),
			],
			params: vec!["colon-params", "pyformat-params"],
		};
		assert_eq!(
			render(&answers),
			"# squill configuration: https://mckayla.dev/squill/docs/configuration/\n\n\
			 dialect = \"postgres\"\n\
			 colon-params = true\n\
			 pyformat-params = true\n\n\
			 [[embedded]]\ninclude = [\"**/*.go\"]\ngrammar = \"go\"\n\n\
			 [[embedded]]\ninclude = [\"**/*.py\"]\ngrammar = \"python\"\ndialect = \"sqlite\"\n"
		);
	}
}
