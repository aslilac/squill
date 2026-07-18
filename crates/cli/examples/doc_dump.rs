fn main() {
    let source = std::fs::read_to_string(std::env::args().nth(1).unwrap()).unwrap();
    let options = formatter::Options {
        at_params: true,
        ..Default::default()
    };
    let tokens = parser::lexer::lex_with(&source, options.dialect, options.lex_options());
    let parse = parser::parser::parse(&tokens, options.dialect);
    for node in parse.cst.root().children() {
        if let Some(doc) = formatter::debug_lower(node) {
            println!("{doc:#?}");
        }
    }
}
