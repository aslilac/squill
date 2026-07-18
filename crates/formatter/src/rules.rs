//! CST-to-doc lowering: the formatting rules for SELECT statements.
//!
//! Layout: canonical clause-per-line at indent 0 of the statement, with
//! contents grouped so short statements collapse to one line. Comments are
//! carried through leading/trailing slots — a comment on the same line as
//! preceding code trails it (line comments force the enclosing groups to
//! break via `BreakParent`); a comment after a newline leads the next
//! token. Comments are visited exactly once, in source order, so they can
//! never be dropped, duplicated, or reordered.

use parser::syntax::{SyntaxElement, SyntaxKind, SyntaxNode, SyntaxToken};

use crate::doc::{
    Doc, IdentPos, concat, fresh_line, group, hard_line, ident, if_break, indent, keyword, nil,
    soft_line, soft_line_or_space, space, text, verbatim,
};

/// Lower one statement node to a document. Returns `None` for kinds the
/// rules do not format (ErrorStatement — handled as verbatim upstream).
pub(crate) fn lower_statement(stmt: &SyntaxNode) -> Option<Doc> {
    match stmt.kind() {
        SyntaxKind::SelectStmt
        | SyntaxKind::EmptyStmt
        | SyntaxKind::InsertStmt
        | SyntaxKind::UpdateStmt
        | SyntaxKind::DeleteStmt
        | SyntaxKind::DdlStmt
        | SyntaxKind::PlBlock
        | SyntaxKind::PlIf
        | SyntaxKind::PlCase
        | SyntaxKind::PlLoop
        | SyntaxKind::PlWhile
        | SyntaxKind::PlFor
        | SyntaxKind::PlForeach
        | SyntaxKind::PlExit
        | SyntaxKind::PlReturn
        | SyntaxKind::PlRaise
        | SyntaxKind::PlAssign
        | SyntaxKind::PlPerform
        | SyntaxKind::PlExecute
        | SyntaxKind::PlGetDiag
        | SyntaxKind::PlNull => {
            // Statement-leading trivia (comments before the first token)
            // attaches at statement level, outside the body group, so a
            // commented short statement still collapses to one line.
            let mut header = Vec::new();
            let mut skip_trivia = 0;
            let mut pending_blank = false;
            let mut emitted = false;
            for token in stmt
                .descendants_with_tokens()
                .filter_map(|el| el.into_token())
            {
                if !token.kind().is_trivia() {
                    break;
                }
                skip_trivia += 1;
                match token.kind() {
                    SyntaxKind::Whitespace => {
                        if token.text().matches('\n').count() >= 2 {
                            pending_blank = true;
                        }
                    }
                    _ => {
                        if pending_blank && emitted {
                            header.push(hard_line());
                        }
                        pending_blank = false;
                        emitted = true;
                        if token.text().contains('\n') {
                            header.push(verbatim(token.text()));
                        } else {
                            header.push(text(token.text()));
                        }
                        header.push(hard_line());
                    }
                }
            }
            let mut lowerer = Lowerer {
                pending: Vec::new(),
                at_line_start: true,
                pending_blank: false,
                emitted_any: false,
                skip_trivia,
            };
            let mut docs = Vec::new();
            match stmt.kind() {
                SyntaxKind::SelectStmt | SyntaxKind::EmptyStmt => {
                    lowerer.statement_flow(&mut docs, stmt);
                }
                kind if is_pl_container(kind) => {
                    let doc = lowerer.node(stmt);
                    docs.push(doc);
                }
                SyntaxKind::DdlStmt => lowerer.ddl_statement(&mut docs, stmt),
                _ => lowerer.dml_flow(&mut docs, stmt),
            }
            lowerer.flush_pending(&mut docs);
            header.push(group(concat(docs)));
            Some(concat(header))
        }
        _ => None,
    }
}

struct PendingComment {
    doc: Doc,
    blank_before: bool,
}

struct Lowerer {
    /// Leading comments waiting for the next content push.
    pending: Vec<PendingComment>,
    /// Has a newline occurred in trivia since the last non-trivia token?
    at_line_start: bool,
    /// Was there a blank line (>= 2 newlines) since the last content?
    pending_blank: bool,
    emitted_any: bool,
    /// Statement-leading trivia already emitted by `lower_statement`.
    skip_trivia: usize,
}

impl Lowerer {
    // ---- comment machinery ----

    fn trivia(&mut self, docs: &mut Vec<Doc>, token: &SyntaxToken) {
        if self.skip_trivia > 0 {
            self.skip_trivia -= 1;
            return;
        }
        match token.kind() {
            SyntaxKind::Whitespace => {
                let newlines = token.text().matches('\n').count();
                if newlines > 0 {
                    self.at_line_start = true;
                }
                if newlines >= 2 {
                    self.pending_blank = true;
                }
            }
            SyntaxKind::LineComment => {
                if self.at_line_start {
                    self.buffer_leading(text(token.text()));
                } else {
                    docs.push(space());
                    docs.push(text(token.text()));
                    // A line comment swallows the rest of its line: the
                    // next content must start on a fresh line, even in
                    // positions with no structural separator.
                    docs.push(fresh_line());
                }
            }
            SyntaxKind::BlockComment => {
                let comment = if token.text().contains('\n') {
                    verbatim(token.text())
                } else {
                    text(token.text())
                };
                if self.at_line_start {
                    self.buffer_leading(comment);
                } else {
                    docs.push(space());
                    docs.push(comment);
                }
            }
            _ => unreachable!("trivia() called on non-trivia"),
        }
    }

    fn buffer_leading(&mut self, doc: Doc) {
        self.pending.push(PendingComment {
            doc,
            blank_before: self.pending_blank && (self.emitted_any || !self.pending.is_empty()),
        });
        self.pending_blank = false;
    }

    /// Push content, flushing any buffered leading comments first.
    fn push(&mut self, docs: &mut Vec<Doc>, doc: Doc) {
        self.flush_pending(docs);
        docs.push(doc);
        self.at_line_start = false;
        self.pending_blank = false;
        self.emitted_any = true;
    }

    fn flush_pending(&mut self, docs: &mut Vec<Doc>) {
        if self.pending.is_empty() {
            return;
        }
        // A separator space right before the comment's line break would
        // end the line with trailing whitespace: drop it.
        if matches!(docs.last(), Some(Doc::Text(t)) if t == " ") {
            docs.pop();
        }
        for comment in std::mem::take(&mut self.pending) {
            // A leading comment always begins on its own line, wherever
            // the flush happens; fresh_line is a no-op when a separator
            // already broke the line.
            docs.push(fresh_line());
            if comment.blank_before {
                docs.push(hard_line());
            }
            docs.push(comment.doc);
            docs.push(hard_line());
        }
    }

