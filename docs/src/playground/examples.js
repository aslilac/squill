// Playground examples: one messy-but-real snippet per mode. `host` is
// the wasm request host, `monacoLang` drives the input editor, and
// `shikiLang` highlights the formatted output.

export const examples = [
	{
		label: "plain SQL",
		host: "sql",
		monacoLang: "sql",
		shikiLang: "sql",
		source: `SELECT w.id, w.name, count(*) AS build_count FROM workspaces w JOIN workspace_builds wb ON wb.workspace_id = w.id WHERE w.deleted = false AND w.last_used_at > NOW() - interval '30 days' GROUP BY w.id, w.name ORDER BY build_count DESC LIMIT 20;

CREATE FUNCTION notify_workspace_change() RETURNS trigger SECURITY DEFINER AS $$ BEGIN PERFORM pg_notify('workspaces', NEW.id::text); RETURN NEW; END; $$ LANGUAGE plpgsql;

CREATE TABLE workspace_agents (id uuid NOT NULL, workspace_id uuid NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE, name text NOT NULL, created_at timestamptz NOT NULL DEFAULT NOW(), PRIMARY KEY (id));
`,
	},
	{
		label: "Rust · sqlx",
		host: "rust",
		monacoLang: "rust",
		shikiLang: "rust",
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
		label: "Go · database/sql",
		host: "go",
		monacoLang: "go",
		shikiLang: "go",
		source: `func ListSessions(ctx context.Context, db *sql.DB, userID string) (*sql.Rows, error) {
	return db.QueryContext(ctx, \`SELECT id, created_at, expires_at FROM sessions
	WHERE user_id = $1 AND expires_at > now() ORDER BY created_at DESC\`, userID)
}
`,
	},
	{
		label: "Python · psycopg",
		host: "python",
		monacoLang: "python",
		shikiLang: "python",
		source: `def load_users(cur, org, status):
    cur.execute("""SELECT id,name FROM users WHERE org=%s AND status=%(status)s ORDER BY name""", {"org": org, "status": status})
    return cur.fetchall()
`,
	},
	{
		label: "TypeScript · postgres.js",
		host: "typescript",
		monacoLang: "typescript",
		shikiLang: "typescript",
		source: `export async function recentBuilds(db: Sql, workspaceId: string) {
	return db.query(\`SELECT id, build_number, transition FROM workspace_builds
	WHERE workspace_id = $1 ORDER BY build_number DESC LIMIT 50\`, [workspaceId]);
}
`,
	},
	{
		label: "Gleam · sqlight",
		host: "gleam",
		monacoLang: "gleam",
		shikiLang: "gleam",
		source: `pub fn list_users(db: sqlight.Connection, org: Int) {
  sqlight.query("select id,name from users where org = ? order by name", on: db, with: [sqlight.int(org)])
}
`,
	},
];
