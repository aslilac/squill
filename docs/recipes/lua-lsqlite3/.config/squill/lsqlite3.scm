; The first string argument of lsqlite3's database methods, with or
; without parentheses: db:exec[[…]], db:prepare("…"), db:nrows(…).
((function_call
   name: (method_index_expression method: (identifier) @_method)
   arguments: (arguments . (string content: (string_content) @sql)))
 (#any-of? @_method "exec" "prepare" "nrows" "rows" "urows"))
