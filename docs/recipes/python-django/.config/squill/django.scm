; The first string argument of cursor.execute(…) and friends, and of
; Manager.raw(…): Person.objects.raw("""…""").
((call
   function: (attribute attribute: (identifier) @_method)
   arguments: (argument_list . (string) @sql))
 (#any-of? @_method "execute" "executemany" "raw"))
