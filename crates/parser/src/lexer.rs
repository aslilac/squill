//! Lexer: source text to tokens.

use std::fmt;

use crate::syntax::SyntaxKind;

/// A single lexed token: its kind plus the source text it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'src> {
    pub kind: SyntaxKind,
    pub text: &'src str,
}

/// Errors produced while lexing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LexError {
    /// The lexer is not implemented yet.
    Unimplemented,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LexError::Unimplemented => f.write_str("lexer not implemented"),
        }
    }
}

impl std::error::Error for LexError {}

/// Lex `input` into a token stream.
pub fn lex(_input: &str) -> Result<Vec<Token<'_>>, LexError> {
    Err(LexError::Unimplemented)
}
