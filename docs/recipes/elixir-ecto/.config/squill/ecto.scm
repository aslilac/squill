; A string or ~S sigil passed to Ecto's query/query! (Repo.query!,
; Ecto.Adapters.SQL.query!) or a migration's execute. ~s sigils
; interpolate, so only ~S is taken.

; A string's escapes are read and written back as they're spelled.
((call
   target: [(identifier) @_fn (dot right: (identifier) @_fn)]
   (arguments (string) @sql))
 (#any-of? @_fn "query" "query!" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\u{XXXX}"))

; ~S takes no escapes.
((call
   target: [(identifier) @_fn (dot right: (identifier) @_fn)]
   (arguments (sigil (sigil_name) @_sigil . (quoted_content) @sql .)))
 (#eq? @_sigil "S")
 (#any-of? @_fn "query" "query!" "execute")
 (#set! squill.raw))

; A string with #{} interpolation is SQL with a hole in it: skipped.
(string (interpolation)) @squill.skip
