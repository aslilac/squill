; A string piped into pog.query: "select …" |> pog.query
((binary_expression
   left: (string) @sql
   operator: "|>"
   right: (field_access record: (identifier) @_mod field: (label) @_fn))
 (#eq? @_mod "pog")
 (#eq? @_fn "query"))

; And the default's pog.query("select …"), which this file replaces.
((function_call
   function: (field_access record: (identifier) @_mod field: (label) @_fn)
   arguments: (arguments . (argument value: (string) @sql)))
 (#eq? @_mod "pog")
 (#eq? @_fn "query"))
