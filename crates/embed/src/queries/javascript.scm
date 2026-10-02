; Default extraction query for JavaScript/TypeScript: the first string
; or template-literal argument of `.query` / `.execute` / `.prepare`
; method calls (pg, mysql2, better-sqlite3 style), plus `sql`-tagged
; template literals (postgres.js style). TypeScript reads a generic call
; right after `await` (`await pool.query<Row>(…)`) as a call of the whole
; `await` expression, so that shape is matched too.

((call_expression
   function: [
     (member_expression property: (property_identifier) @_method)
     (await_expression
       (member_expression property: (property_identifier) @_method))
   ]
   arguments: (arguments . [(string) (template_string)] @sql))
 (#any-of? @_method "query" "execute" "prepare"))

((call_expression
   function: (identifier) @_tag
   arguments: (template_string) @sql)
 (#eq? @_tag "sql"))
