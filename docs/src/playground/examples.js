// The playground's languages: every host squill has a built-in grammar
// for. `host` is the wasm request host (and names its module),
// `monacoLang` drives the input editor, and `shikiLang` highlights the
// formatted output.
export const languages = [
	{ host: "sql", label: "SQL", monacoLang: "sql", shikiLang: "sql" },
	{ host: "rust", label: "Rust", monacoLang: "rust", shikiLang: "rust" },
	{ host: "go", label: "Go", monacoLang: "go", shikiLang: "go" },
	{
		host: "python",
		label: "Python",
		monacoLang: "python",
		shikiLang: "python",
	},
	{
		host: "javascript",
		label: "JavaScript",
		monacoLang: "javascript",
		shikiLang: "javascript",
	},
	{
		host: "typescript",
		label: "TypeScript",
		monacoLang: "typescript",
		shikiLang: "typescript",
	},
	{ host: "tsx", label: "TSX", monacoLang: "typescript", shikiLang: "tsx" },
	{ host: "gleam", label: "Gleam", monacoLang: "gleam", shikiLang: "gleam" },
	{ host: "cpp", label: "C++", monacoLang: "cpp", shikiLang: "cpp" },
	{ host: "csharp", label: "C#", monacoLang: "csharp", shikiLang: "csharp" },
	{ host: "java", label: "Java", monacoLang: "java", shikiLang: "java" },
	{
		host: "kotlin",
		label: "Kotlin",
		monacoLang: "kotlin",
		shikiLang: "kotlin",
	},
	{ host: "swift", label: "Swift", monacoLang: "swift", shikiLang: "swift" },
];

// Plain SQL's examples, one per dialect; every other language's come
// from the docs recipes (see the playground page).
export const sqlExamples = [
	{
		title: "Postgres",
		source: `SELECT w.id, w.name, count(*) AS build_count FROM workspaces w
JOIN workspace_builds wb ON wb.workspace_id = w.id
WHERE w.deleted = false AND w.last_used_at > NOW() - interval '30 days'
GROUP BY w.id, w.name ORDER BY build_count DESC LIMIT 20;

CREATE FUNCTION notify_workspace_change() RETURNS trigger SECURITY DEFINER AS $$
BEGIN PERFORM pg_notify('workspaces', NEW.id::text); RETURN NEW; END;
$$ LANGUAGE plpgsql;

CREATE TABLE workspace_agents (id uuid NOT NULL,
  workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
  name text NOT NULL, created_at timestamptz NOT NULL DEFAULT NOW(),
  PRIMARY KEY (id));
`,
	},
	{
		title: "SQLite",
		dialect: "sqlite",
		source: `CREATE TABLE IF NOT EXISTS cards (id INTEGER PRIMARY KEY AUTOINCREMENT,
  deck_id INTEGER NOT NULL REFERENCES decks (id) ON DELETE CASCADE,
  front TEXT NOT NULL, back TEXT NOT NULL,
  due INTEGER NOT NULL DEFAULT 0, tags TEXT NOT NULL DEFAULT '') STRICT;

INSERT OR REPLACE INTO config (key, val, mtime) VALUES (?1, ?2, unixepoch());

SELECT c.id, c.front, d.name AS deck FROM cards c JOIN decks d ON d.id = c.deck_id
WHERE c.due <= unixepoch() AND c.tags LIKE '%' || :tag || '%'
ORDER BY c.due LIMIT 50;
`,
	},
];
