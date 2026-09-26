; The first string argument of postgres / tokio-postgres Client and
; Transaction methods: client.query("…", &[…]), client.execute("…", &[…]).
((call_expression
   function: (field_expression field: (field_identifier) @_method)
   arguments: (arguments . [(string_literal) (raw_string_literal)] @sql))
 (#any-of? @_method
   "query" "query_one" "query_opt" "query_raw" "query_typed"
   "execute" "execute_raw" "prepare" "batch_execute" "simple_query"))
