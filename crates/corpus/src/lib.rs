//! The vendored test corpora under `corpus/`: which files each one holds
//! and how to read them.
//!
//! Every corpus harness (the parser's lossless tests, the formatter's
//! safety oracle and snapshots, `corpus-report`) walks the same list, so
//! a new corpus, or a new dialect rule, is one entry here.

use std::path::Path;
use std::path::PathBuf;

use parser::Dialect;
use parser::lexer::LexOptions;

/// One vendored corpus: a directory under `corpus/`.
struct Corpus {
	name: &'static str,
	/// The dialect of a file, by its path relative to the corpus
	/// directory; `None` for files that aren't part of the corpus.
	dialect: fn(&str) -> Option<Dialect>,
	lex_options: LexOptions,
}

const PLAIN: LexOptions = LexOptions {
	at_params: false,
	pyformat_params: false,
	question_params: false,
	colon_params: false,
};

const CORPORA: &[Corpus] = &[
	// sqlc SQL for Postgres: `@name` params.
	Corpus {
		name: "coder",
		dialect: |path| path.ends_with(".sql").then_some(Dialect::Postgres),
		lex_options: LexOptions { at_params: true, ..PLAIN },
	},
	Corpus {
		name: "anki",
		dialect: |path| path.ends_with(".sql").then_some(Dialect::Sqlite),
		lex_options: PLAIN,
	},
	// One schema for two databases: `.sql.sqlite` and `.sql.postgres`
	// files are one dialect each, and plain `.sql` files run on both;
	// they count as SQLite here, the dialect this corpus is for.
	Corpus {
		name: "synapse",
		dialect: |path| {
			if path.ends_with(".sql.postgres") {
				Some(Dialect::Postgres)
			} else if path.ends_with(".sql.sqlite") || path.ends_with(".sql") {
				Some(Dialect::Sqlite)
			} else {
				None
			}
		},
		lex_options: PLAIN,
	},
	Corpus {
		name: "vaultwarden",
		dialect: |path| path.ends_with(".sql").then_some(Dialect::Sqlite),
		lex_options: PLAIN,
	},
];

/// A corpus file and how to read it.
#[derive(Debug, Clone)]
pub struct CorpusFile {
	pub path: PathBuf,
	/// The corpus it belongs to (`coder`, `anki`, ...).
	pub corpus: &'static str,
	/// Its path within the corpus directory, `/`-separated.
	pub relative: String,
	pub dialect: Dialect,
	pub lex_options: LexOptions,
}

impl CorpusFile {
	/// A name unique across every corpus, usable as a snapshot name:
	/// `coder__queries__users` for `coder/queries/users.sql`.
	pub fn name(&self) -> String {
		let stem = [".sql.postgres", ".sql.sqlite", ".sql"]
			.iter()
			.find_map(|ext| self.relative.strip_suffix(ext))
			.unwrap_or(&self.relative);
		// Keep the dialect suffix: `foo.sql.sqlite` and `foo.sql.postgres`
		// often sit side by side.
		let suffix = if self.relative.ends_with(".sql.postgres") {
			".postgres"
		} else if self.relative.ends_with(".sql.sqlite") {
			".sqlite"
		} else {
			""
		};
		format!("{}__{}{suffix}", self.corpus, stem.replace('/', "__"))
	}
}

/// The `corpus/` directory of this checkout.
pub fn root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus")
}

/// Every file of every corpus under `root`, sorted by path.
pub fn files(root: &Path) -> std::io::Result<Vec<CorpusFile>> {
	let mut out = Vec::new();
	for corpus in CORPORA {
		let dir = root.join(corpus.name);
		if !dir.is_dir() {
			continue;
		}
		let mut paths = Vec::new();
		walk(&dir, &mut paths)?;
		for path in paths {
			let relative = path
				.strip_prefix(&dir)
				.expect("walked below the corpus directory")
				.components()
				.map(|part| part.as_os_str().to_string_lossy())
				.collect::<Vec<_>>()
				.join("/");
			if let Some(dialect) = (corpus.dialect)(&relative) {
				out.push(CorpusFile {
					path,
					corpus: corpus.name,
					relative,
					dialect,
					lex_options: corpus.lex_options,
				});
			}
		}
	}
	out.sort_by(|a, b| a.path.cmp(&b.path));
	Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
	for entry in std::fs::read_dir(dir)? {
		let path = entry?.path();
		if path.is_dir() {
			walk(&path, out)?;
		} else {
			out.push(path);
		}
	}
	Ok(())
}
