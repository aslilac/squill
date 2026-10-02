; SQL in Rust: the string argument of sqlx's `query!` / `query_as!` /
; `query_scalar!` / `query_unchecked!` macros, and the first argument
; of its `query` / `query_as` / `query_scalar` functions (any path
; whose last segment matches).

; A plain string in a macro. It takes escapes, which squill reads and
; writes back as they're spelled. One spanning lines becomes a raw
; string, with as few `#`s as it can, when every escape in it is
; between SQL tokens: layout, which squill writes its own way.
((macro_invocation
   macro: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
   ]
   (token_tree (string_literal) @sql))
 (#any-of? @_name "query" "query_as" "query_scalar" "query_unchecked")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation-trim")
 (#set! squill.promote-to-raw-syntax "r#\"{}\"#")
 (#set! squill.promote-to-raw-syntax "r##\"{}\"##")
 (#set! squill.promote-to-raw-syntax "r###\"{}\"###"))

; A raw string in a macro: no escapes. Its delimiters aren't nodes, so
; its content is what's captured.
((macro_invocation
   macro: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
   ]
   (token_tree (raw_string_literal (string_content) @sql)))
 (#any-of? @_name "query" "query_as" "query_scalar" "query_unchecked")
 (#set! squill.raw)
 (#set! squill.multiline))

; The functions, likewise: a plain string,
((call_expression
   function: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
     (generic_function function: [
       (identifier) @_name
       (scoped_identifier name: (identifier) @_name)
     ])
   ]
   arguments: (arguments . (string_literal) @sql))
 (#any-of? @_name "query" "query_as" "query_scalar")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation-trim")
 (#set! squill.promote-to-raw-syntax "r#\"{}\"#")
 (#set! squill.promote-to-raw-syntax "r##\"{}\"##")
 (#set! squill.promote-to-raw-syntax "r###\"{}\"###"))

; and a raw one.
((call_expression
   function: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
     (generic_function function: [
       (identifier) @_name
       (scoped_identifier name: (identifier) @_name)
     ])
   ]
   arguments: (arguments . (raw_string_literal (string_content) @sql)))
 (#any-of? @_name "query" "query_as" "query_scalar")
 (#set! squill.raw)
 (#set! squill.multiline))
