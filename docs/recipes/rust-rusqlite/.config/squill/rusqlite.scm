; The first string argument of rusqlite's Connection and Transaction
; methods: conn.execute("…", params![…]), conn.prepare("…"), and so on.
((call_expression
   function: (field_expression field: (field_identifier) @_method)
   arguments: (arguments . [(string_literal) (raw_string_literal)] @sql))
 (#any-of? @_method
   "execute" "execute_batch" "prepare" "prepare_cached"
   "query_row" "query_one"))
