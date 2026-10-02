; SQL in JavaScript and TypeScript: template literals, which take line
; breaks, and escapes, which squill reads and writes back as they're
; spelled.

; The first argument of `.query` / `.execute` / `.prepare` method calls
; (pg, mysql2, better-sqlite3 style). TypeScript reads a generic call
; right after `await` (`await pool.query<Row>(…)`) as a call of the
; whole `await` expression, so that shape is matched too.
((call_expression
   function: [
     (member_expression property: (property_identifier) @_method)
     (await_expression
       (member_expression property: (property_identifier) @_method))
   ]
   arguments: (arguments . (template_string) @sql))
 (#any-of? @_method "query" "execute" "prepare")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\$")
 (#set! squill.escape "\\`")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation")
 (#set! squill.multiline))

; `sql`-tagged templates (postgres.js style).
((call_expression
   function: (identifier) @_tag
   arguments: (template_string) @sql)
 (#eq? @_tag "sql")
 (#set! squill.escape "whitespace")
 (#set! squill.escape "punctuation")
 (#set! squill.escape "\\$")
 (#set! squill.escape "\\`")
 (#set! squill.escape "\\xHH")
 (#set! squill.escape "\\uXXXX")
 (#set! squill.escape "\\u{XXXX}")
 (#set! squill.escape "\\NNN")
 (#set! squill.escape "line-continuation")
 (#set! squill.multiline))

; A template with a `${…}` hole is SQL with a hole in it: skipped.
(template_substitution) @squill.skip
