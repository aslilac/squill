; SQL in Go: string arguments of `.Query`-family method calls
; (`database/sql` style).

; An interpreted string takes escapes, which squill reads and writes
; back as they're spelled, but no line breaks: one whose SQL needs
; several lines becomes a raw string, when every escape in it is
; between SQL tokens.
((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list (interpreted_string_literal) @sql))
 (#any-of? @_method
   "Query" "QueryRow" "Exec"
   "QueryContext" "QueryRowContext" "ExecContext")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\UXXXXXXXX")
 (#set! squill.escape "\\NNN")
 (#set! squill.promote-to-raw-syntax "`{}`"))

; A raw string takes line breaks and no escapes.
((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list (raw_string_literal) @sql))
 (#any-of? @_method
   "Query" "QueryRow" "Exec"
   "QueryContext" "QueryRowContext" "ExecContext")
 (#set! squill.raw)
 (#set! squill.multiline))
