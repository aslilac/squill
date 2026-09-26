local sqlite3 = require("lsqlite3")

local M = {}

function M.open(path)
  local db = sqlite3.open(path)
  db:exec[[
    CREATE TABLE IF NOT EXISTS scores (id INTEGER PRIMARY KEY, player TEXT NOT NULL, points INTEGER NOT NULL, played_at INTEGER NOT NULL DEFAULT (unixepoch()));
    CREATE INDEX IF NOT EXISTS scores_by_points ON scores (points DESC);
  ]]
  return db
end

function M.leaders(db, n)
  local leaders = {}
  local stmt = db:prepare([[
    SELECT player, max(points) AS best FROM scores GROUP BY player ORDER BY best DESC LIMIT ?
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
