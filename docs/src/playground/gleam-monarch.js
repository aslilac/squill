// Monaco has no built-in Gleam support; a small Monarch grammar covers
// the input-editor basics (keywords, strings, numbers, comments, types).

export const gleamConfiguration = {
	comments: { lineComment: "//" },
	brackets: [
		["{", "}"],
		["[", "]"],
		["(", ")"],
	],
	autoClosingPairs: [
		{ open: "{", close: "}" },
		{ open: "[", close: "]" },
		{ open: "(", close: ")" },
		{ open: '"', close: '"' },
	],
	surroundingPairs: [
		{ open: "{", close: "}" },
		{ open: "[", close: "]" },
		{ open: "(", close: ")" },
		{ open: '"', close: '"' },
	],
};

export const gleamLanguage = {
	defaultToken: "",
	keywords: [
		"as",
		"assert",
		"case",
		"const",
		"echo",
		"else",
		"fn",
		"if",
		"import",
		"let",
		"opaque",
		"panic",
		"pub",
		"todo",
		"type",
		"use",
	],
	tokenizer: {
		root: [
			[/\/\/\/?.*$/, "comment"],
			[/"(?:[^"\\]|\\.)*"/, "string"],
			[/\b0[bB][01_]+\b/, "number"],
			[/\b0[xX][0-9a-fA-F_]+\b/, "number"],
			[/\b\d[\d_]*(\.\d+)?(e-?\d+)?\b/, "number"],
			[/\b[A-Z][A-Za-z0-9]*\b/, "type.identifier"],
			[
				/\b[a-z_][a-z0-9_]*\b/,
				{ cases: { "@keywords": "keyword", "@default": "identifier" } },
			],
			[/\|>|<-|->|<>|[=<>!+\-*/%]=?|\.\.|\|/, "operator"],
		],
	},
};
