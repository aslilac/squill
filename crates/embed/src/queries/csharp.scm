; Default extraction query for C#: raw-string (`"""`) arguments of EF
; Core migrations and raw-SQL calls, ADO.NET's `CommandText`, and
; Dapper's query/execute family, generic (`QueryAsync<Order>`) or not.
; Interpolated raw strings (`$"""`) are a different node and never
; match. Raw strings take no escapes, and a one-line `"""…"""` becomes
; a multi-line one in squill's layout: its content on lines of its own.

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

((assignment_expression
   left: (member_access_expression name: (identifier) @_prop)
   right: (raw_string_literal (raw_string_content) @sql))
 (#eq? @_prop "CommandText")
 (#set! squill.raw) (#set! squill.multiline))
