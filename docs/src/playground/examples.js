// The playground's languages: every host squill has a built-in grammar
// for. `host` is the wasm request host (and names its module),
// `monacoLang` drives the input editor, and `shikiLang` highlights the
// formatted output.
export const languages = [
	{ host: "sql", label: "SQL", monacoLang: "sql", shikiLang: "sql" },
	{ host: "rust", label: "Rust", monacoLang: "rust", shikiLang: "rust" },
	{ host: "go", label: "Go", monacoLang: "go", shikiLang: "go" },
	{ host: "python", label: "Python", monacoLang: "python", shikiLang: "python" },
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
	{ host: "kotlin", label: "Kotlin", monacoLang: "kotlin", shikiLang: "kotlin" },
	{ host: "swift", label: "Swift", monacoLang: "swift", shikiLang: "swift" },
];

// Examples: one messy-but-real snippet for some of them. `host` picks the
// language, and `params` turns on the `*-params` options the example's
// driver needs.

export const examples = [
	{
		label: "SQL",
		host: "sql",
		source: `SELECT w.id, w.name, count(*) AS build_count FROM workspaces w JOIN workspace_builds wb ON wb.workspace_id = w.id WHERE w.deleted = false AND w.last_used_at > NOW() - interval '30 days' GROUP BY w.id, w.name ORDER BY build_count DESC LIMIT 20;

CREATE FUNCTION notify_workspace_change() RETURNS trigger SECURITY DEFINER AS $$ BEGIN PERFORM pg_notify('workspaces', NEW.id::text); RETURN NEW; END; $$ LANGUAGE plpgsql;

CREATE TABLE workspace_agents (id uuid NOT NULL, workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE, name text NOT NULL, created_at timestamptz NOT NULL DEFAULT NOW(), PRIMARY KEY (id));
`,
	},
	{
		label: "Rust + sqlx",
		host: "rust",
		source: `pub async fn active_users(pool: &PgPool, org: Uuid) -> sqlx::Result<Vec<User>> {
	sqlx::query_as!(
		User,
		r#"SELECT id, name, email FROM users
		WHERE org_id = $1 AND deleted = false ORDER BY name"#,
		org,
	)
	.fetch_all(pool)
	.await
}

pub async fn delete_rule(pool: &PgPool, team: Uuid, id: Uuid) -> sqlx::Result<()> {
	sqlx::query!(r#"delete from team_auto_add_rules where team_id = $1 and id = $2"#, team, id)
		.execute(pool)
		.await?;
	Ok(())
}
`,
	},
	{
		label: "Go + database/sql",
		host: "go",
		source: `func ListSessions(ctx context.Context, db *sql.DB, userID string) (*sql.Rows, error) {
	return db.QueryContext(ctx, \`SELECT id, created_at, expires_at FROM sessions
	WHERE user_id = $1 AND expires_at > now() ORDER BY created_at DESC\`, userID)
}
`,
	},
	{
		label: "Python + psycopg",
		host: "python",
		// psycopg's `%s` placeholders: what the recipe configures.
		params: { pyformat_params: true },
		source: `def load_users(cur, org, status):
    cur.execute("""SELECT id,name FROM users WHERE org=%s AND status=%(status)s ORDER BY name""", {"org": org, "status": status})
    return cur.fetchall()
`,
	},
	{
		label: "TypeScript + postgres.js",
		host: "typescript",
		source: `export async function recentBuilds(db: Sql, workspaceId: string) {
	return db.query(\`SELECT id, build_number, transition FROM workspace_builds
	WHERE workspace_id = $1 ORDER BY build_number DESC LIMIT 50\`, [workspaceId]);
}
`,
	},
	{
		label: "Gleam + sqlight",
		host: "gleam",
		source: `pub fn list_users(db: sqlight.Connection, org: Int) {
  sqlight.query("select id,name from users where org = ? order by name", on: db, with: [sqlight.int(org)])
}
`,
	},
];
