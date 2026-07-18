//! TREE-95 acceptance: SELECT grammar coverage, error recovery, and the
//! lossless invariant.

use parser::Dialect;
use parser::lexer::lex;
use parser::parser::parse;
use parser::syntax::SyntaxKind;
use parser::tree::Cst;

/// Parse expecting full success (no ErrorStatements), and assert the
/// lossless round-trip.
#[track_caller]
fn parse_ok(sql: &str) -> Cst {
    let tokens = lex(sql, Dialect::Postgres);
    let parse = parse(&tokens, Dialect::Postgres);
    assert_eq!(parse.cst.text(), sql, "round-trip failed");
    assert!(
        parse.diagnostics.is_empty(),
        "unexpected diagnostics for {sql:?}: {:?}",
        parse.diagnostics
    );
    parse.cst
}

fn top_level_kinds(cst: &Cst) -> Vec<SyntaxKind> {
    cst.root().children().map(|node| node.kind()).collect()
}

// ---- SELECT feature coverage ----

#[test]
fn with_recursive_and_materialized() {
    parse_ok(
        "WITH RECURSIVE t AS MATERIALIZED (SELECT 1), u(a, b) AS NOT MATERIALIZED (SELECT 2, 3) \
         SELECT * FROM t, u;",
    );
}

#[test]
fn with_search_and_cycle() {
    parse_ok(
        "WITH RECURSIVE t(a) AS (SELECT 1 UNION ALL SELECT a + 1 FROM t) \
         SEARCH BREADTH FIRST BY a SET ordercol \
         CYCLE a SET is_cycle USING path \
         SELECT * FROM t LIMIT 10;",
    );
}

#[test]
fn join_forms() {
    parse_ok(
        "SELECT * FROM a JOIN b ON a.x = b.x \
         LEFT OUTER JOIN c USING (id) AS ualias \
         NATURAL RIGHT JOIN d \
         CROSS JOIN e, \
         f AS alias(x, y), \
         LATERAL (SELECT 1) AS l, \
         ONLY g, \
         generate_series(1, 10) WITH ORDINALITY AS gs(n, ord), \
         (h JOIN i ON h.a = i.a);",
    );
}

#[test]
fn distinct_forms() {
    parse_ok("SELECT DISTINCT a FROM t;");
    parse_ok("SELECT DISTINCT ON (a, b) a, b, c FROM t ORDER BY a, b;");
    parse_ok("SELECT ALL a FROM t;");
}

#[test]
fn group_by_forms() {
    parse_ok(
        "SELECT a, b FROM t \
         GROUP BY GROUPING SETS ((a), (a, b), ()), ROLLUP (a, b), CUBE (a), a \
         HAVING count(*) > 1;",
    );
    parse_ok("SELECT a FROM t GROUP BY ALL a;");
    parse_ok("SELECT a FROM t GROUP BY DISTINCT ROLLUP (a);");
}

#[test]
fn aggregate_clauses() {
    parse_ok(
        "SELECT count(*) FILTER (WHERE x > 0), \
         percentile_cont(0.5) WITHIN GROUP (ORDER BY y DESC), \
         string_agg(DISTINCT z, ',' ORDER BY z), \
         array_agg(VARIADIC v) \
         FROM t;",
    );
}

#[test]
fn window_functions() {
    parse_ok(
        "SELECT sum(x) OVER w, \
         avg(y) OVER (PARTITION BY a, b ORDER BY c \
                      ROWS BETWEEN 1 PRECEDING AND CURRENT ROW EXCLUDE TIES), \
         row_number() OVER (w RANGE BETWEEN UNBOUNDED PRECEDING AND UNBOUNDED FOLLOWING \
                            EXCLUDE NO OTHERS), \
         first_value(z) OVER (GROUPS CURRENT ROW EXCLUDE GROUP), \
         nth_value(z, 2) OVER () \
         FROM t \
         WINDOW w AS (ORDER BY x RANGE UNBOUNDED PRECEDING), \
                w2 AS (w PARTITION BY y);",
    );
}

