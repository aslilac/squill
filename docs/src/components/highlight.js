import { createHighlighter } from "shiki";
import { sqlGrammar } from "../playground/sql-grammar.js";
import { colors, shikiTheme, themed } from "../playground/theme.js";

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
		langs: [queryGrammar, sqlGrammar],
	});
	const h = await highlighter;
	if (!h.getLoadedLanguages().includes(lang)) await h.loadLanguage(lang);
	return h;
}

export async function highlight(code, lang) {
	const h = await loaded(lang);
	return themed(h.codeToHtml(code, { lang, theme: shikiTheme.name }));
}

// Host code with SQL in its strings, twice over: once with the SQL
// highlighted as SQL, once with it colored as the plain string it is.
// `spans` are where the SQL is — squill's own answer, from `squill
// locate` — as [start, end) offsets into `code`. Inside one, everything
// but an interpolation hole is SQL; outside, nothing is, even where a
// host grammar (shiki's Ruby and C++) would inject SQL colors itself.
export async function highlightHost(code, lang, spans) {
	const h = await loaded(lang);
	const { tokens } = h.codeToTokens(code, {
		lang,
		theme: shikiTheme.name,
		includeExplanation: true,
	});

	// Scoped pieces with their offsets, line breaks included.
	const pieces = [];
	let offset = 0;
	tokens.forEach((line, i) => {
		if (i > 0) {
			pieces.push({ text: "\n", offset, kind: "host" });
			offset += 1;
		}
		// Ruby's grammar starts a heredoc's SQL right after `<<~SQL`, but
		// the rest of that line is still the call: plain host code.
		let heredocOpened = false;
		for (const token of line) {
			for (const part of token.explanation ?? [{ content: token.content }]) {
				const scopes = (part.scopes ?? []).map((s) => s.scopeName);
				const piece = {
					text: part.content,
					offset,
					color: token.color,
					fontStyle: token.fontStyle,
					kind: classify(scopes),
				};
				if (heredocOpened) {
					Object.assign(piece, {
						kind: "host",
						color: colors.fg,
						fontStyle: 0,
					});
				}
				heredocOpened ||= scopes.some((s) =>
					s.startsWith("string.definition.begin"),
				);
				pieces.push(piece);
				offset += part.content.length;
			}
		}
	});

	// Cut pieces at span edges, so each lies wholly inside or outside.
	const edges = [...new Set(spans.flat())].sort((a, b) => a - b);
	const cut = pieces.flatMap((piece) => {
		const end = piece.offset + piece.text.length;
		const inner = edges.filter((edge) => edge > piece.offset && edge < end);
		let from = piece.offset;
		return [...inner, end].map((to) => {
			const part = {
				...piece,
				text: piece.text.slice(from - piece.offset, to - piece.offset),
				offset: from,
			};
			from = to;
			return part;
		});
	});
	const spanAt = (at) =>
		spans.findIndex(([start, end]) => at >= start && at < end);

	const sql = [];
	const plain = [];
	let run = null;
	const flush = () => {
		if (!run) return;
		plain.push({ text: run.text, color: colors.string });
		const lines = h.codeToTokens(run.text, {
			lang: "sql",
			theme: shikiTheme.name,
		}).tokens;
		lines.forEach((line, i) => {
			if (i > 0) sql.push({ text: "\n" });
			sql.push(...line.map((t) => ({ ...t, text: t.content })));
		});
		run = null;
	};
	for (const piece of cut) {
		const span = spanAt(piece.offset);
		if (span >= 0 && piece.kind !== "hole") {
			if (run && run.span !== span) flush();
			run ??= { span, text: "" };
			run.text += piece.text;
			continue;
		}
		flush();
		// SQL a host grammar injected where squill finds none is, to
		// squill, just a string.
		const shown =
			piece.kind === "injected"
				? { text: piece.text, color: colors.string }
				: piece;
		sql.push(shown);
		plain.push(shown);
	}
	flush();
	return { sql: render(sql), plain: render(plain) };
}

function classify(scopes) {
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
		return "hole";
	}
	if (any((s) => s === "source.sql" || /\.sql(\.|$)/.test(s))) {
		return "injected";
	}
	return "host";
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
			if (text)
				lines
					.at(-1)
					.push(`<span style="${style(piece)}">${escape(text)}</span>`);
		});
	}
	const body = lines
		.map((line) => `<span class="line">${line.join("")}</span>`)
		.join("\n");
	return themed(
		`<pre class="shiki squill-dark" style="color:${colors.fg}"><code>${body}</code></pre>`,
	);
}
