local pgmoon = require("pgmoon")

local pg = pgmoon.new({ host = "127.0.0.1", database = "blog", user = "blog" })
assert(pg:connect())

local function drafts_for(author_id)
  return pg:query([[
    SELECT p.id, p.title, p.updated_at FROM posts p
      WHERE p.author_id = $1 AND p.published_at IS NULL
      ORDER BY p.updated_at DESC
  ]], author_id)
end

local function publish(id)
  return pg:query([==[
    UPDATE posts SET published_at = now() WHERE id = $1
      AND published_at IS NULL RETURNING id, published_at
  ]==], id)
end

local function titles(author)
  return pg:query("SELECT title FROM posts WHERE author = " .. pg:escape_literal(author))
end
