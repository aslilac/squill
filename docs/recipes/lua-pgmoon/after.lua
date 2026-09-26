local pgmoon = require("pgmoon")

local pg = pgmoon.new({ host = "127.0.0.1", database = "blog", user = "blog" })
assert(pg:connect())

local function drafts_for(author_id)
  return pg:query([[
  select p.id, p.title, p.updated_at
  from posts p
  where p.author_id = $1 and p.published_at is null
  order by p.updated_at desc
  ]], author_id)
end

local function publish(id)
  return pg:query([==[
  update posts
  set published_at = now()
  where id = $1 and published_at is null
  returning id, published_at
  ]==], id)
end

local function titles(author)
  return pg:query("SELECT title FROM posts WHERE author = " .. pg:escape_literal(author))
end
