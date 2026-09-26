require "sqlite3"

db = SQLite3::Database.new("bookmarks.db")
db.execute_batch(<<~SQL)
create table if not exists bookmarks (
  id integer primary key,
  url text not null unique,
  title text,
  saved_at integer not null default (unixepoch())
);
create table if not exists tags (
  bookmark_id integer not null references bookmarks (id) on delete cascade,
  name text not null,
  primary key (bookmark_id, name)
)
SQL

def tagged(db, tag)
  db.execute(<<~SQL, [tag])
  select b.url, b.title
  from bookmarks b join tags t on t.bookmark_id = b.id
  where t.name = ?
  order by b.saved_at desc
  SQL
end

def count(db)
  db.get_first_value("SELECT count(*) FROM bookmarks")
end
