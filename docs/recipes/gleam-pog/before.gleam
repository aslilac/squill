import gleam/dynamic/decode
import pog

pub fn overdue(db: pog.Connection, cutoff: pog.Timestamp) {
  let decoder = {
    use id <- decode.field(0, decode.int)
    use email <- decode.field(1, decode.string)
    decode.success(#(id, email))
  }
  pog.query(
    "SELECT i.id, c.email FROM invoices i JOIN customers c ON c.id = i.customer_id WHERE i.due_at < $1 AND i.paid_at IS NULL ORDER BY i.due_at",
  )
  |> pog.parameter(pog.timestamp(cutoff))
  |> pog.returning(decoder)
  |> pog.execute(db)
}
