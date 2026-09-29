// The playground's languages: every host squill has a built-in grammar
// for. `host` is the wasm request host (and names its module),
// `monacoLang` drives the input editor, and `shikiLang` highlights the
// formatted output.

export const languages: PlaygroundLanguage[] = [
	{
		host: "sql",
		label: "SQL",
	},

	// Host languages with built-in grammars.
	{
		host: "c#",
		label: "C#",
		monacoLang: "csharp",
	},
	{
		host: "c++",
		label: "C++",
		monacoLang: "cpp",
	},
	{
		host: "gleam",
		label: "Gleam",
	},
	{
		host: "go",
		label: "Go",
	},
	{
		host: "java",
		label: "Java",
	},
	{
		host: "kotlin",
		label: "Kotlin",
	},
	{
		host: "python",
		label: "Python",
	},
	{
		host: "rust",
		label: "Rust",
	},
	{
		host: "swift",
		label: "Swift",
	},
	{
		host: "typescript",
		label: "TypeScript",
	},
	{
		host: "tsx",
		label: "TSX",
		monacoLang: "typescript",
	},
	// CLEANUP: Could I get away with just removing this? TSX is strictly superior
	// for this use-case afaik. Any valid JS syntax, including JSX, should be
	// valid TSX, which makes this feel moot. Also JS has no recipes right now.
	{
		host: "javascript",
		label: "JavaScript",
	},
];

// TODO: I don't like these field names. I think `id`, `displayName`,
// `monacoLanguageId`, and `shikiLanguageId` would all be more appropriate, but
// I'm gonna wait until I finish adding TypeScript types to rename them.
type PlaygroundLanguage = {
	readonly host: string;
	readonly label: string;

	/** Monaco's id for the language, where it differs from `host`. */
	readonly monacoLang?: string;
	/** Shiki's id for the language, where it differs from `host`. */
	readonly shikiLang?: string;
};

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
