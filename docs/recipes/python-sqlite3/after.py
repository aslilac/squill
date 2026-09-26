import sqlite3


def migrate(db: sqlite3.Connection) -> None:
    db.executescript("""
    create table if not exists bookmarks (
      id integer primary key,
      url text not null unique,
      title text,
      added_at text not null default (datetime('now'))
    );
    create table if not exists tags (
      bookmark_id integer not null references bookmarks (id) on delete cascade,
      tag text not null,
      primary key (bookmark_id, tag)
    );
    """)


def tagged(db: sqlite3.Connection, tag: str) -> list[sqlite3.Row]:
    return db.execute(
        """
        select b.id, b.url, b.title
        from bookmarks b join tags t on t.bookmark_id = b.id
        where t.tag = ?
        order by b.added_at desc
        """,
        (tag,),
    ).fetchall()


def rename(db: sqlite3.Connection, bookmark_id: int, title: str) -> None:
    db.execute(
        """
        update bookmarks
        set title = :title
        where id = :id
        """,
        {"id": bookmark_id, "title": title},
    )
