//! Dev tool: run a built-in extraction query against a snippet and show
//! captures.

fn main() {
	use streaming_iterator::StreamingIterator;
	let src =
		"pub fn list(db) {\n  sqlight.query(\"select 1\", on: db, with: [])\n}\n";
	let language: tree_sitter::Language = tree_sitter_gleam::LANGUAGE.into();
	let query =
		tree_sitter::Query::new(&language, embed::GLEAM_SQL_QUERY).expect("query");
	let mut parser = tree_sitter::Parser::new();
	parser.set_language(&language).expect("language");
	let tree = parser.parse(src, None).expect("parse");
	let mut cursor = tree_sitter::QueryCursor::new();
	let mut matches = cursor.matches(&query, tree.root_node(), src.as_bytes());
	while let Some(m) = matches.next() {
		for c in m.captures {
			println!(
				"{}: {:?}",
				query.capture_names()[c.index as usize],
				&src[c.node.byte_range()]
			);
		}
	}
	println!("done");
}
