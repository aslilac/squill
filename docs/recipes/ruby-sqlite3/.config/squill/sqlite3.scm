; A squiggly heredoc (<<~SQL) passed first to the sqlite3 gem's
; Database methods, as a statement or assigned. The heredoc's body
; follows the whole statement in the tree, so the match is the
; statement, then the body right after it. A body with #{}
; interpolation has more than one content node and never matches.
(([
   (call
     method: (identifier) @_method
     arguments: (argument_list . (heredoc_beginning)))
   (assignment
     right: (call
       method: (identifier) @_method
       arguments: (argument_list . (heredoc_beginning))))
  ]
  .
  (heredoc_body . (heredoc_content) @sql . (heredoc_end)))
 (#any-of? @_method
   "execute" "execute_batch" "execute2" "query" "prepare"
   "get_first_row" "get_first_value"))
