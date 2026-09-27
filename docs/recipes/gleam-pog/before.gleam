import gleam/dynamic/decode
import pog

pub fn overdue(db: pog.Connection, cutoff: pog.Timestamp) {
  let decoder = {
    use id <- decode.field(0, decode.int)
    use email <- decode.field(1, decode.string)
    decode.success(#(id, email))
  }
  pog.query(
    "SELECT i.id, c.email FROM invoices i JOIN customers c ON c.id = i.customer_id
        WHERE i.due_at < $1 AND i.paid_at IS NULL ORDER BY i.due_at",
  )
  |> pog.parameter(pog.timestamp(cutoff))
  |> pog.returning(decoder)
  |> pog.execute(db)
}

pub fn top_posters(db: pog.Connection, since: pog.Date) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    use posts <- decode.field(1, decode.int)
    decode.success(#(name, posts))
  }
  "SELECT u.name, count(*) AS posts FROM posts p JOIN users u ON u.id = p.author_id
      WHERE p.created_at >= $1 GROUP BY u.name HAVING count(*) > 10
      ORDER BY posts DESC"
  |> pog.query
  |> pog.parameter(pog.calendar_date(since))
  |> pog.returning(decoder)
  |> pog.execute(db)
}
