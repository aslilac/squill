; SQL in Swift: multi-line (`"""`) string arguments. These take
; escapes, so aren't `raw`; a string with a `\(…)` interpolation or an
; escape has several content nodes, so the anchored `.` patterns below
; never match it. Raw strings (`#"""`) aren't taken.

; GRDB: the argument labeled `sql:`, in SQLite.
((value_argument
   name: (value_argument_label (simple_identifier) @_label)
   value: (multi_line_string_literal . (multi_line_str_text) @sql.sqlite .))
 (#eq? @_label "sql")
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
         value: (multi_line_string_literal . (multi_line_str_text) @sql .)))))
 (#any-of? @_fn "run" "execute" "prepare" "scalar" "query" "raw")
 (#set! squill.multiline))
