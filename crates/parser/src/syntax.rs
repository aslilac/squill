//! Syntax kinds shared by the lexer, parser, and CST.
//!
//! Token kinds are hand-written for now; TREE-94 replaces this with a
//! codegen'd definition covering node kinds too.

use cstree::Syntax;

/// Every kind of token and node that can appear in the CST.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Syntax)]
#[repr(u32)]
#[non_exhaustive]
pub enum SyntaxKind {
    /// Unrecognized input, or an unterminated string/comment spanning to EOF.
    Error,
    /// Root node of a parsed file.
    Root,

    // Trivia
    /// A run of whitespace.
    Whitespace,
    /// `-- ...` to end of line (exclusive).
    LineComment,
    /// `/* ... */`; nests in Postgres, not in SQLite.
    BlockComment,

    // Identifiers and literals
    /// Bare identifier or keyword (keywords are resolved by the parser).
    Ident,
    /// Delimited identifier: `"..."`, or SQLite `` `...` `` / `[...]`.
    QuotedIdent,
    /// Standard string: `'...'` with doubled-quote escape.
    String,
    /// Postgres escape string: `E'...'` with backslash escapes.
    EscapeString,
    /// Postgres Unicode string: `U&'...'`.
    UnicodeString,
    /// Postgres bit string: `B'...'`.
    BitString,
    /// Hex string / blob literal: `X'...'`.
    HexString,
    /// Postgres dollar-quoted string: `$tag$...$tag$`.
    DollarString,
    /// Numeric literal.
    Number,
    /// Bind parameter: `$1` (PG); `?`, `?NNN`, `:name`, `@name`, `$name` (SQLite).
    Param,

    // Punctuation and operators
    LParen,
    RParen,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Dot,
    Colon,
    /// The `::` cast operator (Postgres).
    ColonColon,
    /// Any other operator, including Postgres custom operators.
    Operator,
}
