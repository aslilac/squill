import { createHighlighter } from "shiki";
import { colors, shikiTheme } from "../playground/theme.js";

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
async function loaded(lang) {
	highlighter ??= createHighlighter({
		themes: [shikiTheme],
		langs: [queryGrammar, "sql"],
	});
	const h = await highlighter;
	if (!h.getLoadedLanguages().includes(lang)) await h.loadLanguage(lang);
	return h;
}

export async function highlight(code, lang) {
	const h = await loaded(lang);
	return h.codeToHtml(code, { lang, theme: shikiTheme.name });
}

// Host code with SQL in its strings, twice over: once with the SQL
// highlighted as SQL, once with it colored as the plain string it is.
// Host grammars disagree — shiki's Ruby and C++ inject SQL by delimiter
// name, the rest never do — so both are built the same way here: a string
// body is SQL when a grammar already said so, or when it starts like a
// statement.
export async function highlightHost(code, lang) {
	const h = await loaded(lang);
	const { tokens } = h.codeToTokens(code, {
		lang,
		theme: shikiTheme.name,
		includeExplanation: true,
	});

	// Flatten to scoped pieces, keeping line breaks as pieces of their own.
	const pieces = [];
	tokens.forEach((line, i) => {
		if (i > 0) pieces.push({ text: "\n", newline: true });
		// Ruby's grammar starts a heredoc's SQL right after `<<~SQL`, but
		// the rest of that line is still the call: the body starts below.
		let heredocOpened = false;
		for (const token of line) {
			for (const part of token.explanation ?? [{ content: token.content }]) {
				const scopes = (part.scopes ?? []).map((s) => s.scopeName);
				let kind = classify(scopes, part.content);
				if (heredocOpened) kind = "host";
				heredocOpened ||= scopes.some((s) =>
					s.startsWith("string.definition.begin"),
				);
				pieces.push({
					text: part.content,
					color: token.color,
					fontStyle: token.fontStyle,
					kind,
				});
			}
		}
	});

	// Runs of string body (newlines between them included) are candidates.
	const segments = [];
	let run = null;
	const flush = () => {
		if (!run) return;
		// A run may end in newlines that belong to the host again.
		const trailing = [];
		while (run.pieces.at(-1)?.newline) trailing.unshift(run.pieces.pop());
		const text = run.pieces.map((p) => p.text).join("");
		if (run.embedded || STATEMENT.test(text)) {
			segments.push({ sql: text });
		} else {
			segments.push(...run.pieces);
		}
		segments.push(...trailing);
		run = null;
	};
	for (const piece of pieces) {
		if (piece.kind === "sql" || piece.kind === "string") {
			run ??= { pieces: [], embedded: false };
			run.pieces.push(piece);
			run.embedded ||= piece.kind === "sql";
		} else if (piece.newline && run) {
			run.pieces.push(piece);
		} else {
			flush();
			segments.push(piece);
		}
	}
	flush();

	const sql = [];
	const plain = [];
	for (const segment of segments) {
		if (segment.sql === undefined) {
			sql.push(segment);
			plain.push(segment);
			continue;
		}
		plain.push({ text: segment.sql, color: colors.string });
		const lines = h.codeToTokens(segment.sql, {
			lang: "sql",
			theme: shikiTheme.name,
		}).tokens;
		lines.forEach((line, i) => {
			if (i > 0) sql.push({ text: "\n" });
			sql.push(...line.map((t) => ({ text: t.content, ...t })));
		});
	}
	return { sql: render(sql), plain: render(plain) };
}

// Where a statement starts: what makes a string body worth reading as SQL.
const STATEMENT =
	/^\s*(select|insert|update|delete|with|create|alter|drop|pragma|begin|merge|values|explain|truncate|grant|revoke|do|call|copy|vacuum|analyze)\b/i;

function classify(scopes, text) {
	const any = (test) => scopes.some(test);
	// An interpolation hole is host code, even inside a string.
	if (
		any(
			(s) =>
				s.startsWith("meta.embedded.line") ||
				s.startsWith("meta.template.expression") ||
				s.startsWith("meta.interpolation") ||
				s.startsWith("punctuation.section.embedded") ||
				s.startsWith("punctuation.definition.template-expression"),
		)
	) {
		return "host";
	}
	// A heredoc's opener and terminator (`<<~SQL`, `SQL`), which Ruby's
	// grammar scopes inside the SQL it injects.
	if (any((s) => s.startsWith("string.definition"))) return "host";
	// The SQL grammar's own tokens, where a host grammar injected it —
	// quotes of SQL string literals included.
	if (any((s) => s === "source.sql" || s.endsWith(".sql"))) return "sql";
	// The host string's own delimiters. Some grammars (Gleam's, Kotlin's)
	// scope their quotes as the string itself, so a piece that is nothing
	// but quotes counts too.
	if (
		any((s) => s.startsWith("punctuation.definition.string")) ||
		(/^["'`]+$/.test(text) &&
			!any((s) => s.startsWith("constant.character.escape")))
	) {
		return "host";
	}
	// C++ marks the whole of an `R"sql(…)sql"` string as `….raw.sql.cpp`.
	if (any((s) => /\.sql\./.test(s))) return "sql";
	return any((s) => s.startsWith("string.")) ? "string" : "host";
}

const escape = (text) =>
	text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");

function style({ color, fontStyle }) {
	const css = [];
	if (color) css.push(`color:${color}`);
	if (fontStyle & 1) css.push("font-style:italic");
	if (fontStyle & 2) css.push("font-weight:bold");
	if (fontStyle & 4) css.push("text-decoration:underline");
	return css.join(";");
}

// Pieces to shiki-shaped HTML: a pre, a code, and a span per line.
function render(pieces) {
	const lines = [[]];
	for (const piece of pieces) {
		piece.text.split("\n").forEach((text, i) => {
			if (i > 0) lines.push([]);
			if (text) lines.at(-1).push(`<span style="${style(piece)}">${escape(text)}</span>`);
		});
	}
	const body = lines
		.map((line) => `<span class="line">${line.join("")}</span>`)
		.join("\n");
	return `<pre class="shiki squill-dark" style="color:${colors.fg}"><code>${body}</code></pre>`;
}
