; Default extraction query for Gleam: the first string argument of
; `query` / `exec` / `execute` calls, module-qualified (`sqlight.query`,
; `pog.query`) or bare, or a string piped into one (`"…" |> pog.query`,
; `"…" |> sqlight.query(on: db)`). `sqlight` is a SQLite library, so its
; calls carry that dialect; everything else uses the session dialect.

((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql.sqlite)))
 (#eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql)))
 (#not-eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((function_call
   function: (identifier) @_fn
   arguments: (arguments . (argument value: (string) @sql)))
 (#any-of? @_fn "query" "exec" "execute"))

((binary_expression
   left: (string) @sql.sqlite
   operator: "|>"
   right: [
     (field_access record: (identifier) @_mod field: (label) @_fn)
     (function_call
       function: (field_access record: (identifier) @_mod field: (label) @_fn))
   ])
 (#eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((binary_expression
   left: (string) @sql
   operator: "|>"
   right: [
     (field_access record: (identifier) @_mod field: (label) @_fn)
     (function_call
       function: (field_access record: (identifier) @_mod field: (label) @_fn))
   ])
 (#not-eq? @_mod "sqlight")
 (#any-of? @_fn "query" "exec" "execute"))

((binary_expression
   left: (string) @sql
   operator: "|>"
   right: [
     (identifier) @_fn
     (function_call function: (identifier) @_fn)
   ])
 (#any-of? @_fn "query" "exec" "execute"))
