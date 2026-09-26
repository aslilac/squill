; The first string argument of pgmoon's pg:query, with or without
; parentheses: pg:query([[…]], …), pg:query("…").
((function_call
   name: (method_index_expression method: (identifier) @_method)
   arguments: (arguments . (string content: (string_content) @sql)))
 (#eq? @_method "query"))
