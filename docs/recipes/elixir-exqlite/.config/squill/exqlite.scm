; A string or ~S sigil passed to Exqlite.Sqlite3's execute/prepare.
; A string with #{} interpolation or an escape has more than one
; child, and never matches; ~s sigils interpolate too, so only ~S is
; taken.
((call
   target: (dot right: (identifier) @_fn)
   (arguments (string . (quoted_content) @sql .)))
 (#any-of? @_fn "execute" "prepare"))

((call
   target: (dot right: (identifier) @_fn)
   (arguments (sigil (sigil_name) @_sigil . (quoted_content) @sql .)))
 (#eq? @_sigil "S")
 (#any-of? @_fn "execute" "prepare"))
