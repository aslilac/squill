; A heredoc (<<~SQL, <<-SQL, or <<SQL) passed first to the sqlite3 gem's
; Database methods, as a statement or assigned. The heredoc's body
; follows the whole statement in the tree, so the match is the
; statement, then the body right after it. Its content and
; escapes are what's captured; squill reads the escapes and writes them
; back as they're spelled.
(([
   (call
     method: (identifier) @_method
     arguments: (argument_list . (heredoc_beginning) @_open))
   (assignment
     right: (call
       method: (identifier) @_method
       arguments: (argument_list . (heredoc_beginning) @_open)))
  ]
  .
  (heredoc_body [(heredoc_content) (escape_sequence)]+ @sql . (heredoc_end)))
 (#any-of? @_method
   "execute" "execute_batch" "execute2" "query" "prepare"
   "get_first_row" "get_first_value")
 (#not-eq? @_open "<<~'SQL'")
 (#not-eq? @_open "<<-'SQL'")
 (#not-eq? @_open "<<'SQL'")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN"))

; The same heredoc single-quoted (<<~'SQL') takes no escapes.
(([
   (call
     method: (identifier) @_method
     arguments: (argument_list . (heredoc_beginning) @_open))
   (assignment
     right: (call
       method: (identifier) @_method
       arguments: (argument_list . (heredoc_beginning) @_open)))
  ]
  .
  (heredoc_body . (heredoc_content) @sql . (heredoc_end)))
 (#any-of? @_method
   "execute" "execute_batch" "execute2" "query" "prepare"
   "get_first_row" "get_first_value")
 (#any-of? @_open "<<~'SQL'" "<<-'SQL'" "<<'SQL'")
 (#set! squill.raw))

; A heredoc with #{} interpolation is SQL with a hole in it: skipped.
(heredoc_body (interpolation)) @squill.skip
