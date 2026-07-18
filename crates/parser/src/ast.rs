//! Typed AST accessors over the raw syntax tree, generated from
//! `syntax.def` so formatter code never string-matches node kinds.

use crate::syntax::{SyntaxKind, SyntaxNode, SyntaxToken};

/// A typed wrapper around a syntax node of one known kind.
pub trait AstNode<'a>: Copy {
    /// Wrap `syntax` if it has this type's kind.
    fn cast(syntax: &'a SyntaxNode) -> Option<Self>;
    /// The underlying syntax node.
    fn syntax(&self) -> &'a SyntaxNode;
}

include!(concat!(env!("OUT_DIR"), "/ast.rs"));
