; The first string argument of pgmoon's pg:query, with or without
; parentheses: pg:query([[…]], …), pg:query("…").

; Long strings ([[…]], [=[…]=], …) take line breaks and no escapes.
((function_call
   name: (method_index_expression method: (identifier) @_method)
   arguments: (arguments . (string start: _ @_open content: (string_content) @sql)))
 (#eq? @_method "query")
 (#any-of? @_open "[[" "[=[" "[==[" "[===[")
 (#set! squill.raw)
 (#set! squill.multiline))

; Quoted strings take escapes, and a line break only escaped.
((function_call
   name: (method_index_expression method: (identifier) @_method)
   arguments: (arguments . (string start: _ @_open content: (string_content) @sql)))
 (#eq? @_method "query")
 (#any-of? @_open "\"" "'"))
