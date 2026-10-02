; SQL in Gleam: the first string argument of `query` / `exec` /
; `execute` calls, or a string piped into one. Every Gleam string takes
; line breaks, and escapes, which squill reads and writes back as
; they're spelled. `sqlight` is a SQLite library, so its calls carry
; that dialect; everything else uses the configured one.

; `sqlight.query("…")`.
((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql.sqlite)))
 (#eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.multiline))

; `pog.query("…")`, and any other module's.
((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql)))
 (#not-eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.multiline))

; `query("…")`, imported bare.
((function_call
   function: (identifier) @_fn
   arguments: (arguments . (argument value: (string) @sql)))
 (#any-of? @_fn "query" "exec" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.multiline))

; `"…" |> sqlight.query(on: db)`.
((binary_expression
   left: (string) @sql.sqlite
   operator: "|>"
   right: [
     (field_access record: (identifier) @_mod field: (label) @_fn)
     (function_call
       function: (field_access record: (identifier) @_mod field: (label) @_fn))
   ])
 (#eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.multiline))

; `"…" |> pog.query`, and any other module's.
((binary_expression
   left: (string) @sql
   operator: "|>"
   right: [
     (field_access record: (identifier) @_mod field: (label) @_fn)
     (function_call
       function: (field_access record: (identifier) @_mod field: (label) @_fn))
   ])
 (#not-eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.multiline))

; `"…" |> query`, imported bare.
((binary_expression
   left: (string) @sql
   operator: "|>"
   right: [
     (identifier) @_fn
     (function_call function: (identifier) @_fn)
   ])
 (#any-of? @_fn "query" "exec" "execute")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.multiline))
