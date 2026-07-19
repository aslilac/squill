fn main() {
	let source =
		std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
	let options = formatter::Options { at_params: true, ..Default::default() };
	let tokens = parser::lexer::lex_with(
		&source,
		parser::Dialect::Postgres,
		options.lex_options(),
	);
	let parse = parser::parser::parse(&tokens, parser::Dialect::Postgres);
	let formatted = formatter::format_cst(&parse.cst, &options);
	eprintln!("fallbacks: {}", formatted.fallback_statements);
	print!("{}", formatted.text);
}
