fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY,
                title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
                updated_at INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS notes_updated ON notes (updated_at DESC);
        "#,
    )
}

fn recent(conn: &Connection, since: i64) -> rusqlite::Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        r"SELECT id, title, updated_at FROM notes WHERE updated_at > ?1
            ORDER BY updated_at DESC LIMIT 50",
    )?;
    let rows = stmt.query_map([since], Note::from_row)?;
    rows.collect()
}
