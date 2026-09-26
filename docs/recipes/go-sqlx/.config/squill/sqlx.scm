; String arguments of database/sql's methods, which sqlx keeps, and of
; sqlx's own: db.Select(&rows, `…`), db.Get(&row, `…`), db.NamedExec(`…`, v).
((call_expression
   function: (selector_expression field: (field_identifier) @_method)
   arguments: (argument_list
     [(raw_string_literal) (interpreted_string_literal)] @sql))
 (#any-of? @_method
   "Query" "QueryRow" "Exec" "Prepare"
   "QueryContext" "QueryRowContext" "ExecContext" "PrepareContext"
   "Select" "Get" "SelectContext" "GetContext"
   "Queryx" "QueryRowx" "QueryxContext" "QueryRowxContext"
   "MustExec" "MustExecContext" "Preparex" "PreparexContext"
   "NamedExec" "NamedExecContext" "NamedQuery" "NamedQueryContext"
   "PrepareNamed" "PrepareNamedContext" "Rebind"))
