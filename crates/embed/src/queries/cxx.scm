; SQL in C++: raw-string (`R"(…)"`) arguments of sqlite3, libpq, and
; libpqxx calls. Raw strings take line breaks and no escapes.

; sqlite3's C API: SQLite, whatever the configured dialect.
((call_expression
   function: (identifier) @_fn
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql.sqlite)))
 (#any-of? @_fn
   "sqlite3_prepare" "sqlite3_prepare_v2" "sqlite3_prepare_v3" "sqlite3_exec")
 (#set! squill.raw) (#set! squill.multiline))

; libpq's C API: Postgres.
((call_expression
   function: (identifier) @_fn
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql.postgres)))
 (#any-of? @_fn "PQexec" "PQexecParams" "PQprepare" "PQsendQuery")
 (#set! squill.raw) (#set! squill.multiline))

; libpqxx-style `exec`/`query` methods, templated (`tx.query<int>`) or
; not, in the configured dialect.
((call_expression
   function: [
     (field_expression field: [
       (field_identifier) @_fn
       (template_method name: (field_identifier) @_fn)
     ])
     (qualified_identifier name: (identifier) @_fn)
   ]
   arguments: (argument_list (raw_string_literal (raw_string_content) @sql)))
 (#any-of? @_fn
   "exec" "exec0" "exec1" "exec_n" "exec_params" "exec_params0"
   "exec_params1" "exec_prepared" "prepare" "query" "query1" "query01"
   "query_n" "query_value" "for_query" "stream")
 (#set! squill.raw) (#set! squill.multiline))
