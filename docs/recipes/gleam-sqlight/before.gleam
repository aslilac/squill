import gleam/dynamic/decode
import sqlight

pub fn unread(db: sqlight.Connection, user_id: Int) {
  let decoder = {
    use id <- decode.field(0, decode.int)
    use subject <- decode.field(1, decode.string)
    decode.success(Message(id:, subject:))
  }
  sqlight.query(
    "SELECT id, subject FROM messages WHERE recipient_id = ?
        AND read_at IS NULL ORDER BY sent_at DESC LIMIT 25",
    on: db,
    with: [sqlight.int(user_id)],
    expecting: decoder,
  )
}

pub fn migrate(db: sqlight.Connection) {
  sqlight.exec(
    "CREATE TABLE IF NOT EXISTS messages (id INTEGER PRIMARY KEY,
            recipient_id INTEGER NOT NULL,
            subject TEXT NOT NULL, sent_at INTEGER NOT NULL,
            read_at INTEGER);",
    db,
  )
}
