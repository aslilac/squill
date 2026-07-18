//! TREE-94 acceptance: lossless event-stream tree building and the trivia
//! attachment policy, exercised through a hand-built toy grammar.

use parser::Dialect;
use parser::ast::{AstNode, Root, Statement};
use parser::lexer::lex;
use parser::syntax::SyntaxKind;
use parser::tree::{Cst, Event, build_tree};

/// Toy grammar: `Root = Statement*`, where a statement is any run of
/// tokens up to and including `;`.
fn toy_parse(input: &str) -> Cst {
    let tokens = lex(input, Dialect::Postgres);
    let mut events = vec![Event::StartNode(SyntaxKind::Root)];
    let mut in_statement = false;
    for token in &tokens {
        if token.kind.is_trivia() {
            continue;
        }
        if !in_statement {
            events.push(Event::StartNode(SyntaxKind::Statement));
            in_statement = true;
        }
        events.push(Event::Token);
        if token.kind == SyntaxKind::Semicolon {
            events.push(Event::FinishNode);
            in_statement = false;
        }
    }
    if in_statement {
        events.push(Event::FinishNode);
    }
    events.push(Event::FinishNode);
    build_tree(&tokens, &events)
}

fn statement_texts(cst: &Cst) -> Vec<String> {
    let root = Root::cast(cst.root()).expect("root node");
    root.statements()
        .map(|statement| statement.syntax().to_string())
        .collect()
}

#[track_caller]
fn assert_roundtrip(input: &str) -> Cst {
    let cst = toy_parse(input);
    assert_eq!(cst.text(), input, "tree text must reproduce the input");
    cst
}

#[test]
fn roundtrip_lossless() {
    for input in [
        "",
        "select 1;",
        "select 1;select 2",
        "-- only a comment",
        "\n\n  /* multi\n line */ select $tag$x$tag$ from t; -- t\n",
        "select /* inner */ 'it''s' || E'\\n' from x where a @> b;\n",
    ] {
        assert_roundtrip(input);
    }
}

#[test]
fn comment_between_statements_attaches_to_next() {
    let cst = assert_roundtrip("select 1;\n-- separator\nselect 2;");
    let stmts = statement_texts(&cst);
    assert_eq!(stmts.len(), 2);
    assert_eq!(stmts[0], "select 1;");
    // Leading newline whitespace also attaches to the following statement,
    // so blank-line policy can later be decided from leading trivia alone.
    assert_eq!(stmts[1], "\n-- separator\nselect 2;");
}

#[test]
fn trailing_comment_stays_with_its_statement() {
    let cst = assert_roundtrip("select 1; -- one\nselect 2;");
    let stmts = statement_texts(&cst);
    assert_eq!(stmts[0], "select 1; -- one");
    assert_eq!(stmts[1], "\nselect 2;");
}

#[test]
fn comment_inside_expression() {
    let cst = assert_roundtrip("select /* the answer */ 42;");
    let stmts = statement_texts(&cst);
    assert_eq!(stmts, ["select /* the answer */ 42;"]);
    // The comment is a token inside the statement node, before `42`.
    let root = Root::cast(cst.root()).expect("root");
    let statement = root.statements().next().expect("statement");
    let kinds: Vec<_> = statement
        .syntax()
        .children_with_tokens()
        .map(|element| element.kind())
        .collect();
    assert!(kinds.contains(&SyntaxKind::BlockComment));
}

#[test]
fn comment_before_eof_without_trailing_newline() {
    // Same line as the last statement: attaches inside it.
    let cst = assert_roundtrip("select 1; -- bye");
    assert_eq!(statement_texts(&cst), ["select 1; -- bye"]);

    // On its own line: stays outside the statement, absorbed by Root.
    let cst = assert_roundtrip("select 1;\n-- bye");
    assert_eq!(statement_texts(&cst), ["select 1;"]);
}

#[test]
fn sqlc_annotations_lead_their_statement() {
    let input = "\
-- name: GetAPIKeyByID :one
SELECT * FROM api_keys WHERE id = $1;

-- name: ListAPIKeys :many
SELECT * FROM api_keys;
";
    let cst = assert_roundtrip(input);
    let stmts = statement_texts(&cst);
    assert_eq!(stmts.len(), 2);
    assert!(
        stmts[0].starts_with("-- name: GetAPIKeyByID :one\n"),
        "first statement must carry its annotation, got {:?}",
        stmts[0]
    );
    assert!(
        stmts[1]
            .trim_start()
            .starts_with("-- name: ListAPIKeys :many"),
        "second statement must carry its annotation, got {:?}",
        stmts[1]
    );
}

#[test]
fn typed_accessors() {
    let cst = assert_roundtrip("select 1;\nselect 2;");
    let root = Root::cast(cst.root()).expect("root");
    let statements: Vec<Statement<'_>> = root.statements().collect();
    assert_eq!(statements.len(), 2);
    for statement in statements {
        let semicolon = statement.semicolon().expect("semicolon token");
        assert_eq!(semicolon.text(), ";");
    }
    // Casting to the wrong kind fails.
    assert!(Statement::cast(cst.root()).is_none());
}
