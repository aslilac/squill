; The first string argument of cursor.execute(…) and friends, and of
; Manager.raw(…): Person.objects.raw("""…"""). Only triple-quoted
; strings, which take line breaks.

; Escapes are written back as they're spelled.
((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list
     . (string (string_start) @_start . (string_content) @sql . (string_end))))
 (#any-of? @_method "execute" "executemany" "raw")
 (#any-of? @_start "\"\"\"" "'''" "u\"\"\"" "u'''" "U\"\"\"" "U'''")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\UXXXXXXXX")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation")
 (#set! squill.multiline))

; An `r` prefix means none.
((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list
     . (string (string_start) @_start . (string_content) @sql . (string_end))))
 (#any-of? @_method "execute" "executemany" "raw")
 (#any-of? @_start "r\"\"\"" "r'''" "R\"\"\"" "R'''")
 (#set! squill.raw)
 (#set! squill.multiline))
