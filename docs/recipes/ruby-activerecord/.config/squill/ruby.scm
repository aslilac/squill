; A heredoc (<<~SQL, <<-SQL, or <<SQL) passed first to ActiveRecord's raw-SQL
; methods or the pg gem's exec family, bare or first in an array (the
; find_by_sql([sql, binds]) form), as a statement or assigned. The
; heredoc's body follows the whole statement in the tree, so the match
; is the statement, then the body right after it. A body with #{}
; interpolation has more than one content node and never matches.
(([
   (call
     method: (identifier) @_method
     arguments: (argument_list .
       [(heredoc_beginning) @_open (array . (heredoc_beginning) @_open)]))
   (assignment
     right: (call
       method: (identifier) @_method
       arguments: (argument_list .
         [(heredoc_beginning) @_open (array . (heredoc_beginning) @_open)])))
  ]
  .
  (heredoc_body . (heredoc_content) @sql . (heredoc_end)))
 (#any-of? @_method
   "execute" "exec_query" "exec_update" "exec_delete" "exec_insert"
   "select_all" "select_one" "select_rows" "select_value" "select_values"
   "find_by_sql" "exec" "exec_params" "async_exec" "async_exec_params")
 (#not-eq? @_open "<<~'SQL'")
 (#not-eq? @_open "<<-'SQL'")
 (#not-eq? @_open "<<'SQL'"))

; The same heredoc single-quoted (<<~'SQL') takes no escapes.
(([
   (call
     method: (identifier) @_method
     arguments: (argument_list .
       [(heredoc_beginning) @_open (array . (heredoc_beginning) @_open)]))
   (assignment
     right: (call
       method: (identifier) @_method
       arguments: (argument_list .
         [(heredoc_beginning) @_open (array . (heredoc_beginning) @_open)])))
  ]
  .
  (heredoc_body . (heredoc_content) @sql . (heredoc_end)))
 (#any-of? @_method
   "execute" "exec_query" "exec_update" "exec_delete" "exec_insert"
   "select_all" "select_one" "select_rows" "select_value" "select_values"
   "find_by_sql" "exec" "exec_params" "async_exec" "async_exec_params")
 (#any-of? @_open "<<~'SQL'" "<<-'SQL'" "<<'SQL'")
 (#set! squill.raw))
