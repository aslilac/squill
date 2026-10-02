; Default extraction query for Kotlin: raw-string (`"""`) arguments of
; JDBC, Spring, and Exposed calls, bare or `.trimIndent()`ed. A raw
; string with `$` templates has several content nodes and never matches.
; Raw strings take line breaks and no escapes.

((call_expression
   [
     (identifier) @_method
     (navigation_expression (identifier) @_method .)
   ]
   (value_arguments
     (value_argument
       (multiline_string_literal . (string_content) @sql .))))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "execute" "addBatch" "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "update" "batchUpdate" "exec")
 (#set! squill.raw) (#set! squill.multiline))

((call_expression
   [
     (identifier) @_method
     (navigation_expression (identifier) @_method .)
   ]
   (value_arguments
     (value_argument
       (call_expression
         (navigation_expression
           (multiline_string_literal . (string_content) @sql .)
           (identifier) @_trim)))))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "execute" "addBatch" "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "update" "batchUpdate" "exec")
 (#eq? @_trim "trimIndent")
 (#set! squill.raw) (#set! squill.multiline))
