; Default extraction query for Go: string arguments of `.Query`-family
; method calls (`database/sql` style).

((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list
     [(raw_string_literal) (interpreted_string_literal)] @sql))
 (#any-of? @_method
   "Query" "QueryRow" "Exec"
   "QueryContext" "QueryRowContext" "ExecContext"))
