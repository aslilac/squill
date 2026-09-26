require "sqlite3"

db = SQLite3::Database.new("bookmarks.db")
db.execute_batch(<<~SQL)
  CREATE TABLE IF NOT EXISTS bookmarks (id INTEGER PRIMARY KEY, url TEXT NOT NULL UNIQUE, title TEXT, saved_at INTEGER NOT NULL DEFAULT (unixepoch()));
  CREATE TABLE IF NOT EXISTS tags (bookmark_id INTEGER NOT NULL REFERENCES bookmarks (id) ON DELETE CASCADE, name TEXT NOT NULL, PRIMARY KEY (bookmark_id, name));
SQL

def tagged(db, tag)
  db.execute(<<~SQL, [tag])
    SELECT b.url, b.title FROM bookmarks b JOIN tags t ON t.bookmark_id = b.id WHERE t.name = ? ORDER BY b.saved_at DESC
  SQL
end

def count(db)
  db.get_first_value("SELECT count(*) FROM bookmarks")
end
