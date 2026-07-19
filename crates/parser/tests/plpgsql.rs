//! TREE-102: PL/pgSQL body grammar — unit coverage and all corpus
//! bodies.

use parser::Dialect;
use parser::lexer::lex;
use parser::parser::parse_plpgsql_body;
use std::path::Path;
use std::path::PathBuf;

#[track_caller]
fn parse_ok(body: &str) {
	let tokens = lex(body, Dialect::Postgres);
	let parse = parse_plpgsql_body(&tokens, Dialect::Postgres);
	assert_eq!(parse.cst.text(), body, "round-trip failed");
	assert!(
		parse.diagnostics.is_empty(),
		"diagnostics for {body:?}: {:?}",
		parse.diagnostics
	);
}

#[test]
fn blocks_and_declarations() {
	parse_ok(
		"DECLARE\n\
         \tcount integer := 0;\n\
         \tname text NOT NULL DEFAULT 'x';\n\
         \trec users%ROWTYPE;\n\
         \tv_id users.id%TYPE;\n\
         \targ ALIAS FOR $1;\n\
         BEGIN\n\
         \tRETURN count;\n\
         END;",
	);
	parse_ok("BEGIN NULL; END");
	parse_ok("<<outer>> BEGIN NULL; END outer;");
	parse_ok("BEGIN BEGIN NULL; END; END;");
}

#[test]
fn control_flow() {
	parse_ok(
		"BEGIN\n\
         IF a > 1 THEN RETURN 1;\n\
         ELSIF a > 0 THEN RETURN 0;\n\
         ELSE RETURN -1;\n\
         END IF;\n\
         END;",
	);
	parse_ok(
		"BEGIN\n\
         CASE x WHEN 1, 2 THEN NULL; ELSE NULL; END CASE;\n\
         CASE WHEN x > 0 THEN NULL; END CASE;\n\
         END;",
	);
	parse_ok(
		"BEGIN\n\
         LOOP EXIT WHEN done; END LOOP;\n\
         WHILE i < 10 LOOP i := i + 1; END LOOP;\n\
         FOR i IN 1..10 LOOP CONTINUE WHEN i = 5; END LOOP;\n\
         FOR i IN REVERSE 10..1 BY 2 LOOP NULL; END LOOP;\n\
         FOR r IN SELECT id FROM users LOOP PERFORM audit(r.id); END LOOP;\n\
         FOREACH v IN ARRAY arr LOOP NULL; END LOOP;\n\
         END;",
	);
}

#[test]
fn statements() {
	parse_ok(
		"BEGIN\n\
         x := 1;\n\
         y.z = 'two';\n\
         arr[1] := 3;\n\
         PERFORM pg_notify('chan', payload::text);\n\
         EXECUTE format('DROP TABLE %I', tbl) USING a, b;\n\
         EXECUTE 'SELECT count(*) FROM t' INTO STRICT n;\n\
         GET DIAGNOSTICS n = ROW_COUNT;\n\
         RAISE NOTICE 'value is %', x;\n\
         RAISE EXCEPTION 'bad % and %', a, b USING ERRCODE = 'P0001', HINT = 'no';\n\
         RAISE;\n\
         RETURN NEXT r;\n\
         RETURN QUERY SELECT * FROM t;\n\
         RETURN QUERY EXECUTE 'SELECT 1';\n\
         RETURN;\n\
         END;",
	);
}

#[test]
fn embedded_sql_with_into() {
	parse_ok(
		"BEGIN\n\
         SELECT id, name INTO STRICT v_id, v_name FROM users WHERE id = uid;\n\
         INSERT INTO audit (kind) VALUES ('x') RETURNING id INTO aid;\n\
         UPDATE t SET n = n + 1 WHERE id = 1;\n\
         DELETE FROM t WHERE id = 2;\n\
         CREATE TEMP TABLE scratch (id int);\n\
         END;",
	);
}

#[test]
fn exception_handlers() {
	parse_ok(
		"BEGIN\n\
         INSERT INTO t VALUES (1);\n\
         EXCEPTION\n\
         WHEN unique_violation THEN NULL;\n\
         WHEN OTHERS THEN\n\
         RAISE WARNING 'oops: %', SQLERRM;\n\
         RETURN false;\n\
         END;",
	);
}

#[test]
fn recovery_keeps_tokens() {
	let body = "BEGIN\nFROBNICATE badly here;\nRETURN 1;\nEND;";
	let tokens = lex(body, Dialect::Postgres);
	let parse = parse_plpgsql_body(&tokens, Dialect::Postgres);
	assert_eq!(parse.cst.text(), body, "recovery must be lossless");
	assert!(!parse.diagnostics.is_empty());
}

// ---- corpus ----

fn collect_sql_files(dir: &Path, out: &mut Vec<PathBuf>) {
	for entry in std::fs::read_dir(dir).expect("read dir") {
		let path = entry.expect("entry").path();
		if path.is_dir() {
			collect_sql_files(&path, out);
		} else if path.extension().is_some_and(|ext| ext == "sql") {
			out.push(path);
		}
	}
}

/// Every `LANGUAGE plpgsql` dollar-quoted body in the corpus parses with
/// zero error statements and round-trips losslessly.
#[test]
fn corpus_plpgsql_bodies_parse() {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
	let mut files = Vec::new();
	collect_sql_files(&root, &mut files);
	files.sort();

	let mut bodies = 0;
	let mut failures = Vec::new();
	for path in files {
		let source = std::fs::read_to_string(&path).expect("read corpus file");
		if !source.to_ascii_lowercase().contains("plpgsql") {
			continue;
		}
		let tokens = lex(&source, Dialect::Postgres);
		for token in tokens
			.iter()
			.filter(|t| t.kind == parser::syntax::SyntaxKind::DollarString)
		{
			let Some(open) = token.text[1..].find('$').map(|i| i + 2) else {
				continue;
			};
			let tag = &token.text[..open];
			let Some(body) = token.text[tag.len()..].strip_suffix(tag) else {
				continue;
			};
			if body.trim().is_empty() {
				continue;
			}
			bodies += 1;
			let body_tokens = lex(body, Dialect::Postgres);
			let parse = parse_plpgsql_body(&body_tokens, Dialect::Postgres);
			assert_eq!(
				parse.cst.text(),
				body,
				"round-trip failed in {}",
				path.display()
			);
			if !parse.diagnostics.is_empty() {
				failures.push(format!(
					"{}: {}",
					path.display(),
					parse.diagnostics[0].message
				));
			}
		}
	}
	assert!(bodies >= 70, "expected at least 70 bodies, found {bodies}");
	assert!(
		failures.is_empty(),
		"{} of {bodies} bodies failed:\n{}",
		failures.len(),
		failures.join("\n")
	);
}