    // ---- dispatch ----

    fn node(&mut self, node: &SyntaxNode) -> Doc {
        match node.kind() {
            SyntaxKind::SelectCore => self.select_core(node),
            // PL/pgSQL containers and statements.
            SyntaxKind::PlBlock => self.pl_block(node),
            SyntaxKind::PlIf | SyntaxKind::PlElsif => self.pl_container(node, false),
            SyntaxKind::PlCase => self.pl_container(node, true),
            SyntaxKind::PlLoop
            | SyntaxKind::PlWhile
            | SyntaxKind::PlFor
            | SyntaxKind::PlForeach
            | SyntaxKind::PlWhen
            | SyntaxKind::PlElse => self.pl_container(node, false),
            SyntaxKind::PlException => self.pl_exception(node),
            SyntaxKind::PlDeclare => self.space_flow(node, IdentPos::ColumnOrTable),
            SyntaxKind::PlExit
            | SyntaxKind::PlReturn
            | SyntaxKind::PlRaise
            | SyntaxKind::PlAssign
            | SyntaxKind::PlPerform
            | SyntaxKind::PlExecute
            | SyntaxKind::PlGetDiag
            | SyntaxKind::PlNull => {
                let mut docs = Vec::new();
                self.dml_flow(&mut docs, node);
                group(concat(docs))
            }
            SyntaxKind::PlInto => self.kw_clause(node),
            // Nested error statements pass through verbatim.
            SyntaxKind::ErrorStatement => verbatim(node.to_string().trim().to_string()),
            // DML nested in CTE bodies.
            SyntaxKind::InsertStmt | SyntaxKind::UpdateStmt | SyntaxKind::DeleteStmt => {
                let mut docs = Vec::new();
                self.dml_flow(&mut docs, node);
                group(concat(docs))
            }
            SyntaxKind::DdlStmt => {
                let mut docs = Vec::new();
                self.ddl_statement(&mut docs, node);
                group(concat(docs))
            }
            SyntaxKind::SetClause
            | SyntaxKind::UsingClause
            | SyntaxKind::OnConflictClause
            | SyntaxKind::ReturningClause => self.kw_clause(node),
            SyntaxKind::SetItem | SyntaxKind::ColumnDef => {
                self.space_flow(node, IdentPos::ColumnOrTable)
            }
            SyntaxKind::ElementList => self.paren_block(node),
            SyntaxKind::SetOperation => {
                let mut docs = Vec::new();
                self.statement_flow(&mut docs, node);
                concat(docs)
            }
            SyntaxKind::WithClause => self.kw_clause(node),
            SyntaxKind::Cte => self.cte(node),
            SyntaxKind::SelectList => self.comma_list(node),
            SyntaxKind::SelectItem => self.select_item(node),
            SyntaxKind::FromClause => self.kw_clause(node),
            SyntaxKind::TableRef => self.table_ref(node),
            SyntaxKind::ParenTableRef => self.paren_block(node),
            SyntaxKind::JoinExpr => self.join_expr(node),
            SyntaxKind::JoinCondition => self.space_flow(node, IdentPos::ColumnOrTable),
            SyntaxKind::WhereClause
            | SyntaxKind::HavingClause
            | SyntaxKind::GroupByClause
            | SyntaxKind::OrderByClause
            | SyntaxKind::WindowClause
            | SyntaxKind::LimitClause
            | SyntaxKind::OffsetClause
            | SyntaxKind::FetchClause
            | SyntaxKind::ValuesClause => self.kw_clause(node),
            SyntaxKind::LockingClause
            | SyntaxKind::GroupingElement
            | SyntaxKind::FrameClause
            | SyntaxKind::OrderingTerm
            | SyntaxKind::SearchClause
            | SyntaxKind::CycleClause
            | SyntaxKind::WindowDef
            | SyntaxKind::TableCore => self.space_flow(node, IdentPos::ColumnOrTable),
            SyntaxKind::WindowSpec => self.paren_block(node),
            SyntaxKind::ParenSelect | SyntaxKind::SubqueryExpr => self.paren_block(node),
            SyntaxKind::Literal => self.space_flow(node, IdentPos::ColumnOrTable),
            SyntaxKind::ColumnRef => self.column_ref(node, IdentPos::ColumnOrTable),
            SyntaxKind::ParenExpr | SyntaxKind::RowExpr | SyntaxKind::ArrayExpr => {
                self.paren_block(node)
            }
            SyntaxKind::QuantifiedExpr | SyntaxKind::InExpr => self.tail_paren_expr(node),
            SyntaxKind::CaseExpr => self.case_expr(node),
            SyntaxKind::WhenClause => self.when_clause(node),
            SyntaxKind::FunctionCall => self.function_call(node),
            SyntaxKind::ArgList => self.paren_block(node),
            SyntaxKind::FilterClause | SyntaxKind::WithinGroupClause | SyntaxKind::OverClause => {
                self.space_flow(node, IdentPos::ColumnOrTable)
            }
            SyntaxKind::CastExpr => self.cast_expr(node),
            SyntaxKind::TypeName => self.type_name(node),
            SyntaxKind::PrefixExpr => self.prefix_expr(node),
            SyntaxKind::BinaryExpr => self.binary_expr(node),
            SyntaxKind::IsExpr | SyntaxKind::BetweenExpr => self.binary_expr(node),
            SyntaxKind::SubscriptExpr => self.tight_flow(node),
            _ => self.space_flow(node, IdentPos::ColumnOrTable),
        }
    }

    // ---- statement / clause layout ----

