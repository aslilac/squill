//! Document IR the formatter lowers the CST into before rendering
//! (Wadler/Prettier style, modeled on `ruff_formatter`).

/// Syntactic position of an identifier, for the quoting safety rules.
///
/// Postgres `col_name` keywords may appear bare as column/table names but
/// not as type/function names; `type_func_name` keywords the reverse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentPos {
    ColumnOrTable,
    TypeOrFunction,
}

/// A layout document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Doc {
    /// Literal text. Must not contain newlines (use `verbatim` or line
    /// primitives) — rendered exactly as given.
    Text(String),
    /// A SQL keyword; case-normalized at render time per
    /// [`crate::KeywordCase`]. Never used for identifiers, strings, or
    /// comments.
    Keyword(String),
    /// An identifier; the quoting transform is applied at render time per
    /// [`crate::IdentQuoting`] and the dialect's safety rules.
    Ident { text: String, pos: IdentPos },
    /// Byte-exact passthrough for ErrorStatements and other opaque
    /// regions. May contain newlines; forces enclosing groups to break.
    Verbatim(String),
    /// A sequence of documents.
    Concat(Vec<Doc>),
    /// Lay the contents on one line if they fit; otherwise break the soft
    /// lines directly inside this group.
    Group(Box<Doc>),
    /// Increase indentation by one level for the contents.
    Indent(Box<Doc>),
    /// Nothing when flat; a line break when the enclosing group breaks.
    SoftLine,
    /// A space when flat; a line break when the enclosing group breaks.
    SoftLineOrSpace,
    /// Always a line break; forces enclosing groups to break.
    HardLine,
    /// `broken` when the enclosing group breaks, `flat` otherwise.
    IfBreak { broken: Box<Doc>, flat: Box<Doc> },
    /// Alternating content and separator documents; separators break
    /// individually, only where the next content would not fit.
    Fill(Vec<Doc>),
}

pub fn text(text: impl Into<String>) -> Doc {
    let text = text.into();
    debug_assert!(!text.contains('\n'), "use verbatim for multi-line text");
    Doc::Text(text)
}

pub fn keyword(keyword: impl Into<String>) -> Doc {
    Doc::Keyword(keyword.into())
}

pub fn ident(text: impl Into<String>, pos: IdentPos) -> Doc {
    Doc::Ident {
        text: text.into(),
        pos,
    }
}

pub fn verbatim(text: impl Into<String>) -> Doc {
    Doc::Verbatim(text.into())
}

pub fn concat(items: impl IntoIterator<Item = Doc>) -> Doc {
    Doc::Concat(items.into_iter().collect())
}

pub fn nil() -> Doc {
    Doc::Concat(Vec::new())
}

pub fn space() -> Doc {
    Doc::Text(" ".to_string())
}

pub fn group(doc: Doc) -> Doc {
    Doc::Group(Box::new(doc))
}

pub fn indent(doc: Doc) -> Doc {
    Doc::Indent(Box::new(doc))
}

pub fn soft_line() -> Doc {
    Doc::SoftLine
}

pub fn soft_line_or_space() -> Doc {
    Doc::SoftLineOrSpace
}

pub fn hard_line() -> Doc {
    Doc::HardLine
}

pub fn if_break(broken: Doc, flat: Doc) -> Doc {
    Doc::IfBreak {
        broken: Box::new(broken),
        flat: Box::new(flat),
    }
}

pub fn fill(items: impl IntoIterator<Item = Doc>) -> Doc {
    Doc::Fill(items.into_iter().collect())
}
