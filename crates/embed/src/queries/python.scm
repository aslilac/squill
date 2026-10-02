; Default extraction query for Python: the first string argument of
; `.execute`-family method calls (sqlite3 / psycopg / asyncpg style)
; and of SQLAlchemy's `text(...)`.

((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list . (string) @sql))
 (#any-of? @_method
   "execute" "executemany" "executescript"
   "fetch" "fetchrow" "fetchval"))

((call
   function: (identifier) @_fn
   arguments: (argument_list . (string) @sql))
 (#eq? @_fn "text"))