#[test]
fn set_operations_and_tail_clauses() {
    parse_ok(
        "SELECT 1 UNION ALL SELECT 2 INTERSECT DISTINCT SELECT 3 EXCEPT SELECT 4 \
         ORDER BY 1 DESC NULLS LAST OFFSET 2 LIMIT 5;",
    );
    parse_ok("SELECT a FROM t OFFSET 5 LIMIT 2;");
    parse_ok("SELECT a FROM t ORDER BY a USING < FETCH FIRST 10 ROWS WITH TIES;");
    parse_ok("SELECT a FROM t FETCH NEXT ROW ONLY;");
    parse_ok("SELECT a FROM t FOR UPDATE OF t SKIP LOCKED FOR SHARE NOWAIT;");
    parse_ok("SELECT a FROM t FOR NO KEY UPDATE;");
    parse_ok("SELECT a FROM t FOR KEY SHARE;");
    parse_ok("(SELECT 1 ORDER BY 1) UNION (SELECT 2 LIMIT 1);");
    parse_ok("TABLE t UNION SELECT 1;");
    parse_ok("VALUES (1, 'a'), (2, 'b') ORDER BY 1;");
}

#[test]
fn expression_zoo() {
    parse_ok(
        "SELECT a::int[], b::character varying(10), c::numeric(10, 2), \
         arr[1], arr[1:2], arr[:2], arr[1:], mx[1][2], \
         x BETWEEN SYMMETRIC 1 AND 10, y NOT BETWEEN 1 AND 2, \
         c1 IS NOT DISTINCT FROM d, c2 IS DISTINCT FROM e, \
         f IS NULL, g IS NOT TRUE, h ISNULL, i NOTNULL, \
         j COLLATE \"en_US\", k AT TIME ZONE 'utc', \
         CASE WHEN x THEN 1 WHEN y THEN 2 ELSE 3 END, \
         CASE x WHEN 1 THEN 'a' END, \
         ARRAY[1, 2, 3], ARRAY[ARRAY[1], ARRAY[2]], ARRAY(SELECT 1), \
         ROW(1, 2), (1, 2, 3), \
         EXISTS (SELECT 1 FROM t), \
         q IN (1, 2), r NOT IN (SELECT z FROM t), \
         s > ANY (SELECT w FROM t), s2 <= ALL (ARRAY[1]), s3 = SOME (1, 2), \
         CAST(v AS timestamp with time zone), \
         n NOT LIKE 'a%' ESCAPE '!', n2 ILIKE 'b_', n3 SIMILAR TO 'x+', \
         interval '1 day', timestamp '2020-01-01', \
         NOT (p AND q2 OR r2), \
         -x + +y, ~bits, @absval, \
         amount * 1.5 ^ 2 % 3 - 7, \
         tags @> ARRAY['a'], meta -> 'k' ->> 'j', \
         ((SELECT 1), 2) \
         FROM t;",
    );
}

#[test]
fn nested_subquery_disambiguation() {
    // `((SELECT ...))` both as scalar and in FROM.
    parse_ok("SELECT ((SELECT 1));");
    parse_ok("SELECT * FROM ((SELECT 1) UNION (SELECT 2)) AS u;");
    parse_ok("SELECT * FROM ((SELECT 1 AS a) s JOIN t ON t.a = s.a);");
    parse_ok("SELECT ((SELECT max(x) FROM t), 'k');");
}

#[test]
fn sqlc_style_params() {
    // sqlc uses `@name` args and `sqlc.arg('x')` calls against Postgres.
    parse_ok("SELECT * FROM t WHERE id = @id AND org = sqlc.narg('org')::uuid LIMIT @lim;");
}

// ---- statement framework ----

#[test]
fn empty_statements() {
    let cst = parse_ok(";;");
    assert_eq!(
        top_level_kinds(&cst),
        [SyntaxKind::EmptyStmt, SyntaxKind::EmptyStmt]
    );
}

