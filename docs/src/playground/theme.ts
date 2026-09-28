// Brand colors and the shiki textmate theme built from them, shared by
// the playground (Monaco + live shiki) and the landing page's
// build-time highlighting.

export const colors = {
	bg: "#1c1824",
	bgDeep: "#191520",
	fg: "#cfc7db",
	keyword: "#a58fe0",
	string: "#63b98a",
	number: "#d9a05c",
	comment: "#6f6878",
	func: "#7ba3e0",
	selection: "#3a3348",
} as const;

// The light theme's, for the playground's editor: the light halves of
// styles/theme.css's code variables.
export const lightColors = {
	bg: "#eeeaf4",
	bgDeep: "#e9e5f0",
	fg: "#3a3246",
	keyword: "#6b4fc8",
	string: "#23794d",
	number: "#a15c12",
	comment: "#8a8296",
	func: "#3565b0",
	selection: "#d9d0ec",
};

// The site's panes follow its light or dark theme, so there the palette
// is CSS variables (styles/theme.css gives each a light and a dark value).
// shiki only takes literal colors, so highlight with the dark theme and
// swap each of its colors for its variable afterwards.
const variables = {
	fg: "--code-fg",
	keyword: "--code-keyword",
	string: "--code-string",
	number: "--code-number",
	comment: "--code-comment",
	func: "--code-func",
} as const;

const byHex = new Map(
	Object.entries(variables).map(([name, variable]) => [
		colors[name as keyof typeof variables].toLowerCase(),
		`var(${variable})`,
	]),
);

export function themed(html: string): string {
	return html.replace(
		/#[0-9a-f]{6}\b/gi,
		(hex) => byHex.get(hex.toLowerCase()) ?? hex,
	);
}

export const shikiTheme = {
	name: "squill-dark",
	type: "dark",
	colors: {
		"editor.background": colors.bgDeep,
		"editor.foreground": colors.fg,
	},
	settings: [
		{ settings: { background: colors.bgDeep, foreground: colors.fg } },
		{
			scope: ["keyword", "storage"],
			settings: { foreground: colors.keyword, fontStyle: "bold" },
		},
		{ scope: ["string"], settings: { foreground: colors.string } },
		{
			scope: ["constant.numeric"],
			settings: { foreground: colors.number },
		},
		{ scope: ["comment"], settings: { foreground: colors.comment } },
		{
			scope: ["entity.name.function", "support.function"],
			settings: { foreground: colors.func },
		},
	],
};