    /// Clause-level flow: child clause nodes separated by
    /// `soft_line_or_space` at the statement's indent; keyword tokens
    /// space-joined; `;` attached tight.
    fn statement_flow(&mut self, docs: &mut Vec<Doc>, node: &SyntaxNode) {
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(docs, token);
                }
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::Semicolon => {
                    self.push(docs, text(";"));
                }
                SyntaxElement::Token(token) => {
                    // Set-operation keywords (`union all`) sit directly in
                    // the flow, on their own line when broken.
                    if !first {
                        docs.push(soft_line_or_space());
                    }
                    self.push(docs, token_leaf(token));
                    first = false;
                    // Keyword runs: following keywords join with a space.
                    while false {}
                }
                SyntaxElement::Node(child) => {
                    if !first {
                        docs.push(soft_line_or_space());
                    }
                    let doc = self.node(child);
                    self.push(docs, doc);
                    first = false;
                }
            }
        }
    }

    /// `SELECT [ALL|DISTINCT [ON (...)]] list [clauses...]`: the head plus
    /// the select list form one clause; the remaining clause nodes flow at
    /// statement level.
    fn select_core(&mut self, node: &SyntaxNode) -> Doc {
        let mut head = Vec::new();
        let mut list = Vec::new();
        let mut tail = Vec::new();
        let mut items = ListJoiner::new();
        let mut seen_list = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    let docs = if seen_list { &mut tail } else { &mut head };
                    self.trivia(docs, token);
                }
                SyntaxElement::Node(child) if child.kind() == SyntaxKind::SelectList => {
                    seen_list = true;
                    let doc = self.comma_list(child);
                    self.push(&mut list, doc);
                }
                SyntaxElement::Node(child) if seen_list => {
                    // from / where / group by / having / window clauses.
                    if !tail.is_empty() {
                        tail.push(soft_line_or_space());
                    }
                    let doc = self.node(child);
                    if tail.is_empty() {
                        self.push(&mut tail, concat([soft_line_or_space(), doc]));
                    } else {
                        self.push(&mut tail, doc);
                    }
                }
                element => {
                    // Head: select / all / distinct [on (...)] tokens and
                    // the DISTINCT ON expressions.
                    self.list_element(&mut head, &mut items, element);
                }
            }
        }
        concat([clause(head, list), concat(tail)])
    }

    /// DDL statement: ALTER statements get their action list indented one
    /// level, one action per line when broken; everything else uses the
    /// plain DML flow.
    fn ddl_statement(&mut self, docs: &mut Vec<Doc>, node: &SyntaxNode) {
        let is_alter = node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .find(|t| !t.kind().is_trivia())
            .is_some_and(|t| t.text().eq_ignore_ascii_case("alter"));
        if is_alter {
            self.alter_flow(docs, node);
        } else {
            self.dml_flow(docs, node);
        }
    }

    /// `ALTER TABLE name` head, then each action on its own (indented)
    /// soft line, commas attached to the preceding action.
    fn alter_flow(&mut self, docs: &mut Vec<Doc>, node: &SyntaxNode) {
        const ACTION_KWS: &[&str] = &[
            "add", "drop", "alter", "rename", "validate", "owner", "set", "reset", "enable",
            "disable", "attach", "detach", "cluster", "replica", "inherit", "force", "no",
        ];
        let mut head: Vec<Doc> = Vec::new();
        let mut segments: Vec<Vec<Doc>> = Vec::new();
        let mut head_tokens = 0usize;
        let mut first = true;
        let mut tight = false;
        let mut semicolon = false;
        let mut pending_segment = false;
        for element in node.children_with_tokens() {
            // Transitions first, so the borrow below targets the right vec.
            if let SyntaxElement::Token(token) = element
                && !token.kind().is_trivia()
            {
                if segments.is_empty()
                    && token.kind() == SyntaxKind::Ident
                    && head_tokens >= 2
                    && ACTION_KWS
                        .iter()
                        .any(|kw| token.text().eq_ignore_ascii_case(kw))
                {
                    segments.push(Vec::new());
                    first = true;
                    tight = false;
                } else if !segments.is_empty() && token.kind() == SyntaxKind::Comma {
                    let current = segments.last_mut().expect("segment open");
                    self.push(current, text(","));
                    // Start the next segment lazily so a trailing comment
                    // after the comma stays with this action.
                    pending_segment = true;
                    continue;
                } else if pending_segment {
                    pending_segment = false;
                    segments.push(Vec::new());
                    first = true;
                    tight = false;
                }
            }
            let current = segments.last_mut().unwrap_or(&mut head);
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(current, token)
                }
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::Semicolon => {
                    semicolon = true;
                }
                SyntaxElement::Token(token) => {
                    let tight_before = tight
                        || matches!(
                            token.kind(),
                            SyntaxKind::RParen
                                | SyntaxKind::RBracket
                                | SyntaxKind::LBracket
                                | SyntaxKind::Dot
                                | SyntaxKind::ColonColon
                        );
                    if !first && !tight_before {
                        current.push(space());
                    }
                    tight = matches!(
                        token.kind(),
                        SyntaxKind::LParen
                            | SyntaxKind::LBracket
                            | SyntaxKind::Dot
                            | SyntaxKind::ColonColon
                    );
                    let leaf = match token.kind() {
                        SyntaxKind::QuotedIdent => name_leaf(token, IdentPos::ColumnOrTable),
                        _ => token_leaf(token),
                    };
                    self.push(current, leaf);
                    if segments.is_empty() {
                        head_tokens += 1;
                    }
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let clause = is_clause_level(child.kind());
                    if clause {
                        if !first {
                            current.push(soft_line_or_space());
                        }
                    } else if !first && !tight {
                        current.push(space());
                    }
                    tight = false;
                    let doc = self.node(child);
                    self.push(current, doc);
                    first = false;
                }
            }
        }
        docs.extend(head);
        if !segments.is_empty() {
            let mut actions = Vec::new();
            for segment in segments {
                if segment.is_empty() {
                    continue;
                }
                actions.push(soft_line_or_space());
                actions.extend(segment);
            }
            docs.push(indent(concat(actions)));
        }
        if semicolon {
            self.push(docs, text(";"));
        }
    }

    /// DML/DDL statement flow: keyword-soup tokens space-joined, clause
    /// children on soft lines, top-level commas (ALTER action lists,
    /// multiple targets) breaking softly, `;` attached tight.
    fn dml_flow(&mut self, docs: &mut Vec<Doc>, node: &SyntaxNode) {
        let mut first = true;
        let mut pending_sls = false;
        let mut tight = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => self.trivia(docs, token),
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::Semicolon => {
                    self.push(docs, text(";"));
                }
                SyntaxElement::Token(token) if token.kind() == SyntaxKind::Comma => {
                    self.push(docs, text(","));
                    pending_sls = true;
                }
                SyntaxElement::Token(token) => {
                    // Inline parens (INSERT column lists): tight inside.
                    let tight_before = tight
                        || matches!(
                            token.kind(),
                            SyntaxKind::RParen
                                | SyntaxKind::RBracket
                                | SyntaxKind::LBracket
                                | SyntaxKind::Dot
                                | SyntaxKind::ColonColon
                        );
                    // `RAISE ... USING` / `EXECUTE ... USING` can break
                    // before the USING keyword.
                    let soft_break =
                        matches!(node.kind(), SyntaxKind::PlRaise | SyntaxKind::PlExecute)
                            && token.text().eq_ignore_ascii_case("using");
                    if (soft_break && !first) || (pending_sls && !tight_before) {
                        docs.push(soft_line_or_space());
                    } else if !first && !tight_before {
                        docs.push(space());
                    }
                    pending_sls = false;
                    tight = matches!(
                        token.kind(),
                        SyntaxKind::LParen
                            | SyntaxKind::LBracket
                            | SyntaxKind::Dot
                            | SyntaxKind::ColonColon
                    );
                    let leaf = match token.kind() {
                        SyntaxKind::QuotedIdent => name_leaf(token, IdentPos::ColumnOrTable),
                        _ => token_leaf(token),
                    };
                    self.push(docs, leaf);
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let clause = is_clause_level(child.kind());
                    if clause {
                        if !first {
                            docs.push(soft_line_or_space());
                        }
                        pending_sls = false;
                    } else if tight {
                        pending_sls = false;
                    } else if pending_sls {
                        docs.push(soft_line_or_space());
                        pending_sls = false;
                    } else if !first {
                        docs.push(space());
                    }
                    tight = false;
                    let doc = self.node(child);
                    self.push(docs, doc);
                    first = false;
                }
            }
        }
    }

    /// `KEYWORDS content` clause: leading keyword tokens, then the rest as
    /// grouped, indented content that collapses onto the keyword line when
    /// it fits.
    fn kw_clause(&mut self, node: &SyntaxNode) -> Doc {
        let mut head = Vec::new();
        let mut content = Vec::new();
        let mut in_head = true;
        let mut head_first = true;
        let mut items = ListJoiner::new();
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    let docs = if in_head { &mut head } else { &mut content };
                    self.trivia(docs, token);
                }
                SyntaxElement::Token(token) if in_head && token.kind() == SyntaxKind::Ident => {
                    if !head_first {
                        head.push(space());
                    }
                    self.push(&mut head, keyword(token.text()));
                    head_first = false;
                }
                element => {
                    in_head = false;
                    self.list_element(&mut content, &mut items, element);
                }
            }
        }
        clause(head, content)
    }

    /// Comma-separated list content (select lists, from lists, CTE lists).
    fn comma_list(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut items = ListJoiner::new();
        for element in node.children_with_tokens() {
            self.list_element(&mut docs, &mut items, element);
        }
        concat(docs)
    }

    /// Shared element handling for comma-separated content: `,` attaches
    /// tight and puts the next item on a soft line; other neighbors join
    /// with spaces.
    fn list_element(
        &mut self,
        docs: &mut Vec<Doc>,
        items: &mut ListJoiner,
        element: SyntaxElement<'_>,
    ) {
        match element {
            SyntaxElement::Token(token) if token.kind().is_trivia() => self.trivia(docs, token),
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::Comma => {
                self.push(docs, text(","));
                items.next_sep = Some(soft_line_or_space());
            }
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::LParen => {
                items.sep(docs);
                self.push(docs, text("("));
                items.tight_next = true;
            }
            SyntaxElement::Token(token) if token.kind() == SyntaxKind::RParen => {
                // Tight before `)`.
                items.next_sep = None;
                items.tight_next = true;
                items.sep(docs);
                self.push(docs, text(")"));
            }
            SyntaxElement::Token(token) => {
                items.sep(docs);
                self.push(docs, token_leaf(token));
            }
            SyntaxElement::Node(child) => {
                items.sep(docs);
                let doc = self.node(child);
                self.push(docs, doc);
            }
        }
    }

    // ---- specific constructs ----

    fn select_item(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    if !first {
                        docs.push(space());
                    }
                    // `as` is a keyword; the alias itself is a name.
                    let leaf = if token.kind() == SyntaxKind::Ident
                        && token.text().eq_ignore_ascii_case("as")
                    {
                        keyword(token.text())
                    } else {
                        name_leaf(token, IdentPos::ColumnOrTable)
                    };
                    self.push(&mut docs, leaf);
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    if !first {
                        docs.push(space());
                    }
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                    first = false;
                }
            }
        }
        concat(docs)
    }

    /// Table references: `[lateral] [only] name [args] [as] [alias (cols)]`.
    fn table_ref(&mut self, node: &SyntaxNode) -> Doc {
        let has_args = node
            .children()
            .any(|child| child.kind() == SyntaxKind::ArgList);
        let name_pos = if has_args {
            IdentPos::TypeOrFunction
        } else {
            IdentPos::ColumnOrTable
        };
        let mut docs = Vec::new();
        let mut first = true;
        let mut seen_name = false;
        let mut tight = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    let lower = token.text().to_ascii_lowercase();
                    let is_marker = matches!(
                        lower.as_str(),
                        "lateral" | "only" | "as" | "with" | "ordinality"
                    ) && token.kind() == SyntaxKind::Ident;
                    let leaf = match token.kind() {
                        SyntaxKind::Dot => {
                            tight = true;
                            text(".")
                        }
                        SyntaxKind::Ident | SyntaxKind::QuotedIdent if is_marker && !seen_name => {
                            name_leaf(token, name_pos)
                        }
                        SyntaxKind::Ident if is_marker => keyword(token.text()),
                        SyntaxKind::Ident | SyntaxKind::QuotedIdent => {
                            let pos = if seen_name {
                                IdentPos::ColumnOrTable // alias / column names
                            } else {
                                name_pos
                            };
                            name_leaf(token, pos)
                        }
                        SyntaxKind::LParen | SyntaxKind::RParen | SyntaxKind::Comma => {
                            // Alias column list: tight parens.
                            let t = raw_leaf(token);
                            if token.kind() == SyntaxKind::LParen {
                                tight = true;
                            }
                            t
                        }
                        _ => raw_leaf(token),
                    };
                    let attach_tight = tight
                        || matches!(
                            token.kind(),
                            SyntaxKind::Dot | SyntaxKind::RParen | SyntaxKind::Comma
                        );
                    if !first && !attach_tight {
                        docs.push(space());
                    }
                    if token.kind() != SyntaxKind::Dot && token.kind() != SyntaxKind::LParen {
                        tight = false;
                    }
                    if matches!(token.kind(), SyntaxKind::Ident | SyntaxKind::QuotedIdent)
                        && !is_marker
                    {
                        seen_name = true;
                    }
                    self.push(&mut docs, leaf);
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let is_args = child.kind() == SyntaxKind::ArgList;
                    if !first && !is_args {
                        docs.push(space());
                    }
                    if child.kind() == SyntaxKind::SubqueryExpr {
                        seen_name = true;
                    }
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                    first = false;
                }
            }
        }
        concat(docs)
    }

    /// Flattened join chains: each join segment on its own soft line.
    fn join_expr(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        self.join_segments(&mut docs, node);
        group(concat(docs))
    }

    fn join_segments(&mut self, docs: &mut Vec<Doc>, node: &SyntaxNode) {
        let mut first = true;
        let mut segment_started = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => self.trivia(docs, token),
                SyntaxElement::Token(token) => {
                    // Join keywords start a new soft-line segment.
                    if !segment_started {
                        docs.push(soft_line_or_space());
                        segment_started = true;
                    } else {
                        docs.push(space());
                    }
                    self.push(docs, keyword(token.text()));
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    if child.kind() == SyntaxKind::JoinExpr && first {
                        // Flatten the left-nested join chain.
                        self.join_segments(docs, child);
                    } else {
                        if !first && !segment_started {
                            docs.push(soft_line_or_space());
                        } else if !first {
                            docs.push(space());
                        }
                        let doc = self.node(child);
                        self.push(docs, doc);
                    }
                    first = false;
                    if child.kind() == SyntaxKind::JoinCondition {
                        segment_started = false;
                    }
                }
            }
        }
    }

    /// CTE: `name [(cols)] as [not materialized] ( body ) [search] [cycle]`.
    fn cte(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut first = true;
        let mut seen_as = false;
        let mut body: Option<Vec<Doc>> = None;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    let docs = body.as_mut().unwrap_or(&mut docs);
                    self.trivia(docs, token);
                    // Trivia must not count as a first element.
                    continue;
                }
                SyntaxElement::Token(token) => match token.kind() {
                    SyntaxKind::LParen if seen_as && body.is_none() => {
                        body = Some(Vec::new());
                        self.at_line_start = false;
                    }
                    SyntaxKind::RParen if body.is_some() => {
                        // Close the body: `( body )` block. Comments
                        // pending before the `)` belong inside it.
                        let mut inner = body.take().expect("body open");
                        self.flush_pending(&mut inner);
                        self.at_line_start = false;
                        let inner = inner;
                        docs.push(space());
                        self.push(
                            &mut docs,
                            group(concat([
                                text("("),
                                indent(concat(
                                    [soft_line()].into_iter().chain(inner).collect::<Vec<_>>(),
                                )),
                                soft_line(),
                                text(")"),
                            ])),
                        );
                    }
                    _ => {
                        let in_body = body.is_some();
                        let lower = token.text().to_ascii_lowercase();
                        let leaf = match token.kind() {
                            SyntaxKind::Ident
                                if matches!(
                                    lower.as_str(),
                                    "as" | "not" | "materialized" | "recursive"
                                ) =>
                            {
                                if lower == "as" {
                                    seen_as = true;
                                }
                                keyword(token.text())
                            }
                            SyntaxKind::Ident | SyntaxKind::QuotedIdent => {
                                name_leaf(token, IdentPos::ColumnOrTable)
                            }
                            _ => raw_leaf(token),
                        };
                        let tight = matches!(
                            token.kind(),
                            SyntaxKind::RParen | SyntaxKind::Comma | SyntaxKind::LParen
                        );
                        let docs_ref = body.as_mut().unwrap_or(&mut docs);
                        let want_space = if in_body {
                            !docs_ref.is_empty() && !tight
                        } else {
                            !first && !tight
                        };
                        if want_space {
                            docs_ref.push(space());
                        }
                        self.push(docs_ref, leaf);
                    }
                },
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    match &mut body {
                        Some(inner) => {
                            if !inner.is_empty() {
                                inner.push(soft_line_or_space());
                            }
                            self.push(inner, doc);
                        }
                        None => {
                            if !first {
                                docs.push(space());
                            }
                            self.push(&mut docs, doc);
                        }
                    }
                }
            }
            first = false;
        }
        concat(docs)
    }

    /// A parenthesized block: `(` soft-indented contents `)`. Used for
    /// subqueries, arg lists, window specs, paren/row/array expressions.
    fn paren_block(&mut self, node: &SyntaxNode) -> Doc {
        let mut head = Vec::new();
        let mut inner = Vec::new();
        let mut tail = Vec::new();
        let mut in_inner = false;
        let mut closed = false;
        let mut head_content = false;
        let mut tail_content = false;
        let mut items = ListJoiner::new();
        let mut open = text("(");
        let mut close = text(")");
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    let docs = if closed {
                        &mut tail
                    } else if in_inner {
                        &mut inner
                    } else {
                        &mut head
                    };
                    self.trivia(docs, token);
                }
                SyntaxElement::Token(token)
                    if matches!(token.kind(), SyntaxKind::LParen | SyntaxKind::LBracket)
                        && !in_inner =>
                {
                    if token.kind() == SyntaxKind::LBracket {
                        open = text("[");
                        close = text("]");
                    }
                    self.flush_pending(&mut head);
                    in_inner = true;
                    // The paren is content, even though it is emitted via
                    // the block skeleton rather than push().
                    self.at_line_start = false;
                }
                SyntaxElement::Token(token)
                    if matches!(token.kind(), SyntaxKind::RParen | SyntaxKind::RBracket)
                        && in_inner
                        && !closed =>
                {
                    self.flush_pending(&mut inner);
                    closed = true;
                    self.at_line_start = false;
                }
                element => {
                    if !in_inner || closed {
                        // Head keywords (`exists`, `array`, `row`, ...)
                        // before the parens; trailing parts after them.
                        let (docs, content_flag) = if closed {
                            (&mut tail, &mut tail_content)
                        } else {
                            (&mut head, &mut head_content)
                        };
                        if let SyntaxElement::Token(token) = element {
                            if *content_flag {
                                docs.push(space());
                            }
                            self.push(docs, token_leaf(token));
                            *content_flag = true;
                        } else if let SyntaxElement::Node(child) = element {
                            if *content_flag {
                                docs.push(space());
                            }
                            let doc = self.node(child);
                            self.push(docs, doc);
                            *content_flag = true;
                        }
                    } else {
                        // Clause-flow inside subqueries; list-flow inside
                        // everything else. Clause nodes join with soft
                        // lines, list elements via the comma joiner.
                        match element {
                            SyntaxElement::Node(child) if is_clause_level(child.kind()) => {
                                if !inner.is_empty() {
                                    inner.push(soft_line_or_space());
                                }
                                let doc = self.node(child);
                                self.push(&mut inner, doc);
                            }
                            element => self.list_element(&mut inner, &mut items, element),
                        }
                    }
                }
            }
        }
        let head_doc = concat(head);
        if !in_inner {
            // The parentheses live inside a child node (e.g. `x in
            // (subquery)`): nothing to wrap here.
            return group(head_doc);
        }
        // `exists (`, `x in (`: space between head content and the open
        // paren — but tight for call-like forms (`any(`, `row(`, args).
        let attach = if head_content
            && !matches!(
                node.kind(),
                SyntaxKind::ArgList | SyntaxKind::RowExpr | SyntaxKind::QuantifiedExpr
            ) {
            space()
        } else {
            nil()
        };
        if inner.is_empty() {
            // Empty parens never benefit from breaking: `f()`.
            return group(concat([head_doc, attach, open, close, concat(tail)]));
        }
        group(concat([
            head_doc,
            attach,
            open,
            indent(concat(
                [soft_line()].into_iter().chain(inner).collect::<Vec<_>>(),
            )),
            soft_line(),
            close,
            concat(tail),
        ]))
    }

    /// `lhs [not] in (...)` / `any (...)`: keywords then a paren tail.
    fn tail_paren_expr(&mut self, node: &SyntaxNode) -> Doc {
        // Reuse paren_block; its "head" handling covers the lhs and
        // keywords, and the paren run covers the tail.
        self.paren_block(node)
    }

    fn case_expr(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut arms = Vec::new();
        let mut tail = Vec::new();
        let mut stage = 0; // 0 = head (case + operand), 1 = arms/else, 2 = end
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    let docs = match stage {
                        0 => &mut docs,
                        1 => &mut arms,
                        _ => &mut tail,
                    };
                    self.trivia(docs, token);
                }
                SyntaxElement::Token(token) => {
                    let lower = token.text().to_ascii_lowercase();
                    match lower.as_str() {
                        "end" => {
                            stage = 2;
                            self.push(&mut tail, keyword(token.text()));
                        }
                        "else" => {
                            stage = 1;
                            arms.push(soft_line_or_space());
                            self.push(&mut arms, keyword(token.text()));
                        }
                        _ => {
                            let docs = match stage {
                                0 => &mut docs,
                                1 => &mut arms,
                                _ => &mut tail,
                            };
                            if !docs.is_empty() {
                                docs.push(space());
                            }
                            self.push(docs, keyword(token.text()));
                        }
                    }
                }
                SyntaxElement::Node(child) => {
                    if child.kind() == SyntaxKind::WhenClause {
                        stage = 1;
                        arms.push(soft_line_or_space());
                        let doc = self.node(child);
                        self.push(&mut arms, doc);
                    } else {
                        let docs = match stage {
                            0 => &mut docs,
                            1 => &mut arms,
                            _ => &mut tail,
                        };
                        if !docs.is_empty() {
                            docs.push(space());
                        }
                        let doc = self.node(child);
                        self.push(docs, doc);
                    }
                }
            }
        }
        group(concat([
            concat(docs),
            indent(concat(arms)),
            soft_line_or_space(),
            concat(tail),
        ]))
    }

    fn when_clause(&mut self, node: &SyntaxNode) -> Doc {
        let mut head = Vec::new();
        let mut then = Vec::new();
        let mut in_then = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    let docs = if in_then { &mut then } else { &mut head };
                    self.trivia(docs, token);
                }
                SyntaxElement::Token(token) => {
                    if token.text().eq_ignore_ascii_case("then") {
                        in_then = true;
                        self.push(&mut then, keyword(token.text()));
                    } else {
                        let docs = if in_then { &mut then } else { &mut head };
                        if !docs.is_empty() {
                            docs.push(space());
                        }
                        self.push(docs, keyword(token.text()));
                    }
                }
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    let docs = if in_then { &mut then } else { &mut head };
                    if !docs.is_empty() {
                        docs.push(space());
                    }
                    self.push(docs, doc);
                }
            }
        }
        group(concat([
            concat(head),
            indent(concat(
                [soft_line_or_space()]
                    .into_iter()
                    .chain(then)
                    .collect::<Vec<_>>(),
            )),
        ]))
    }

    fn function_call(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    if !first {
                        docs.push(space());
                    }
                    self.push(&mut docs, token_leaf(token));
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let doc = match child.kind() {
                        SyntaxKind::ColumnRef => self.column_ref(child, IdentPos::TypeOrFunction),
                        _ => self.node(child),
                    };
                    // ArgList attaches tight to the name; clause nodes
                    // (FILTER/WITHIN GROUP/OVER) join with spaces.
                    if !first && child.kind() != SyntaxKind::ArgList {
                        docs.push(space());
                    }
                    self.push(&mut docs, doc);
                    first = false;
                }
            }
        }
        concat(docs)
    }

    fn column_ref(&mut self, node: &SyntaxNode, pos: IdentPos) -> Doc {
        let mut docs = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    let leaf = name_leaf(token, pos);
                    self.push(&mut docs, leaf);
                }
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                }
            }
        }
        concat(docs)
    }

    fn cast_expr(&mut self, node: &SyntaxNode) -> Doc {
        // `expr::type` tight, or `cast( expr as type )` via paren block.
        let has_cast_kw = node
            .first_token()
            .is_some_and(|t| t.text().eq_ignore_ascii_case("cast"));
        if has_cast_kw {
            return self.paren_block(node);
        }
        let mut docs = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => self.push(&mut docs, raw_leaf(token)),
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                }
            }
        }
        concat(docs)
    }

    fn type_name(&mut self, node: &SyntaxNode) -> Doc {
        let qualified = node
            .children_with_tokens()
            .filter_map(|el| el.into_token())
            .any(|t| t.kind() == SyntaxKind::Dot);
        let mut docs = Vec::new();
        let mut tight = false;
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    let leaf = match token.kind() {
                        // Bare unqualified type words act as keywords for
                        // casing (`INT`, `DOUBLE PRECISION`); qualified or
                        // quoted names pass through untouched.
                        SyntaxKind::Ident if !qualified => keyword(token.text()),
                        _ => raw_leaf(token),
                    };
                    let is_tight_kind = matches!(
                        token.kind(),
                        SyntaxKind::Dot
                            | SyntaxKind::LParen
                            | SyntaxKind::RParen
                            | SyntaxKind::LBracket
                            | SyntaxKind::RBracket
                            | SyntaxKind::Comma
                            | SyntaxKind::Number
                    );
                    if !first && !tight && !is_tight_kind {
                        docs.push(space());
                    }
                    tight = matches!(
                        token.kind(),
                        SyntaxKind::Dot | SyntaxKind::LParen | SyntaxKind::LBracket
                    );
                    self.push(&mut docs, leaf);
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                    first = false;
                }
            }
        }
        concat(docs)
    }

    fn prefix_expr(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut needs_space = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    if token.kind() == SyntaxKind::Ident {
                        // `not x`
                        self.push(&mut docs, keyword(token.text()));
                        needs_space = true;
                    } else {
                        // `-x`, `@name`: tight.
                        self.push(&mut docs, raw_leaf(token));
                        needs_space = false;
                    }
                }
                SyntaxElement::Node(child) => {
                    if needs_space {
                        docs.push(space());
                    }
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                    needs_space = false;
                }
            }
        }
        concat(docs)
    }

    /// Binary-ish expressions: `and`/`or` chains flatten with the operator
    /// leading each soft line; other operators get a soft line before the
    /// operator run.
    fn binary_expr(&mut self, node: &SyntaxNode) -> Doc {
        let chain_op = bool_chain_op(node);
        let mut docs = Vec::new();
        self.binary_parts(&mut docs, node, chain_op.as_deref());
        group(concat(docs))
    }

    fn binary_parts(&mut self, docs: &mut Vec<Doc>, node: &SyntaxNode, chain_op: Option<&str>) {
        let mut first = true;
        let mut after_op = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => self.trivia(docs, token),
                SyntaxElement::Token(token) => {
                    if after_op {
                        docs.push(space());
                    } else {
                        docs.push(soft_line_or_space());
                    }
                    let leaf = match token.kind() {
                        SyntaxKind::Ident => keyword(token.text()),
                        _ => raw_leaf(token),
                    };
                    self.push(docs, leaf);
                    after_op = true;
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    // Flatten same-operator boolean chains.
                    let flatten = first
                        && chain_op.is_some()
                        && child.kind() == SyntaxKind::BinaryExpr
                        && bool_chain_op(child).as_deref() == chain_op;
                    if flatten {
                        self.binary_parts(docs, child, chain_op);
                    } else {
                        if after_op {
                            docs.push(space());
                        } else if !first {
                            docs.push(soft_line_or_space());
                        }
                        let doc = self.node(child);
                        self.push(docs, doc);
                    }
                    after_op = false;
                    first = false;
                }
            }
        }
    }

    /// PL/pgSQL block: `[<<label>>] [declare decls] begin stmts
    /// [exception handlers] end [label];` — section keywords at block
    /// indent 0, contents indented once.
    fn pl_block(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    let lower = token.text().to_ascii_lowercase();
                    let section = matches!(lower.as_str(), "declare" | "begin" | "end");
                    if section && !first {
                        docs.push(hard_line());
                    } else if !first && token.kind() != SyntaxKind::Semicolon {
                        docs.push(space());
                    }
                    self.push(&mut docs, token_leaf(token));
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    if child.kind() == SyntaxKind::PlException {
                        docs.push(hard_line());
                        self.push(&mut docs, doc);
                    } else {
                        // Declarations and statements: indented lines.
                        self.push(&mut docs, indent(concat([hard_line(), doc])));
                    }
                    first = false;
                }
            }
        }
        concat(docs)
    }

    /// PL/pgSQL control-flow container (`if ... then` / loops / `when ...
    /// then` arms / `case`): header tokens and expressions inline,
    /// statements indented one level, arm nodes (`elsif`/`else`/`when`)
    /// back at the container's indent (or indented, for CASE arms), the
    /// closing `end ...` on its own line.
    fn pl_container(&mut self, node: &SyntaxNode, indent_arms: bool) -> Doc {
        let mut docs = Vec::new();
        let mut first = true;
        let mut seen_end = false;
        let mut tight_dot = false;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    let lower = token.text().to_ascii_lowercase();
                    // `1..3` ranges: dots attach tight (the lexer may
                    // split them as `1`, `.`, `.3`).
                    let tight = tight_dot
                        || token.kind() == SyntaxKind::Dot
                        || (token.kind() == SyntaxKind::Number && token.text().starts_with('.'));
                    tight_dot = token.kind() == SyntaxKind::Dot;
                    if lower == "end" {
                        seen_end = true;
                        docs.push(hard_line());
                    } else if token.kind() != SyntaxKind::Semicolon && !first && !tight {
                        docs.push(space());
                    }
                    self.push(&mut docs, token_leaf(token));
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    let arm = matches!(
                        child.kind(),
                        SyntaxKind::PlElsif
                            | SyntaxKind::PlElse
                            | SyntaxKind::PlWhen
                            | SyntaxKind::PlException
                    );
                    let statement = is_pl_statement(child.kind());
                    let doc = self.node(child);
                    if arm {
                        if indent_arms {
                            self.push(&mut docs, indent(concat([hard_line(), doc])));
                        } else {
                            docs.push(hard_line());
                            self.push(&mut docs, doc);
                        }
                    } else if statement && !seen_end {
                        self.push(&mut docs, indent(concat([hard_line(), doc])));
                    } else {
                        // Range bounds after `..` attach tight.
                        if !first && !tight_dot {
                            docs.push(space());
                        }
                        self.push(&mut docs, doc);
                    }
                    tight_dot = false;
                    first = false;
                }
            }
        }
        concat(docs)
    }

    /// `exception` with its `when ... then` handlers on following lines.
    fn pl_exception(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    self.push(&mut docs, token_leaf(token));
                }
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    self.push(&mut docs, indent(concat([hard_line(), doc])));
                }
            }
        }
        concat(docs)
    }

    /// Space-joined flow with tight punctuation; the fallback layout.
    fn space_flow(&mut self, node: &SyntaxNode, pos: IdentPos) -> Doc {
        let mut docs = Vec::new();
        let mut tight = false;
        let mut first = true;
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => {
                    let no_space_before = tight
                        || matches!(
                            token.kind(),
                            SyntaxKind::Dot
                                | SyntaxKind::Comma
                                | SyntaxKind::RParen
                                | SyntaxKind::RBracket
                                | SyntaxKind::ColonColon
                                | SyntaxKind::Semicolon
                        );
                    if !first && !no_space_before {
                        docs.push(space());
                    }
                    tight = matches!(
                        token.kind(),
                        SyntaxKind::Dot
                            | SyntaxKind::LParen
                            | SyntaxKind::LBracket
                            | SyntaxKind::ColonColon
                    );
                    let leaf = match token.kind() {
                        SyntaxKind::QuotedIdent => name_leaf(token, pos),
                        _ => token_leaf(token),
                    };
                    self.push(&mut docs, leaf);
                    first = false;
                }
                SyntaxElement::Node(child) => {
                    if !first && !tight {
                        docs.push(space());
                    }
                    tight = false;
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                    first = false;
                }
            }
        }
        concat(docs)
    }

    /// Fully tight flow (subscripts, `::` chains).
    fn tight_flow(&mut self, node: &SyntaxNode) -> Doc {
        let mut docs = Vec::new();
        for element in node.children_with_tokens() {
            match element {
                SyntaxElement::Token(token) if token.kind().is_trivia() => {
                    self.trivia(&mut docs, token)
                }
                SyntaxElement::Token(token) => self.push(&mut docs, raw_leaf(token)),
                SyntaxElement::Node(child) => {
                    let doc = self.node(child);
                    self.push(&mut docs, doc);
                }
            }
        }
        concat(docs)
    }
}

