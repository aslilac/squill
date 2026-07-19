//! SQL dialects.

/// The SQL dialect being lexed and parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Dialect {
	#[default]
	Postgres,
	Sqlite,
}
