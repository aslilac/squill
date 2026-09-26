import gleam/dynamic/decode
import sqlight

pub fn unread(db: sqlight.Connection, user_id: Int) {
  let decoder = {
    use id <- decode.field(0, decode.int)
    use subject <- decode.field(1, decode.string)
    decode.success(Message(id:, subject:))
  }
  sqlight.query(
    "
    select id, subject
    from messages
    where recipient_id = ? and read_at is null
    order by sent_at desc
    limit 25
    ",
    on: db,
    with: [sqlight.int(user_id)],
    expecting: decoder,
  )
}

pub fn migrate(db: sqlight.Connection) {
  sqlight.exec(
    "
    create table if not exists messages (
      id integer primary key,
      recipient_id integer not null,
      subject text not null,
      sent_at integer not null,
      read_at integer
    )
    ",
    db,
  )
}
