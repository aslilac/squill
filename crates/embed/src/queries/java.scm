; SQL in Java: text-block (`"""`) arguments of JDBC, JPA, and Spring
; `JdbcTemplate` calls.

; The whole literal is captured, since the grammar has no node spanning
; a text block's content, and one holding an escape is reported, not
; rewritten. Neither property holds: text blocks take escapes, and the
; capture can't tell a text block from a `"…"` string, which can't take
; a line break.
((method_invocation
   name: (identifier) @_method
   arguments: (argument_list (string_literal) @sql))
 (#any-of? @_method
   "prepareStatement" "prepareCall" "executeQuery" "executeUpdate"
   "executeLargeUpdate" "execute" "addBatch"
   "createQuery" "createNativeQuery"
   "query" "queryForObject" "queryForList" "queryForMap"
   "queryForRowSet" "queryForStream" "update" "batchUpdate"))
