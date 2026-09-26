fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        r#"
        create table if not exists notes (
          id integer primary key,
          title text not null,
          body text not null default '',
          updated_at integer not null
        );
        create index if not exists notes_updated
          on notes (updated_at desc);
        "#,
    )
}

fn recent(conn: &Connection, since: i64) -> rusqlite::Result<Vec<Note>> {
    let mut stmt = conn.prepare(
        r"
        select id, title, updated_at
        from notes
        where updated_at > ?1
        order by updated_at desc
        limit 50
        ",
    )?;
    let rows = stmt.query_map([since], Note::from_row)?;
    rows.collect()
}
