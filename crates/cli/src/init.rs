//! `squill init`: write a starter squill.toml (or, with `--yaml`,
//! squill.yaml) for the project in the
//! working directory.
//!
//! The wizard lists every built-in grammar as a checkbox, pre-checking
//! the languages that already hold SQL — found by walking the tree the
//! way `fmt` does (honoring .gitignore, skipping hidden files) and
//! running each grammar's default query over its files. Which files are
//! in which language is a guess from their extensions, and only here:
//! each rule written lists the extensions found, for the user to see and
//! correct, and nothing else in squill reads a language from a name. Then it asks for
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
	/// Files holding SQL, and the SQL strings in them.
	files: usize,
	strings: usize,
	/// The language's extensions that any file has, in [`extensions`]
	/// order.
	extensions: Vec<&'static str>,
}

/// A language's name the way people write it.
pub fn display_name(host: embed::Host) -> &'static str {
	match host.name() {
		"rust" => "Rust",
		"go" => "Go",
		"python" => "Python",
		"javascript" => "JavaScript",
		"typescript" => "TypeScript",
		"tsx" => "TSX",
		"gleam" => "Gleam",
		"c++" => "C++",
		"c#" => "C#",
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

/// File extensions conventionally written in a language. C and C++
/// share `.h`; C has nothing squill can rewrite (no string literal spans
/// lines), so a header counts as C++.
fn extensions(host: embed::Host) -> &'static [&'static str] {
	match host {
		#[cfg(feature = "rust")]
		embed::Host::Rust => &["rs"],
		#[cfg(feature = "go")]
		embed::Host::Go => &["go"],
		#[cfg(feature = "python")]
		embed::Host::Python => &["py"],
		#[cfg(feature = "javascript")]
		embed::Host::JavaScript => &["js", "mjs", "cjs", "jsx"],
		#[cfg(feature = "typescript")]
		embed::Host::TypeScript => &["ts", "mts", "cts"],
		#[cfg(feature = "typescript")]
		embed::Host::Tsx => &["tsx"],
		#[cfg(feature = "gleam")]
		embed::Host::Gleam => &["gleam"],
		#[cfg(feature = "cxx")]
		embed::Host::Cxx => &["cc", "cpp", "cxx", "h", "hh", "hpp", "hxx"],
		#[cfg(feature = "csharp")]
		embed::Host::CSharp => &["cs"],
		#[cfg(feature = "java")]
		embed::Host::Java => &["java"],
		#[cfg(feature = "kotlin")]
		embed::Host::Kotlin => &["kt", "kts"],
		#[cfg(feature = "swift")]
		embed::Host::Swift => &["swift"],
	}
}

/// The language a file is probably in, by its extension, and which of
/// the language's extensions that is.
pub fn host_for(path: &Path) -> Option<(embed::Host, &'static str)> {
	let extension = path.extension()?.to_str()?;
	embed::Host::ALL.iter().find_map(|&host| {
		let known = extensions(host).iter().find(|&&known| known == extension)?;
		Some((host, *known))
	})
}

/// Walk `root` and count the SQL each built-in grammar's default query
/// finds (in [`embed::Host::ALL`] order), plus the plain .sql files.
fn survey(root: &Path) -> (usize, Vec<Found>) {
	let mut sql_files = 0;
	let mut candidates: Vec<(PathBuf, embed::Host)> = Vec::new();
	let mut seen: Vec<(embed::Host, &'static str)> = Vec::new();
	for entry in ignore::WalkBuilder::new(root).build().flatten() {
		if !entry.file_type().is_some_and(|kind| kind.is_file()) {
			continue;
		}
		let path = entry.into_path();
		if path.extension().is_some_and(|ext| ext == "sql") {
			sql_files += 1;
		} else if let Some((host, extension)) = host_for(&path) {
			if !seen.contains(&(host, extension)) {
				seen.push((host, extension));
			}
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
			let extensions = extensions(host)
				.iter()
				.copied()
				.filter(|&ext| seen.contains(&(host, ext)))
				.collect();
			Found { files: mine.len(), strings: mine.iter().sum(), extensions }
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
	hosts: Vec<Chosen>,
	/// The `*-params` keys to turn on.
	params: Vec<&'static str>,
}

/// A language to write an `[[embedded]]` rule for.
struct Chosen {
	host: embed::Host,
	dialect: Dialect,
	/// What its `include` covers: the extensions the project has, or all
	/// the language's when it has none yet.
	extensions: Vec<&'static str>,
}

impl Chosen {
	fn new(host: embed::Host, dialect: Dialect, found: &Found) -> Chosen {
		let extensions = if found.extensions.is_empty() {
			extensions(host).to_vec()
		} else {
			found.extensions.clone()
		};
		Chosen { host, dialect, extensions }
	}

	/// The `include` globs, as a TOML or YAML flow list.
	fn include(&self) -> String {
		let globs: Vec<String> =
			self.extensions.iter().map(|ext| format!("\"**/*.{ext}\"")).collect();
		format!("[{}]", globs.join(", "))
	}
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
		.map(|(&host, found)| Chosen::new(host, dialect, found))
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
		hosts.push(Chosen::new(host, host_dialect, &found[index]));
	}

	// SQLite reads `@name`, `?`, and `:name` as parameters already: only
	// offer those when something is Postgres.
	let postgres = dialect == Dialect::Postgres
		|| hosts.iter().any(|chosen| chosen.dialect == Dialect::Postgres);
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
	for chosen in &answers.hosts {
		out.push_str(&format!(
			"  - include: {}\n    grammar: {}\n",
			chosen.include(),
			chosen.host.name()
		));
		if chosen.dialect != answers.dialect {
			out.push_str(&format!("    dialect: {}\n", dialect_name(chosen.dialect)));
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
	for chosen in &answers.hosts {
		out.push_str(&format!(
			"\n[[embedded]]\ninclude = {}\ngrammar = \"{}\"\n",
			chosen.include(),
			chosen.host.name()
		));
		if chosen.dialect != answers.dialect {
			out
				.push_str(&format!("dialect = \"{}\"\n", dialect_name(chosen.dialect)));
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
		for chosen in &answers.hosts {
			let index = embed::Host::ALL.iter().position(|h| *h == chosen.host);
			let found = &found[index.unwrap_or(0)];
			eprintln!(
				"Formatting SQL in {}: found {} in {}.",
				display_name(chosen.host),
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
				Chosen {
					host: embed::Host::Go,
					dialect: Dialect::Postgres,
					extensions: vec!["go"],
				},
				Chosen {
					host: embed::Host::Python,
					dialect: Dialect::Sqlite,
					extensions: vec!["py"],
				},
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
