import sqlite3


def migrate(db: sqlite3.Connection) -> None:
    db.executescript("""
        CREATE TABLE IF NOT EXISTS bookmarks (id INTEGER PRIMARY KEY,
            url TEXT NOT NULL UNIQUE,
            title TEXT, added_at TEXT NOT NULL DEFAULT (datetime('now')));
        CREATE TABLE IF NOT EXISTS tags (bookmark_id INTEGER NOT NULL REFERENCES bookmarks (id) ON DELETE CASCADE,
            tag TEXT NOT NULL, PRIMARY KEY (bookmark_id, tag));
    """)


def tagged(db: sqlite3.Connection, tag: str) -> list[sqlite3.Row]:
    return db.execute(
        """SELECT b.id, b.url, b.title FROM bookmarks b
            JOIN tags t ON t.bookmark_id = b.id WHERE t.tag = ?
            ORDER BY b.added_at DESC""",
        (tag,),
    ).fetchall()


def rename(db: sqlite3.Connection, bookmark_id: int, title: str) -> None:
    db.execute(
        """UPDATE bookmarks SET title = :title WHERE id = :id""",
        {"id": bookmark_id, "title": title},
    )
