; A string or ~S sigil passed to Ecto's query/query! (Repo.query!,
; Ecto.Adapters.SQL.query!) or a migration's execute. A string with
; #{} interpolation or an escape has more than one child, and never
; matches; ~s sigils interpolate too, so only ~S is taken.
((call
   target: [(identifier) @_fn (dot right: (identifier) @_fn)]
   (arguments (string . (quoted_content) @sql .)))
 (#any-of? @_fn "query" "query!" "execute"))

((call
   target: [(identifier) @_fn (dot right: (identifier) @_fn)]
   (arguments (sigil (sigil_name) @_sigil . (quoted_content) @sql .)))
 (#eq? @_sigil "S")
 (#any-of? @_fn "query" "query!" "execute"))
