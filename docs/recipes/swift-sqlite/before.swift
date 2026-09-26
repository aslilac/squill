import SQLite

let db = try Connection("notes.sqlite3")

try db.execute("""
    CREATE TABLE IF NOT EXISTS notes (id INTEGER PRIMARY KEY, title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', updated_at INTEGER NOT NULL)
    """)

func recent(since: Int64) throws -> [(Int64, String)] {
    let stmt = try db.prepare("""
        SELECT id, title FROM notes WHERE updated_at > ? ORDER BY updated_at DESC LIMIT 20
        """)
    return try stmt.bind(since).map { row in (row[0] as! Int64, row[1] as! String) }
}

let total = try db.scalar("SELECT count(*) FROM notes") as! Int64
