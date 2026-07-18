//! TREE-95 acceptance: for arbitrary input, parsing never panics and the
//! resulting tree reproduces the input byte-for-byte, in both dialects.

#![no_main]

use libfuzzer_sys::fuzz_target;
use parser::Dialect;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        for dialect in [Dialect::Postgres, Dialect::Sqlite] {
            let tokens = parser::lexer::lex(input, dialect);
            let parse = parser::parser::parse(&tokens, dialect);
            assert_eq!(parse.cst.text(), input, "{dialect:?} round-trip failed");
        }
    }
});
