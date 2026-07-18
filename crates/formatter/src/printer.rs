//! The renderer: doc IR to text, deciding where groups break.
//!
//! Iterative Prettier-style printer. `fits` measures a candidate flat
//! layout *plus the rest of the current line* (the pending command
//! stack), so trailing text like a `,` after a group is accounted for.

use crate::doc::Doc;
use crate::quoting;
use crate::{IndentStyle, KeywordCase, MAX_WIDTH, Options};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Flat,
    Break,
}

#[derive(Clone, Copy)]
struct Cmd<'a> {
    indent: u16,
    mode: Mode,
    work: Work<'a>,
}

#[derive(Clone, Copy)]
enum Work<'a> {
    Doc(&'a Doc),
    /// The remaining items of a fill: `[separator, content, rest...]`.
    FillRest(&'a [Doc]),
    /// A fill content item whose mode is decided when it is reached.
    FillContent(&'a Doc),
}

pub(crate) fn render(doc: &Doc, options: &Options) -> String {
    let mut printer = Printer {
        options,
        out: String::new(),
        col: 0,
        at_line_start: true,
    };
    let mut stack = vec![Cmd {
        indent: 0,
        mode: Mode::Break,
        work: Work::Doc(doc),
    }];
    while let Some(cmd) = stack.pop() {
        printer.step(cmd, &mut stack);
    }
    printer.out
}

struct Printer<'o> {
    options: &'o Options,
    out: String,
    col: usize,
    /// Is the output at the start of a (possibly indented) line?
    at_line_start: bool,
}

impl Printer<'_> {
    fn step<'a>(&mut self, cmd: Cmd<'a>, stack: &mut Vec<Cmd<'a>>) {
        let Cmd { indent, mode, work } = cmd;
        let doc = match work {
            Work::Doc(doc) => doc,
            Work::FillRest(items) => {
                self.fill_rest(items, indent, stack);
                return;
            }
            Work::FillContent(doc) => {
                let content_mode = if self.fits(
                    Cmd {
                        indent,
                        mode: Mode::Flat,
                        work: Work::Doc(doc),
                    },
                    stack,
                ) {
                    Mode::Flat
                } else {
                    Mode::Break
                };
                stack.push(Cmd {
                    indent,
                    mode: content_mode,
                    work: Work::Doc(doc),
                });
                return;
            }
        };
        match doc {
            Doc::Text(text) => self.push_text(text),
            Doc::Keyword(keyword) => {
                let cased = match self.options.keyword_case {
                    KeywordCase::Lower => keyword.to_ascii_lowercase(),
                    KeywordCase::Upper => keyword.to_ascii_uppercase(),
                };
                self.push_text(&cased);
            }
            Doc::Ident { text, pos } => {
                let rendered = quoting::render_ident(text, *pos, self.options);
                self.push_text(&rendered);
            }
            Doc::Verbatim(text) => self.push_verbatim(text),
            Doc::Concat(items) => {
                stack.extend(items.iter().rev().map(|doc| Cmd {
                    indent,
                    mode,
                    work: Work::Doc(doc),
                }));
            }
            Doc::Group(inner) => {
                let inner_mode = match mode {
                    Mode::Flat => Mode::Flat,
                    Mode::Break => {
                        if self.fits(
                            Cmd {
                                indent,
                                mode: Mode::Flat,
                                work: Work::Doc(inner),
                            },
                            stack,
                        ) {
                            Mode::Flat
                        } else {
                            Mode::Break
                        }
                    }
                };
                stack.push(Cmd {
                    indent,
                    mode: inner_mode,
                    work: Work::Doc(inner),
                });
            }
            Doc::Indent(inner) => stack.push(Cmd {
                indent: indent + 1,
                mode,
                work: Work::Doc(inner),
            }),
            Doc::SoftLine => {
                if mode == Mode::Break {
                    self.newline(indent);
                }
            }
            Doc::SoftLineOrSpace => match mode {
                Mode::Flat => self.push_text(" "),
                Mode::Break => self.newline(indent),
            },
            Doc::HardLine => self.newline(indent),
            Doc::FreshLine => {
                if !self.at_line_start {
                    self.newline(indent);
                }
            }
            Doc::BreakParent => {}
            Doc::IfBreak { broken, flat } => {
                let chosen = match mode {
                    Mode::Break => broken,
                    Mode::Flat => flat,
                };
                stack.push(Cmd {
                    indent,
                    mode,
                    work: Work::Doc(chosen),
                });
            }
            Doc::Fill(items) => match items.split_first() {
                None => {}
                Some((first, rest)) => {
                    stack.push(Cmd {
                        indent,
                        mode,
                        work: Work::FillRest(rest),
                    });
                    stack.push(Cmd {
                        indent,
                        mode,
                        work: Work::FillContent(first),
                    });
                }
            },
        }
    }

    /// Handle `[separator, content, rest...]` of a fill: keep the pair on
    /// this line when it fits, otherwise break at the separator.
    fn fill_rest<'a>(&mut self, items: &'a [Doc], indent: u16, stack: &mut Vec<Cmd<'a>>) {
        let Some((separator, rest)) = items.split_first() else {
            return;
        };
        let Some((content, rest)) = rest.split_first() else {
            // Trailing separator without content: render as-is.
            stack.push(Cmd {
                indent,
                mode: Mode::Break,
                work: Work::Doc(separator),
            });
            return;
        };
        stack.push(Cmd {
            indent,
            mode: Mode::Break,
            work: Work::FillRest(rest),
        });
        let pair_fits = self.fits_pair(separator, content, indent, stack);
        if pair_fits {
            stack.push(Cmd {
                indent,
                mode: Mode::Flat,
                work: Work::Doc(content),
            });
            stack.push(Cmd {
                indent,
                mode: Mode::Flat,
                work: Work::Doc(separator),
            });
        } else {
            stack.push(Cmd {
                indent,
                mode: Mode::Break,
                work: Work::FillContent(content),
            });
            stack.push(Cmd {
                indent,
                mode: Mode::Break,
                work: Work::Doc(separator),
            });
        }
    }

    // ---- output ----

    fn push_text(&mut self, text: &str) {
        self.out.push_str(text);
        self.col += self.width(text);
        if !text.is_empty() {
            self.at_line_start = false;
        }
    }

    fn push_verbatim(&mut self, text: &str) {
        self.out.push_str(text);
        match text.rfind('\n') {
            Some(pos) => self.col = self.width(&text[pos + 1..]),
            None => self.col += self.width(text),
        }
        if !text.is_empty() {
            self.at_line_start = text.ends_with('\n');
        }
    }

    fn newline(&mut self, indent: u16) {
        // Never leave trailing whitespace on the line being ended.
        while self.out.ends_with(' ') || self.out.ends_with('\t') {
            self.out.pop();
        }
        self.out.push('\n');
        let width = usize::from(self.options.indent_width);
        match self.options.indent_style {
            IndentStyle::Tab => {
                for _ in 0..indent {
                    self.out.push('\t');
                }
            }
            IndentStyle::Spaces => {
                for _ in 0..usize::from(indent) * width {
                    self.out.push(' ');
                }
            }
        }
        self.col = usize::from(indent) * width;
        self.at_line_start = true;
    }

    /// Display width: tabs count as the configured tab width, every other
    /// char as one column.
    fn width(&self, text: &str) -> usize {
        let tab = usize::from(self.options.indent_width);
        text.chars().map(|c| if c == '\t' { tab } else { 1 }).sum()
    }

    // ---- measurement ----

    fn fits(&self, head: Cmd<'_>, stack: &[Cmd<'_>]) -> bool {
        self.fits_many(&[head], stack)
    }

    fn fits_pair(&self, separator: &Doc, content: &Doc, indent: u16, stack: &[Cmd<'_>]) -> bool {
        self.fits_many(
            &[
                Cmd {
                    indent,
                    mode: Mode::Flat,
                    work: Work::Doc(separator),
                },
                Cmd {
                    indent,
                    mode: Mode::Flat,
                    work: Work::Doc(content),
                },
            ],
            stack,
        )
    }

    /// Would `head` (usually flat) followed by the pending line content in
    /// `rest` fit on the current line? Measurement ends at the first line
    /// break in break-mode content (the line ends there anyway).
    fn fits_many(&self, head: &[Cmd<'_>], rest: &[Cmd<'_>]) -> bool {
        let mut remaining = MAX_WIDTH as isize - self.col as isize;
        // Work queue: `head` in order, then `rest` from its top (end).
        let mut queue: Vec<Cmd<'_>> = head.iter().rev().copied().collect();
        let mut rest_iter = rest.iter().rev();
        loop {
            let cmd = match queue.pop() {
                Some(cmd) => cmd,
                None => match rest_iter.next() {
                    Some(cmd) => *cmd,
                    None => return true,
                },
            };
            let Cmd { indent, mode, work } = cmd;
            let doc = match work {
                Work::Doc(doc) => doc,
                // Pending fill work in the line's tail: the upcoming
                // separator is a legal break point, so the line can end
                // here.
                Work::FillRest(_) | Work::FillContent(_) => return true,
            };
            match doc {
                Doc::Text(text) => remaining -= self.width(text) as isize,
                Doc::Keyword(keyword) => remaining -= self.width(keyword) as isize,
                Doc::Ident { text, pos } => {
                    let rendered = quoting::render_ident(text, *pos, self.options);
                    remaining -= self.width(&rendered) as isize;
                }
                Doc::Verbatim(text) => {
                    if text.contains('\n') {
                        // Multi-line verbatim can never sit on a flat
                        // line; in break-mode context the line simply
                        // ends here.
                        return mode == Mode::Break;
                    }
                    remaining -= self.width(text) as isize;
                }
                Doc::Concat(items) => {
                    queue.extend(items.iter().rev().map(|doc| Cmd {
                        indent,
                        mode,
                        work: Work::Doc(doc),
                    }));
                }
                Doc::Group(inner) | Doc::Indent(inner) => {
                    queue.push(Cmd {
                        indent,
                        mode,
                        work: Work::Doc(inner),
                    });
                }
                Doc::SoftLine => {
                    if mode == Mode::Break {
                        return true;
                    }
                }
                Doc::SoftLineOrSpace => match mode {
                    Mode::Flat => remaining -= 1,
                    Mode::Break => return true,
                },
                Doc::HardLine => {
                    return mode == Mode::Break;
                }
                Doc::BreakParent => {
                    // Zero-width, but a flat layout containing it is
                    // invalid: the enclosing group must break.
                    if mode == Mode::Flat {
                        return false;
                    }
                }
                Doc::FreshLine => {
                    return mode == Mode::Break;
                }
                Doc::IfBreak { broken, flat } => {
                    let chosen = match mode {
                        Mode::Break => broken,
                        Mode::Flat => flat,
                    };
                    queue.push(Cmd {
                        indent,
                        mode,
                        work: Work::Doc(chosen),
                    });
                }
                Doc::Fill(items) => {
                    queue.extend(items.iter().rev().map(|doc| Cmd {
                        indent,
                        mode: Mode::Flat,
                        work: Work::Doc(doc),
                    }));
                }
            }
            if remaining < 0 {
                return false;
            }
        }
    }
}
