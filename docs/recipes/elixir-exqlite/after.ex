defmodule Tally.Store do
  alias Exqlite.Sqlite3

  def open(path) do
    {:ok, conn} = Sqlite3.open(path)

    :ok =
      Sqlite3.execute(conn, ~S"""
      create table if not exists events (
        id integer primary key,
        name text not null,
        at integer not null,
        payload text check (json_valid(payload))
      );
      create index if not exists events_by_name on events (name, at desc)
      """)

    {:ok, conn}
  end

  def recent(conn, name) do
    {:ok, stmt} =
      Sqlite3.prepare(conn, """
      select id, at, payload ->> '$.user' as user
      from events
      where name = ?1
      order by at desc
      limit 100
      """)

    :ok = Sqlite3.bind(stmt, [name])
    stmt
  end
end
