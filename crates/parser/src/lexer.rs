//! Lossless dual-dialect lexer: source text to a flat token stream.
//!
//! Everything is a token, including whitespace and comments; concatenating
//! all token texts reproduces the input byte-for-byte. The lexer never
//! fails: unknown bytes become [`SyntaxKind::Error`] tokens, and
//! unterminated strings or comments become error tokens spanning to EOF.
//!
//! Hand-written state machine; no regex anywhere in the dependency tree.

use crate::dialect::Dialect;
use crate::syntax::SyntaxKind;

/// A single lexed token: its kind plus the source text it covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token<'src> {
    pub kind: SyntaxKind,
    pub text: &'src str,
}

/// Lex `input` into a token stream. Infallible and lossless.
pub fn lex(input: &str, dialect: Dialect) -> Vec<Token<'_>> {
    let mut lexer = Lexer {
        input,
        bytes: input.as_bytes(),
        dialect,
        pos: 0,
    };
    let mut tokens = Vec::new();
    while lexer.pos < lexer.bytes.len() {
        let start = lexer.pos;
        let kind = lexer.next_token();
        debug_assert!(lexer.pos > start, "lexer must always make progress");
        tokens.push(Token {
            kind,
            text: &input[start..lexer.pos],
        });
    }
    tokens
}

/// Operator characters in Postgres (`scan.l`'s `op_chars`).
const PG_OP_BYTES: &[u8] = b"+-*/<>=~!@#%^&|`?";

/// Characters that license a Postgres operator to end in `+` or `-`.
const PG_OP_SPECIAL: &[u8] = b"~!@#%^&|`?";

/// SQLite operators, longest first so prefix matching is maximal-munch.
const SQLITE_OPS: &[&str] = &[
    "->>", "->", "||", "<<", ">>", "<=", ">=", "==", "!=", "<>", "<", ">", "=", "+", "-", "*", "/",
    "%", "&", "|", "~",
];

fn is_pg_op_byte(b: u8) -> bool {
    PG_OP_BYTES.contains(&b)
}

struct Lexer<'src> {
    input: &'src str,
    bytes: &'src [u8],
    dialect: Dialect,
    pos: usize,
}

