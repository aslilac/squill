//! Dev tool: show why statements fall back (first token/comment
//! divergence between original and rendered).

fn main() {
	let path = std::env::args().nth(1).expect("path");
	let source = std::fs::read_to_string(&path).expect("read");
	let options = formatter::Options { at_params: true, ..Default::default() };
	let lex_options = options.lex_options();
	let tokens = parser::lexer::lex_with(&source, options.dialect, lex_options);
	let parse = parser::parser::parse(&tokens, options.dialect);

	for node in parse.cst.root().children() {
		let original = node.to_string();
		let Some(doc) = formatter::debug_lower(node) else {
			continue;
		};
		let rendered = formatter::render(&doc, &options);
		let ok_tokens = formatter::check::tokens_equivalent(
			&original,
			&rendered,
			options.dialect,
			lex_options,
		);
		let ok_comments = formatter::check::comments_conserved(
			&original,
			&rendered,
			options.dialect,
			lex_options,
		);
		if ok_tokens && ok_comments {
			continue;
		}
		println!(
			"=== fallback (tokens_ok={ok_tokens} comments_ok={ok_comments}) ==="
		);
		let a: Vec<_> =
			parser::lexer::lex_with(&original, options.dialect, lex_options)
				.into_iter()
				.filter(|t| !t.kind.is_trivia())
				.map(|t| (t.kind, t.text.to_string()))
				.collect();
		let b: Vec<_> =
			parser::lexer::lex_with(&rendered, options.dialect, lex_options)
				.into_iter()
				.filter(|t| !t.kind.is_trivia())
				.map(|t| (t.kind, t.text.to_string()))
				.collect();
		for i in 0..a.len().max(b.len()) {
			let x = a.get(i);
			let y = b.get(i);
			let same = match (x, y) {
				(Some(x), Some(y)) => x.0 == y.0 && x.1.eq_ignore_ascii_case(&y.1),
				_ => false,
			};
			if !same {
				let context = |v: &[(parser::syntax::SyntaxKind, String)]| {
					v[i.saturating_sub(5)..(i + 5).min(v.len())]
						.iter()
						.map(|(_, t)| t.as_str())
						.collect::<Vec<_>>()
						.join(" ")
				};
				println!("first divergence at token {i}:");
				println!("  original: ...{}...", context(&a));
				println!("  rendered: ...{}...", context(&b));
				break;
			}
		}
		if !ok_comments {
			let ca = formatter::check::comment_texts(
				&original,
				options.dialect,
				lex_options,
			);
			let cb = formatter::check::comment_texts(
				&rendered,
				options.dialect,
				lex_options,
			);
			for i in 0..ca.len().max(cb.len()) {
				if ca.get(i) != cb.get(i) {
					println!("comment divergence at {i}:");
					println!("  original: {:?}", ca.get(i));
					println!("  rendered: {:?}", cb.get(i));
					break;
				}
			}
		}
	}
}