/// PL/pgSQL container statements (multi-line by construction).
fn is_pl_container(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::PlBlock
            | SyntaxKind::PlIf
            | SyntaxKind::PlCase
            | SyntaxKind::PlLoop
            | SyntaxKind::PlWhile
            | SyntaxKind::PlFor
            | SyntaxKind::PlForeach
    )
}

/// Any PL/pgSQL or SQL statement kind (a line of its own inside blocks).
fn is_pl_statement(kind: SyntaxKind) -> bool {
    is_pl_container(kind)
        || matches!(
            kind,
            SyntaxKind::PlExit
                | SyntaxKind::PlReturn
                | SyntaxKind::PlRaise
                | SyntaxKind::PlAssign
                | SyntaxKind::PlPerform
                | SyntaxKind::PlExecute
                | SyntaxKind::PlGetDiag
                | SyntaxKind::PlNull
                | SyntaxKind::SelectStmt
                | SyntaxKind::InsertStmt
                | SyntaxKind::UpdateStmt
                | SyntaxKind::DeleteStmt
                | SyntaxKind::DdlStmt
                | SyntaxKind::ErrorStatement
                | SyntaxKind::EmptyStmt
        )
}

/// Is this node one of the clause-level pieces inside a query body?
fn is_clause_level(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        SyntaxKind::WithClause
            | SyntaxKind::SelectCore
            | SyntaxKind::SetOperation
            | SyntaxKind::ValuesClause
            | SyntaxKind::TableCore
            | SyntaxKind::OrderByClause
            | SyntaxKind::LimitClause
            | SyntaxKind::OffsetClause
            | SyntaxKind::FetchClause
            | SyntaxKind::LockingClause
            | SyntaxKind::FromClause
            | SyntaxKind::WhereClause
            | SyntaxKind::GroupByClause
            | SyntaxKind::HavingClause
            | SyntaxKind::WindowClause
            | SyntaxKind::SetClause
            | SyntaxKind::UsingClause
            | SyntaxKind::OnConflictClause
            | SyntaxKind::ReturningClause
    )
}

