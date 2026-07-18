//! Parser: tokens to a concrete syntax tree.

use std::fmt;

use crate::lexer::Token;
pub use crate::tree::Cst;

/// Errors produced while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// The parser is not implemented yet.
    Unimplemented,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Unimplemented => f.write_str("parser not implemented"),
        }
    }
}

impl std::error::Error for ParseError {}

/// Parse a token stream into a CST.
pub fn parse(_tokens: &[Token<'_>]) -> Result<Cst, ParseError> {
    Err(ParseError::Unimplemented)
}
