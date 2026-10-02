local sqlite3 = require("lsqlite3")

local M = {}

function M.open(path)
  local db = sqlite3.open(path)
  db:exec[[
  create table if not exists scores (
    id integer primary key,
    player text not null,
    points integer not null,
    played_at integer not null default (unixepoch())
  );
  create index if not exists scores_by_points on scores (points desc)
  ]]
  return db
end

function M.leaders(db, n)
  local leaders = {}
  local stmt = db:prepare([[
  select player, max(points) as best
  from scores
  group by player
  order by best desc
  limit ?
  ]])
  stmt:bind_values(n)
  for row in stmt:nrows() do
    leaders[#leaders + 1] = row
  end
  stmt:finalize()
  return leaders
end

function M.clear(db)
  db:exec("DELETE FROM scores")
end

return M
