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

pub fn top_posters(db: pog.Connection, since: pog.Date) {
  let decoder = {
    use name <- decode.field(0, decode.string)
    use posts <- decode.field(1, decode.int)
    decode.success(#(name, posts))
  }
  "
  select u.name, count(*) as posts
  from posts p join users u on u.id = p.author_id
  where p.created_at >= $1
  group by u.name
  having count(*) > 10
  order by posts desc
  "
  |> pog.query
  |> pog.parameter(pog.calendar_date(since))
  |> pog.returning(decoder)
  |> pog.execute(db)
}
