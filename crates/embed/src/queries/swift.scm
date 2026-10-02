; Default extraction query for Swift: multi-line (`"""`) string
; arguments. One labeled `sql:` is GRDB's, and SQLite; the first,
; unlabeled argument of `run` / `execute` / `prepare` / `scalar`
; (SQLite.swift), `query` (PostgresNIO), or `raw` (SQLKit) uses the
; configured dialect. A string with a `\(…)` interpolation or an escape
; has several content nodes and never matches; raw strings (`#"""`)
; aren't taken. Multi-line strings take escapes, so aren't `raw`.

((value_argument
   name: (value_argument_label (simple_identifier) @_label)
   value: (multi_line_string_literal . (multi_line_str_text) @sql.sqlite .))
 (#eq? @_label "sql")
 (#set! squill.multiline))

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
