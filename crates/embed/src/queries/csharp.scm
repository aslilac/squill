; SQL in C#: raw-string (`"""`) literals. Raw strings take no escapes,
; and a one-line `"""…"""` can become a multi-line one, its content on
; lines of its own. Interpolated raw strings (`$"""`) are a different
; node: taken only where EF Core binds their holes (below).

; EF Core migrations and raw-SQL calls, and Dapper's query/execute
; family, generic (`QueryAsync<Order>`) or not.
((invocation_expression
   function: (member_access_expression name: [
     (identifier) @_method
     (generic_name (identifier) @_method)
   ])
   arguments: (argument_list
     (argument (raw_string_literal (raw_string_content) @sql))))
 (#any-of? @_method
   "Sql" "ExecuteSql" "ExecuteSqlAsync" "ExecuteSqlRaw" "ExecuteSqlRawAsync"
   "FromSql" "FromSqlRaw" "SqlQuery" "SqlQueryRaw"
   "Query" "QueryAsync" "QueryFirst" "QueryFirstAsync"
   "QueryFirstOrDefault" "QueryFirstOrDefaultAsync"
   "QuerySingle" "QuerySingleAsync"
   "QuerySingleOrDefault" "QuerySingleOrDefaultAsync"
   "QueryMultiple" "QueryMultipleAsync"
   "Execute" "ExecuteAsync" "ExecuteScalar" "ExecuteScalarAsync"
   "ExecuteReader" "ExecuteReaderAsync")
 (#set! squill.raw) (#set! squill.multiline))

; ADO.NET: `cmd.CommandText = """…""";`.
((assignment_expression
   left: (member_access_expression name: (identifier) @_prop)
   right: (raw_string_literal (raw_string_content) @sql))
 (#eq? @_prop "CommandText")
 (#set! squill.raw) (#set! squill.multiline))

; EF Core's interpolated SQL: `FromSql`, `SqlQuery`, and `ExecuteSql`
; take a FormattableString, and send each `{…}` hole as a parameter.
; So squill reads it as one, and writes it back as it was. The `$` and
; `"""` are nodes of their own, so the content is the run between them.
((invocation_expression
   function: (member_access_expression name: [
     (identifier) @_method
     (generic_name (identifier) @_method)
   ])
   arguments: (argument_list
     (argument
       (interpolated_string_expression
         (interpolation_start)
         (interpolation_quote) @_open .
         [(string_content) (interpolation)]+ @sql .
         (interpolation_quote)))))
 (#eq? @_open "\"\"\"")
 (#any-of? @_method
   "FromSql" "SqlQuery" "ExecuteSql" "ExecuteSqlAsync")
 (#set! squill.raw) (#set! squill.multiline))

(interpolated_string_expression (interpolation) @squill.parameter)

