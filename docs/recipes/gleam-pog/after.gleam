import gleam/dynamic/decode
import pog

pub fn overdue(db: pog.Connection, cutoff: pog.Timestamp) {
  let decoder = {
    use id <- decode.field(0, decode.int)
    use email <- decode.field(1, decode.string)
    decode.success(#(id, email))
  }
  pog.query(
    "
    select i.id, c.email
    from invoices i join customers c on c.id = i.customer_id
    where i.due_at < $1 and i.paid_at is null
    order by i.due_at
    ",
  )
  |> pog.parameter(pog.timestamp(cutoff))
  |> pog.returning(decoder)
  |> pog.execute(db)
}
