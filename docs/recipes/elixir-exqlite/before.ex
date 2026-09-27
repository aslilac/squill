defmodule Tally.Store do
  alias Exqlite.Sqlite3

  def open(path) do
    {:ok, conn} = Sqlite3.open(path)

    :ok =
      Sqlite3.execute(conn, ~S"""
      CREATE TABLE IF NOT EXISTS events (id INTEGER PRIMARY KEY,
              name TEXT NOT NULL, at INTEGER NOT NULL,
              payload TEXT CHECK (json_valid(payload)));
      CREATE INDEX IF NOT EXISTS events_by_name ON events (name, at DESC);
      """)

    {:ok, conn}
  end

  def recent(conn, name) do
    {:ok, stmt} =
      Sqlite3.prepare(conn, """
      SELECT id, at, payload ->> '$.user' AS user FROM events
          WHERE name = ?1 ORDER BY at DESC LIMIT 100
      """)

    :ok = Sqlite3.bind(stmt, [name])
    stmt
  end
end
