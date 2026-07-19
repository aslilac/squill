//! Event-stream tree building (rust-analyzer pattern).
//!
//! The parser emits [`Event`]s describing tree structure over the
//! *non-trivia* tokens only; [`build_tree`] consumes the events plus the
//! raw (trivia-included) token stream and builds the cstree green tree.
//! All trivia attachment policy lives here and only here:
//!
//! - Trailing trivia runs to end-of-line: when a node finishes (or a
//!   sibling starts) directly after a token, following trivia tokens stay
//!   attached at the current position until the first one containing a
//!   newline.
//! - Trivia after a newline is leading trivia of the next token, so
//!   statement-separating comments attach to the following statement.
//! - The root node absorbs any trivia remaining at EOF.

use crate::lexer::Token;
use crate::syntax::SyntaxKind;
use crate::syntax::SyntaxNode;
use cstree::build::GreenNodeBuilder;

/// Structure events emitted by the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
	/// Open a node of the given kind.
	StartNode(SyntaxKind),
	/// Emit the next non-trivia token from the raw token stream.
	Token,
	/// Close the most recently opened node.
	FinishNode,
}

/// A concrete syntax tree for one SQL file.
///
/// Lossless: [`Cst::text`] reproduces the parsed input byte-for-byte.
#[derive(Debug)]
pub struct Cst {
	root: SyntaxNode,
}

impl Cst {
	/// The `Root` syntax node.
	pub fn root(&self) -> &SyntaxNode {
		&self.root
	}

	/// The full source text of the tree.
	pub fn text(&self) -> String {
		self.root.to_string()
	}
}

/// Build a green tree from the raw token stream and the parser's events.
///
/// The events must describe exactly the non-trivia tokens of `tokens`, in
/// order, wrapped in one root node.
pub fn build_tree(tokens: &[Token<'_>], events: &[Event]) -> Cst {
	let mut sink =
		Sink { tokens, cursor: 0, builder: GreenNodeBuilder::new(), depth: 0 };
	for &event in events {
		sink.event(event);
	}
	assert_eq!(sink.depth, 0, "unbalanced StartNode/FinishNode events");
	assert_eq!(
		sink.cursor,
		tokens.len(),
		"events did not consume the whole token stream"
	);
	let (green, cache) = sink.builder.finish();
	let interner = cache
		.expect("builder owns its cache")
		.into_interner()
		.expect("cache owns its interner");
	Cst { root: SyntaxNode::new_root_with_resolver(green, interner) }
}

struct Sink<'src, 'tok> {
	tokens: &'tok [Token<'src>],
	/// Index of the next raw token not yet added to the tree.
	cursor: usize,
	builder: GreenNodeBuilder<'static, 'static, SyntaxKind>,
	depth: usize,
}

impl Sink<'_, '_> {
	fn event(&mut self, event: Event) {
		match event {
			Event::StartNode(kind) => {
				self.attach_same_line_trivia();
				self.builder.start_node(kind);
				self.depth += 1;
			}
			Event::Token => {
				self.flush_trivia();
				let token = self.tokens[self.cursor];
				debug_assert!(!token.kind.is_trivia(), "Token event on trivia");
				self.builder.token(token.kind, token.text);
				self.cursor += 1;
			}
			Event::FinishNode => {
				self.depth -= 1;
				if self.depth == 0 {
					// The root absorbs whatever trivia remains at EOF.
					self.flush_trivia();
				} else {
					self.attach_same_line_trivia();
				}
				self.builder.finish_node();
			}
		}
	}

	/// Emit all pending trivia at the current position (leading trivia of
	/// the next token).
	fn flush_trivia(&mut self) {
		while let Some(token) = self.tokens.get(self.cursor)
			&& token.kind.is_trivia()
		{
			self.builder.token(token.kind, token.text);
			self.cursor += 1;
		}
	}

	/// Emit pending trivia that trails the previous token on its own line:
	/// only when the previous raw token is non-trivia (i.e. we are directly
	/// after real code), and only up to the first trivia token containing a
	/// newline. Everything after the newline is leading trivia of whatever
	/// comes next.
	fn attach_same_line_trivia(&mut self) {
		if self.cursor == 0 || self.tokens[self.cursor - 1].kind.is_trivia() {
			return;
		}
		while let Some(token) = self.tokens.get(self.cursor)
			&& token.kind.is_trivia()
			&& !token.text.contains('\n')
		{
			self.builder.token(token.kind, token.text);
			self.cursor += 1;
		}
	}
}
