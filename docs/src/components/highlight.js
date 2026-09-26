import { createHighlighter } from "shiki";
import { shikiTheme } from "../playground/theme.js";

// A TextMate grammar for tree-sitter queries (.scm), which shiki doesn't
// bundle. Small on purpose: it colors what a reader scans for — node
// names, captures, predicates, fields, strings, and comments.
export const queryGrammar = {
	name: "query",
	scopeName: "source.query",
	patterns: [
		{ match: ";.*$", name: "comment.line.semicolon.query" },
		{
			begin: '"',
			end: '"',
			name: "string.quoted.double.query",
			patterns: [{ match: "\\\\.", name: "constant.character.escape.query" }],
		},
		// @sql, @sql.sqlite, @_method
		{ match: "@[\\w.-]+", name: "constant.numeric.capture.query" },
		// #eq?, #any-of?, #set!
		{ match: "#[\\w-]+[?!]?", name: "keyword.control.predicate.query" },
		// field: before a node
		{ match: "[\\w-]+(?=\\s*:)", name: "variable.other.field.query" },
		// (node_kind — the name right after an open paren
		{
			match: "(?<=\\()\\s*([\\w-]+)",
			captures: { 1: { name: "entity.name.function.node.query" } },
		},
	],
};

// One highlighter for every recipe, with the query grammar loaded and
// the bundled languages pulled in as pages ask for them.
let highlighter;
export async function highlight(code, lang) {
	highlighter ??= createHighlighter({
		themes: [shikiTheme],
		langs: [queryGrammar],
	});
	const h = await highlighter;
	if (!h.getLoadedLanguages().includes(lang)) await h.loadLanguage(lang);
	return h.codeToHtml(code, { lang, theme: shikiTheme.name });
}
