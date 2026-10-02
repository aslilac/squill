; Default extraction query for Rust: the string-literal argument of
; sqlx's `query!` / `query_as!` / `query_scalar!` / `query_unchecked!`
; macros, and the first argument of its `query` / `query_as` /
; `query_scalar` functions (any path whose last segment matches).

((macro_invocation
   macro: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
   ]
   (token_tree
     [(string_literal) (raw_string_literal)] @sql))
 (#any-of? @_name "query" "query_as" "query_scalar" "query_unchecked"))

((call_expression
   function: [
     (identifier) @_name
     (scoped_identifier name: (identifier) @_name)
     (generic_function function: [
       (identifier) @_name
       (scoped_identifier name: (identifier) @_name)
     ])
   ]
   arguments: (arguments . [(string_literal) (raw_string_literal)] @sql))
 (#any-of? @_name "query" "query_as" "query_scalar"))
