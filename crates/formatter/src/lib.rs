//! Formatter: doc IR, renderer (TREE-96), and the SELECT formatting
//! rules with their per-statement safety check (TREE-97).

pub mod check;
pub mod doc;
pub mod keywords;
mod printer;
mod quoting;
mod rules;

use parser::Dialect;
use parser::lexer::LexOptions;
use parser::parser::Cst;
use parser::syntax::SyntaxKind;

pub use doc::{Doc, IdentPos};

/// Maximum line width. Fixed; not part of the config surface.
pub const MAX_WIDTH: usize = 80;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndentStyle {
    #[default]
    Tab,
    Spaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeywordCase {
    #[default]
    Lower,
    Upper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IdentQuoting {
    #[default]
    UnquotedWhenSafe,
    AlwaysQuoted,
}

/// The complete configuration surface of the renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Tabs (default) or spaces.
    pub indent_style: IndentStyle,
    /// Width of one indent level: the space count in spaces mode, and the
    /// measured width of a tab in tab mode. Default 2.
    pub indent_width: u8,
    pub keyword_case: KeywordCase,
    pub quoting: IdentQuoting,
    /// Governs the identifier-quoting safety rules.
    pub dialect: Dialect,
    /// sqlc-style `@name` parameters (see [`LexOptions::at_params`]);
    /// used when re-lexing for the safety check.
    pub at_params: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            indent_style: IndentStyle::default(),
            indent_width: 2,
            keyword_case: KeywordCase::default(),
            quoting: IdentQuoting::default(),
            dialect: Dialect::default(),
            at_params: false,
        }
    }
}

impl Options {
    pub fn lex_options(&self) -> LexOptions {
        LexOptions {
            at_params: self.at_params,
        }
    }
}

/// Render a document to text.
pub fn render(doc: &Doc, options: &Options) -> String {
    printer::render(doc, options)
}

/// The result of formatting a CST.
#[derive(Debug)]
pub struct Formatted {
    pub text: String,
    /// Statements the rules could format but whose output failed the
    /// safety check and fell back to verbatim passthrough. Zero for a
    /// fully formatted file. ErrorStatements are always verbatim and are
    /// not counted here.
    pub fallback_statements: usize,
}

/// Format a CST. Statement-by-statement: each formatted statement is
/// re-lexed and compared against its input (token equivalence + comment
/// conservation); on any mismatch the original text passes through
/// verbatim, so output is never less correct than its input.
pub fn format_cst(cst: &Cst, options: &Options) -> Formatted {
    let lex_options = options.lex_options();
    let mut pieces: Vec<(bool, String)> = Vec::new();
    let mut fallbacks = 0;
    let mut pending_blank = false;

    for element in cst.root().children_with_tokens() {
        match element {
            parser::syntax::SyntaxElement::Node(node) => {
                let original = node.to_string();
                let blank = pending_blank || leading_blank(&original);
                pending_blank = false;
                match rules::lower_statement(node) {
                    Some(doc) => {
                        let rendered = render(&doc, options);
                        let safe = check::tokens_equivalent(
                            &original,
                            &rendered,
                            options.dialect,
                            lex_options,
                        ) && check::comments_conserved(
                            &original,
                            &rendered,
                            options.dialect,
                            lex_options,
                        );
                        if safe {
                            // Statement assembly owns inter-statement
                            // newlines; drop any the doc produced (e.g. a
                            // trailing comment's fresh line).
                            pieces.push((blank, rendered.trim_end().to_string()));
                        } else {
                            fallbacks += 1;
                            if std::env::var_os("SQUILL_DEBUG").is_some() {
                                eprintln!(
                                    "== fallback ==\n-- original --\n{original}\n-- rendered --\n{rendered}\n=="
                                );
                            }
                            pieces.push((blank, trim_verbatim(&original)));
                        }
                    }
                    None => pieces.push((blank, trim_verbatim(&original))),
                }
            }
            parser::syntax::SyntaxElement::Token(token) => match token.kind() {
                SyntaxKind::Whitespace => {
                    if token.text().matches('\n').count() >= 2 {
                        pending_blank = true;
                    }
                }
                SyntaxKind::LineComment | SyntaxKind::BlockComment => {
                    pieces.push((pending_blank, token.text().to_string()));
                    pending_blank = false;
                }
                _ => {
                    // Stray root-level tokens (shouldn't happen): keep.
                    pieces.push((pending_blank, token.text().to_string()));
                    pending_blank = false;
                }
            },
        }
    }

    let mut out = String::new();
    for (index, (blank, piece)) in pieces.iter().enumerate() {
        if index > 0 {
            out.push('\n');
            if *blank {
                out.push('\n');
            }
        }
        out.push_str(piece);
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    Formatted {
        text: out,
        fallback_statements: fallbacks,
    }
}

/// Does the statement's own text begin with a blank line (before any
/// comment or code)?
fn leading_blank(original: &str) -> bool {
    let leading: String = original.chars().take_while(|c| c.is_whitespace()).collect();
    leading.matches('\n').count() >= 2
}

/// Verbatim passthrough of a statement, trimmed of the surrounding
/// whitespace that statement assembly regenerates.
fn trim_verbatim(original: &str) -> String {
    original.trim().to_string()
}

/// Dev-tool access to the statement lowering (see examples/).
#[doc(hidden)]
pub fn debug_lower(node: &parser::syntax::SyntaxNode) -> Option<Doc> {
    rules::lower_statement(node)
}
