import SQLite

let db = try Connection("notes.sqlite3")

try db.execute("""
create table if not exists notes (
  id integer primary key,
  title text not null,
  body text not null default '',
  updated_at integer not null
)
""")

func recent(since: Int64) throws -> [(Int64, String)] {
    let stmt = try db.prepare("""
    select id, title
    from notes
    where updated_at > ?
    order by updated_at desc
    limit 20
    """)
    return try stmt.bind(since).map { row in (row[0] as! Int64, row[1] as! String) }
}

let total = try db.scalar("SELECT count(*) FROM notes") as! Int64