#[test]
fn error_recovery_resumes_at_semicolon() {
    let sql = "SELECT 1; FROBNICATE the database; SELECT 2;";
    let tokens = lex(sql, Dialect::Postgres);
    let parse = parse(&tokens, Dialect::Postgres);
    assert_eq!(parse.cst.text(), sql, "ErrorStatement must keep all tokens");
    assert_eq!(
        top_level_kinds(&parse.cst),
        [
            SyntaxKind::SelectStmt,
            SyntaxKind::ErrorStatement,
            SyntaxKind::SelectStmt
        ]
    );
    assert_eq!(parse.diagnostics.len(), 1);
    let diagnostic = &parse.diagnostics[0];
    assert_eq!(
        &sql[diagnostic.start..diagnostic.end],
        "FROBNICATE",
        "diagnostic span should point at the failure"
    );
}

#[test]
fn error_statement_keeps_partial_select() {
    // A select that goes wrong mid-way must roll back into a full
    // ErrorStatement, not a half-built tree.
    let sql = "SELECT a FROM WHERE ORDER;";
    let tokens = lex(sql, Dialect::Postgres);
    let parse = parse(&tokens, Dialect::Postgres);
    assert_eq!(parse.cst.text(), sql);
    assert_eq!(top_level_kinds(&parse.cst), [SyntaxKind::ErrorStatement]);
    assert_eq!(parse.diagnostics.len(), 1);
}

#[test]
fn sqlite_trigger_body_is_one_statement() {
    let sql = "CREATE TRIGGER tr AFTER INSERT ON t BEGIN \
               UPDATE x SET y = 1; DELETE FROM z; END; \
               SELECT 1;";
    let tokens = lex(sql, Dialect::Sqlite);
    let parse = parse(&tokens, Dialect::Sqlite);
    assert_eq!(parse.cst.text(), sql);
    assert_eq!(
        top_level_kinds(&parse.cst),
        [SyntaxKind::DdlStmt, SyntaxKind::SelectStmt],
        "trigger body semicolons must not split the statement"
    );
    assert!(parse.diagnostics.is_empty());
}

#[test]
fn sqlite_begin_transaction_is_not_a_block() {
    let sql = "BEGIN TRANSACTION; SELECT 1;";
    let tokens = lex(sql, Dialect::Sqlite);
    let parse = parse(&tokens, Dialect::Sqlite);
    assert_eq!(parse.cst.text(), sql);
    assert_eq!(
        top_level_kinds(&parse.cst),
        [SyntaxKind::DdlStmt, SyntaxKind::SelectStmt],
        "BEGIN TRANSACTION must end at its own semicolon"
    );
    assert!(parse.diagnostics.is_empty());
}

#[test]
fn dollar_quoted_semicolons_are_not_boundaries() {
    let sql = "SELECT $fn$ a; b; c $fn$; SELECT 2;";
    let cst = parse_ok(sql);
    assert_eq!(
        top_level_kinds(&cst),
        [SyntaxKind::SelectStmt, SyntaxKind::SelectStmt]
    );
}

#[test]
fn pathological_nesting_does_not_overflow() {
    for sql in [
        format!("SELECT {}1{};", "(".repeat(5000), ")".repeat(5000)),
        format!("SELECT * FROM {}t{};", "(".repeat(5000), ")".repeat(5000)),
        "(".repeat(20000),
    ] {
        let tokens = lex(&sql, Dialect::Postgres);
        let parse = parse(&tokens, Dialect::Postgres);
        assert_eq!(parse.cst.text(), sql, "lost tokens on deep nesting");
    }
}

#[test]
fn garbage_never_loses_tokens() {
    for sql in [
        "SELECT ((((;",
        "'unterminated",
        ")))) select ;;; (",
        "SELECT FROM WHERE; SELECT 1; GROUP;",
    ] {
        for dialect in [Dialect::Postgres, Dialect::Sqlite] {
            let tokens = lex(sql, dialect);
            let parse = parse(&tokens, dialect);
            assert_eq!(parse.cst.text(), sql, "{dialect:?} lost tokens for {sql:?}");
        }
    }
}
