; String arguments of database/sql's methods, which sqlx keeps, and of
; sqlx's own: db.Select(&rows, `…`), db.Get(&row, `…`), db.NamedExec(`…`, v).

; An interpreted string takes escapes, which squill writes back as
; they're spelled, but no line breaks: SQL that needs some moves into a
; raw string, when it can.
((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list (interpreted_string_literal) @sql))
 (#any-of? @_method
   "Query" "QueryRow" "Exec" "Prepare"
   "QueryContext" "QueryRowContext" "ExecContext" "PrepareContext"
   "Select" "Get" "SelectContext" "GetContext"
   "Queryx" "QueryRowx" "QueryxContext" "QueryRowxContext"
   "MustExec" "MustExecContext" "Preparex" "PreparexContext"
   "NamedExec" "NamedExecContext" "NamedQuery" "NamedQueryContext"
   "PrepareNamed" "PrepareNamedContext" "Rebind")
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
   "Query" "QueryRow" "Exec" "Prepare"
   "QueryContext" "QueryRowContext" "ExecContext" "PrepareContext"
   "Select" "Get" "SelectContext" "GetContext"
   "Queryx" "QueryRowx" "QueryxContext" "QueryRowxContext"
   "MustExec" "MustExecContext" "Preparex" "PreparexContext"
   "NamedExec" "NamedExecContext" "NamedQuery" "NamedQueryContext"
   "PrepareNamed" "PrepareNamedContext" "Rebind")
 (#set! squill.raw)
 (#set! squill.multiline))