/// The `and`/`or` keyword when this BinaryExpr is a boolean chain link.
fn bool_chain_op(node: &SyntaxNode) -> Option<String> {
    if node.kind() != SyntaxKind::BinaryExpr {
        return None;
    }
    node.children_with_tokens()
        .filter_map(|el| el.into_token())
        .filter(|t| t.kind() == SyntaxKind::Ident)
        .map(|t| t.text().to_ascii_lowercase())
        .find(|t| t == "and" || t == "or")
}

/// Raw token text as a leaf: multi-line tokens (dollar-quoted bodies,
/// multi-line strings) become verbatim.
fn raw_leaf(token: &SyntaxToken) -> Doc {
    if token.text().contains('\n') {
        verbatim(token.text())
    } else {
        text(token.text())
    }
}

/// Default leaf for a token in keyword-position contexts. Bare words act
/// as keywords for casing (semantically safe: bare identifiers fold
/// case-insensitively); everything else passes through.
fn token_leaf(token: &SyntaxToken) -> Doc {
    match token.kind() {
        SyntaxKind::Ident => keyword(token.text()),
        _ => raw_leaf(token),
    }
}

/// Leaf for a token in name position: identifiers get the quoting
/// transform.
fn name_leaf(token: &SyntaxToken, pos: IdentPos) -> Doc {
    match token.kind() {
        SyntaxKind::Ident | SyntaxKind::QuotedIdent => ident(token.text(), pos),
        _ => raw_leaf(token),
    }
}

/// `KEYWORDS` + indented, grouped content that collapses when it fits.
fn clause(head: Vec<Doc>, content: Vec<Doc>) -> Doc {
    if content.is_empty() {
        return group(concat(head));
    }
    group(concat([
        concat(head),
        indent(concat([soft_line_or_space(), group(concat(content))])),
    ]))
}

/// Separator state for comma-joined lists.
struct ListJoiner {
    next_sep: Option<Doc>,
    tight_next: bool,
    any: bool,
}

impl ListJoiner {
    fn new() -> Self {
        ListJoiner {
            next_sep: None,
            tight_next: false,
            any: false,
        }
    }

    fn sep(&mut self, docs: &mut Vec<Doc>) {
        if self.tight_next {
            self.tight_next = false;
            self.next_sep = None;
        } else if let Some(sep) = self.next_sep.take() {
            docs.push(sep);
        } else if self.any {
            docs.push(space());
        }
        self.any = true;
    }
}

// Suppress an unused-import lint when if_break gains users in TREE-98.
#[allow(unused_imports)]
use if_break as _if_break;
