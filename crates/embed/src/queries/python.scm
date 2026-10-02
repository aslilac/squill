; SQL in Python: the first string argument of `.execute`-family method
; calls (sqlite3 / psycopg / asyncpg style) and of SQLAlchemy's
; `text(…)`. Only triple-quoted strings, which take line breaks, and
; only by the prefixes listed: f-strings and bytes never match. The
; anchored `.`s keep a string to one content node.

; Escapes are read and written back as they're spelled.
((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list
     . (string (string_start) @_start . (string_content) @sql . (string_end))))
 (#any-of? @_method
   "execute" "executemany" "executescript"
   "fetch" "fetchrow" "fetchval")
 (#any-of? @_start "\"\"\"" "'''" "u\"\"\"" "u'''" "U\"\"\"" "U'''")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\UXXXXXXXX")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation")
 (#set! squill.multiline))

; An `r` prefix means none.
((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list
     . (string (string_start) @_start . (string_content) @sql . (string_end))))
 (#any-of? @_method
   "execute" "executemany" "executescript"
   "fetch" "fetchrow" "fetchval")
 (#any-of? @_start "r\"\"\"" "r'''" "R\"\"\"" "R'''")
 (#set! squill.raw)
 (#set! squill.multiline))

; SQLAlchemy's `text(…)`, likewise.
((call
   function: (identifier) @_fn
   arguments: (argument_list
     . (string (string_start) @_start . (string_content) @sql . (string_end))))
 (#eq? @_fn "text")
 (#any-of? @_start "\"\"\"" "'''" "u\"\"\"" "u'''" "U\"\"\"" "U'''")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\UXXXXXXXX")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation")
 (#set! squill.multiline))

((call
   function: (identifier) @_fn
   arguments: (argument_list
     . (string (string_start) @_start . (string_content) @sql . (string_end))))
 (#eq? @_fn "text")
 (#any-of? @_start "r\"\"\"" "r'''" "R\"\"\"" "R'''")
 (#set! squill.raw)
 (#set! squill.multiline))
