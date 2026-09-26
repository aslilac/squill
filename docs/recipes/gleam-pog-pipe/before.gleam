import gleam/dynamic/decode
import pog

pub fn top_posters(db: pog.Connection, since: pog.Date) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    use posts <- decode.field(1, decode.int)
    decode.success(#(name, posts))
  }
  "SELECT u.name, count(*) AS posts FROM posts p JOIN users u ON u.id = p.author_id WHERE p.created_at >= $1 GROUP BY u.name HAVING count(*) > 10 ORDER BY posts DESC"
  |> pog.query
  |> pog.parameter(pog.calendar_date(since))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

pub fn archive(db: pog.Connection) {
  pog.query(
    "UPDATE posts SET archived = true WHERE created_at < now() - interval '2 years'",
  )
  |> pog.execute(db)
}
