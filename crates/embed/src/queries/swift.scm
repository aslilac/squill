; SQL in Swift: multi-line (`"""`) string arguments. These take line
; breaks, and escapes, which squill reads and writes back as they're
; spelled. Raw strings (`#"""`) aren't taken.

; GRDB: the argument labeled `sql:`, in SQLite.
((value_argument
   name: (value_argument_label (simple_identifier) @_label)
   value: (multi_line_string_literal) @sql.sqlite)
 (#eq? @_label "sql")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.multiline))

; The first, unlabeled argument of `run` / `execute` / `prepare` /
; `scalar` (SQLite.swift), `query` (PostgresNIO), or `raw` (SQLKit), in
; the configured dialect.
((call_expression
   [
     (simple_identifier) @_fn
     (navigation_expression
       suffix: (navigation_suffix suffix: (simple_identifier) @_fn))
   ]
   (call_suffix
     (value_arguments
       .
       (value_argument
         !name
         value: (multi_line_string_literal) @sql))))
 (#any-of? @_fn "run" "execute" "prepare" "scalar" "query" "raw")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.multiline))

; Swift's escapes aren't `escape_sequence` nodes; these are.
(str_escaped_char) @squill.escape

; A `\(…)` interpolation is SQL with a hole in it: skipped.
(multi_line_string_literal interpolation: (_)) @squill.skip
