//! TREE-93 acceptance: for arbitrary input, lexing never panics and the
//! token texts concatenate back to the input byte-for-byte, in both dialects.

#![no_main]

use libfuzzer_sys::fuzz_target;
use parser::Dialect;

fuzz_target!(|data: &[u8]| {
    if let Ok(input) = std::str::from_utf8(data) {
        for dialect in [Dialect::Postgres, Dialect::Sqlite] {
            let tokens = parser::lexer::lex(input, dialect);
            let rebuilt: String = tokens.iter().map(|t| t.text).collect();
            assert_eq!(rebuilt, input, "{dialect:?} round-trip failed");
        }
    }
});
