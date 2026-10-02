; SQL in Java: text-block (`"""`) arguments of JDBC, JPA, and Spring
; `JdbcTemplate` calls.

; A text block takes line breaks, and escapes, which squill reads and
; writes back as they're spelled. A plain `"…"` string can't take a line
; break, so it isn't taken.
((method_invocation
   name: (identifier) @_method
   arguments: (argument_list (string_literal "\"\"\"") @sql))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "executeLargeUpdate" "execute" "addBatch"
   "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "queryForRowSet" "queryForStream" "update" "batchUpdate")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\NNN")
 (#set! squill.multiline))

; A string template's `\{…}` is SQL with a hole in it: skipped.
(string_literal (string_interpolation)) @squill.skip
