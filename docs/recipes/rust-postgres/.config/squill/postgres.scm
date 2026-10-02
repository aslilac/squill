; The first string argument of postgres / tokio-postgres Client and
; Transaction methods: client.query("…", &[…]), client.execute("…", &[…]).

; A plain string takes escapes, which squill writes back as they're
; spelled; one spanning lines becomes a raw string when it can.
((call_expression
   function: (field_expression field: (field_identifier) @_method)
   arguments: (arguments . (string_literal) @sql))
 (#any-of? @_method
   "query" "query_one" "query_opt" "query_raw" "query_typed"
   "execute" "execute_raw" "prepare" "batch_execute" "simple_query")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation-trim")
 (#set! squill.promote-to-raw-syntax "r#\"{}\"#")
 (#set! squill.promote-to-raw-syntax "r##\"{}\"##")
 (#set! squill.promote-to-raw-syntax "r###\"{}\"###"))

; A raw string takes none. Its delimiters aren't nodes, so its content
; is what's captured.
((call_expression
   function: (field_expression field: (field_identifier) @_method)
   arguments: (arguments . (raw_string_literal (string_content) @sql)))
 (#any-of? @_method
   "query" "query_one" "query_opt" "query_raw" "query_typed"
   "execute" "execute_raw" "prepare" "batch_execute" "simple_query")
 (#set! squill.raw)
 (#set! squill.multiline))

; Byte (`b"…"`) and C (`c"…"`) strings aren't text, and a raw string
; would lose the prefix: skipped.
((string_literal "\"" @_open) @squill.skip
 (#any-of? @_open "b\"" "c\""))
