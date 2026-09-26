import gleam/dynamic/decode
import pog

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

pub fn archive(db: pog.Connection) {
  pog.query(
    "
    update posts
    set archived = true
    where created_at < now() - interval '2 years'
    ",
  )
  |> pog.execute(db)
}
