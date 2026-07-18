//! Syntax kinds shared by the lexer, parser, and CST.

use cstree::Syntax;

/// Every kind of token and node that can appear in the CST.
///
/// Placeholder set — the real grammar lands with later issues.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Syntax)]
#[repr(u32)]
#[non_exhaustive]
pub enum SyntaxKind {
    /// Unrecognized or erroneous input.
    Error,
    /// Root node of a parsed file.
    Root,
}