impl Lexer<'_> {
    fn at(&self, offset: usize) -> Option<u8> {
        self.bytes.get(self.pos + offset).copied()
    }

    /// Advance over `n` bytes (ASCII only — never splits a UTF-8 char).
    fn bump(&mut self, n: usize) {
        self.pos += n;
    }

    /// Advance over one full char, however many bytes it takes.
    fn bump_char(&mut self) {
        let c = self.input[self.pos..].chars().next().expect("in bounds");
        self.pos += c.len_utf8();
    }

    /// Is the byte at `offset` an identifier start? Both dialects treat all
    /// non-ASCII bytes as identifier characters, mirroring their scanners.
    fn is_ident_start_at(&self, offset: usize) -> bool {
        self.at(offset)
            .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_' || b >= 0x80)
    }

    /// Is the byte at `offset` an identifier continuation? Both dialects
    /// also allow `$` and digits inside identifiers.
    fn is_ident_cont_at(&self, offset: usize) -> bool {
        self.at(offset)
            .is_some_and(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'$' || b >= 0x80)
    }

    fn eat_ident(&mut self) {
        while let Some(b) = self.at(0) {
            match b {
                _ if b.is_ascii_alphanumeric() || b == b'_' || b == b'$' => self.bump(1),
                _ if b >= 0x80 => self.bump_char(),
                _ => break,
            }
        }
    }

    fn eat_digits(&mut self) {
        while self.at(0).is_some_and(|b| b.is_ascii_digit() || b == b'_') {
            self.bump(1);
        }
    }

    fn next_token(&mut self) -> SyntaxKind {
        let b = self.at(0).expect("in bounds");
        match b {
            b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c => {
                while matches!(self.at(0), Some(b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)) {
                    self.bump(1);
                }
                SyntaxKind::Whitespace
            }
            b'-' if self.at(1) == Some(b'-') => self.line_comment(),
            b'/' if self.at(1) == Some(b'*') => self.block_comment(),
            b'\'' => self.single_quoted(false, SyntaxKind::String),
            b'"' => self.delimited_ident(b'"'),
            b'e' | b'E' if self.dialect == Dialect::Postgres && self.at(1) == Some(b'\'') => {
                self.bump(1);
                self.single_quoted(true, SyntaxKind::EscapeString)
            }
            b'b' | b'B' if self.dialect == Dialect::Postgres && self.at(1) == Some(b'\'') => {
                self.bump(1);
                self.single_quoted(false, SyntaxKind::BitString)
            }
            b'x' | b'X' if self.at(1) == Some(b'\'') => {
                self.bump(1);
                self.single_quoted(false, SyntaxKind::HexString)
            }
            b'u' | b'U'
                if self.dialect == Dialect::Postgres
                    && self.at(1) == Some(b'&')
                    && self.at(2) == Some(b'\'') =>
            {
                self.bump(2);
                self.single_quoted(false, SyntaxKind::UnicodeString)
            }
            b'`' => match self.dialect {
                Dialect::Sqlite => self.delimited_ident(b'`'),
                Dialect::Postgres => self.pg_operator(),
            },
            b'[' => match self.dialect {
                Dialect::Postgres => {
                    self.bump(1);
                    SyntaxKind::LBracket
                }
                Dialect::Sqlite => self.bracket_ident(),
            },
            b']' => {
                self.bump(1);
                match self.dialect {
                    Dialect::Postgres => SyntaxKind::RBracket,
                    // A `]` outside a bracket identifier is not SQLite syntax.
                    Dialect::Sqlite => SyntaxKind::Error,
                }
            }
            b'(' => {
                self.bump(1);
                SyntaxKind::LParen
            }
            b')' => {
                self.bump(1);
                SyntaxKind::RParen
            }
            b',' => {
                self.bump(1);
                SyntaxKind::Comma
            }
            b';' => {
                self.bump(1);
                SyntaxKind::Semicolon
            }
            b'.' if self.at(1).is_some_and(|b| b.is_ascii_digit()) => self.number(),
            b'.' => {
                self.bump(1);
                SyntaxKind::Dot
            }
            b'0'..=b'9' => self.number(),
            b':' if self.dialect == Dialect::Postgres && self.at(1) == Some(b':') => {
                self.bump(2);
                SyntaxKind::ColonColon
            }
            b':' if self.dialect == Dialect::Sqlite && self.is_ident_start_at(1) => {
                self.bump(1);
                self.eat_ident();
                SyntaxKind::Param
            }
            b':' => {
                self.bump(1);
                SyntaxKind::Colon
            }
            b'$' => self.dollar(),
            b'?' if self.dialect == Dialect::Sqlite => {
                self.bump(1);
                while self.at(0).is_some_and(|b| b.is_ascii_digit()) {
                    self.bump(1);
                }
                SyntaxKind::Param
            }
            b'@' if self.dialect == Dialect::Sqlite => {
                self.bump(1);
                if self.is_ident_cont_at(0) {
                    self.eat_ident();
                    SyntaxKind::Param
                } else {
                    SyntaxKind::Error
                }
            }
            _ if self.dialect == Dialect::Postgres && is_pg_op_byte(b) => self.pg_operator(),
            // `_` is ASCII punctuation but starts an identifier.
            _ if self.dialect == Dialect::Sqlite && b.is_ascii_punctuation() && b != b'_' => {
                self.sqlite_operator()
            }
            _ if self.is_ident_start_at(0) => {
                self.eat_ident();
                SyntaxKind::Ident
            }
            _ => {
                self.bump_char();
                SyntaxKind::Error
            }
        }
    }

    fn line_comment(&mut self) -> SyntaxKind {
        self.bump(2);
        while let Some(b) = self.at(0) {
            match b {
                b'\n' | b'\r' => break,
                _ if b < 0x80 => self.bump(1),
                _ => self.bump_char(),
            }
        }
        SyntaxKind::LineComment
    }

    fn block_comment(&mut self) -> SyntaxKind {
        self.bump(2);
        let mut depth = 1u32;
        while let Some(b) = self.at(0) {
            if b == b'*' && self.at(1) == Some(b'/') {
                self.bump(2);
                depth -= 1;
                if depth == 0 {
                    return SyntaxKind::BlockComment;
                }
            } else if b == b'/' && self.at(1) == Some(b'*') && self.dialect == Dialect::Postgres {
                // Postgres block comments nest; SQLite's do not.
                self.bump(2);
                depth += 1;
            } else if b < 0x80 {
                self.bump(1);
            } else {
                self.bump_char();
            }
        }
        SyntaxKind::Error
    }

    /// Scan a `'...'`-style literal with the opening quote at `pos`
    /// (any prefix already consumed). `''` never terminates; with
    /// `backslash_escapes`, `\` skips the next char, so `\'` doesn't either.
    fn single_quoted(&mut self, backslash_escapes: bool, kind: SyntaxKind) -> SyntaxKind {
        self.bump(1);
        loop {
            match self.at(0) {
                None => return SyntaxKind::Error,
                Some(b'\'') => {
                    if self.at(1) == Some(b'\'') {
                        self.bump(2);
                    } else {
                        self.bump(1);
                        return kind;
                    }
                }
                Some(b'\\') if backslash_escapes => {
                    self.bump(1);
                    if self.at(0).is_some() {
                        self.bump_char();
                    }
                }
                Some(b) if b < 0x80 => self.bump(1),
                Some(_) => self.bump_char(),
            }
        }
    }

    /// Scan a delimited identifier (`"..."` or SQLite backticks), where a
    /// doubled delimiter is an escape.
    fn delimited_ident(&mut self, delim: u8) -> SyntaxKind {
        self.bump(1);
        loop {
            match self.at(0) {
                None => return SyntaxKind::Error,
                Some(b) if b == delim => {
                    if self.at(1) == Some(delim) {
                        self.bump(2);
                    } else {
                        self.bump(1);
                        return SyntaxKind::QuotedIdent;
                    }
                }
                Some(b) if b < 0x80 => self.bump(1),
                Some(_) => self.bump_char(),
            }
        }
    }

    /// SQLite `[bracket]` identifier; `]` cannot be escaped.
    fn bracket_ident(&mut self) -> SyntaxKind {
        self.bump(1);
        loop {
            match self.at(0) {
                None => return SyntaxKind::Error,
                Some(b']') => {
                    self.bump(1);
                    return SyntaxKind::QuotedIdent;
                }
                Some(b) if b < 0x80 => self.bump(1),
                Some(_) => self.bump_char(),
            }
        }
    }

    fn number(&mut self) -> SyntaxKind {
        // Radix-prefixed integers: 0x (both dialects), 0o/0b (Postgres 16+).
        if self.at(0) == Some(b'0') {
            let radix = match self.at(1) {
                Some(b'x' | b'X') => Some(16),
                Some(b'o' | b'O') if self.dialect == Dialect::Postgres => Some(8),
                Some(b'b' | b'B') if self.dialect == Dialect::Postgres => Some(2),
                _ => None,
            };
            if let Some(radix) = radix
                && self.at(2).is_some_and(|b| (b as char).is_digit(radix))
            {
                self.bump(2);
                while self
                    .at(0)
                    .is_some_and(|b| (b as char).is_digit(radix) || b == b'_')
                {
                    self.bump(1);
                }
                return SyntaxKind::Number;
            }
        }
        self.eat_digits();
        if self.at(0) == Some(b'.') && self.at(1) != Some(b'.') {
            self.bump(1);
            self.eat_digits();
        }
        // Exponent, only if it is actually one — `1e` is Number then Ident.
        if matches!(self.at(0), Some(b'e' | b'E')) {
            let digit_at = match self.at(1) {
                Some(b'+' | b'-') => 2,
                _ => 1,
            };
            if self.at(digit_at).is_some_and(|b| b.is_ascii_digit()) {
                self.bump(digit_at);
                self.eat_digits();
            }
        }
        SyntaxKind::Number
    }

    /// `$` dispatch: positional params and dollar-quoted strings (Postgres),
    /// `$name` params (SQLite).
    fn dollar(&mut self) -> SyntaxKind {
        match self.dialect {
            Dialect::Postgres => {
                if self.at(1).is_some_and(|b| b.is_ascii_digit()) {
                    self.bump(1);
                    while self.at(0).is_some_and(|b| b.is_ascii_digit()) {
                        self.bump(1);
                    }
                    return SyntaxKind::Param;
                }
                // Try a dollar-quote opener `$tag$` (tag may be empty and
                // may not start with a digit — handled by the param case).
                let mut end = self.pos + 1;
                loop {
                    match self.bytes.get(end) {
                        Some(b'$') => break,
                        Some(&b) if b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80 => {
                            end += 1;
                        }
                        // Lone `$` is not Postgres syntax.
                        _ => {
                            self.bump(1);
                            return SyntaxKind::Error;
                        }
                    }
                }
                let delim = &self.input[self.pos..=end];
                let body_start = end + 1;
                match self.input[body_start..].find(delim) {
                    Some(offset) => {
                        self.pos = body_start + offset + delim.len();
                        SyntaxKind::DollarString
                    }
                    None => {
                        self.pos = self.input.len();
                        SyntaxKind::Error
                    }
                }
            }
            Dialect::Sqlite => {
                self.bump(1);
                if self.is_ident_cont_at(0) {
                    self.eat_ident();
                    SyntaxKind::Param
                } else {
                    SyntaxKind::Error
                }
            }
        }
    }

    /// Maximal munch over Postgres operator characters, with two scanner
    /// rules from `scan.l`: comment openers terminate the munch, and an
    /// operator may only end in `+`/`-` if it contains one of
    /// `~ ! @ # % ^ & | ` ?`.
    fn pg_operator(&mut self) -> SyntaxKind {
        let start = self.pos;
        while let Some(b) = self.at(0) {
            if !is_pg_op_byte(b) {
                break;
            }
            if (b == b'-' && self.at(1) == Some(b'-')) || (b == b'/' && self.at(1) == Some(b'*')) {
                break;
            }
            self.bump(1);
        }
        let text = &self.bytes[start..self.pos];
        if !text.iter().any(|b| PG_OP_SPECIAL.contains(b)) {
            let mut len = text.len();
            while len > 1 && matches!(text[len - 1], b'+' | b'-') {
                len -= 1;
            }
            self.pos = start + len;
        }
        SyntaxKind::Operator
    }

    /// Longest-prefix match against SQLite's fixed operator set.
    fn sqlite_operator(&mut self) -> SyntaxKind {
        let rest = &self.input[self.pos..];
        for op in SQLITE_OPS {
            if rest.starts_with(op) {
                self.bump(op.len());
                return SyntaxKind::Operator;
            }
        }
        self.bump_char();
        SyntaxKind::Error
    }
}
