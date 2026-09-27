; The first string argument of lsqlite3's database methods, with or
; without parentheses: db:exec[[…]], db:prepare("…"), db:nrows(…).

; Long strings ([[…]], [=[…]=], …) take line breaks and no escapes.
((function_call
   name: (method_index_expression method: (identifier) @_method)
   arguments: (arguments . (string start: _ @_open content: (string_content) @sql)))
 (#any-of? @_method "exec" "prepare" "nrows" "rows" "urows")
 (#any-of? @_open "[[" "[=[" "[==[" "[===[")
 (#set! squill.raw)
 (#set! squill.multiline))

; Quoted strings take escapes, and a line break only escaped.
((function_call
   name: (method_index_expression method: (identifier) @_method)
   arguments: (arguments . (string start: _ @_open content: (string_content) @sql)))
 (#any-of? @_method "exec" "prepare" "nrows" "rows" "urows")
 (#any-of? @_open "\"" "'"))
