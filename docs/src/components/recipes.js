// The docs recipes (docs/recipes/<name>), read at build time: what a
// recipe's config says about its host file, for the live recipes on the
// recipe pages and the playground's framework picker.
import { readFileSync, readdirSync } from "node:fs";
import { join, resolve } from "node:path";
import { parse as parseToml } from "smol-toml";

// Astro runs from docs/, next to the recipes.
const recipesDir = resolve(process.cwd(), "recipes");

// Every grammar with a playground wasm module (scripts/build-wasm.sh):
// the built-ins, not grammars loaded from a URL.
export const LIVE_HOSTS = [
	"rust",
	"go",
	"python",
	"javascript",
	"typescript",
	"tsx",
	"gleam",
	"cpp",
	"csharp",
	"java",
	"kotlin",
	"swift",
];

// The request options a config can set, as squill.toml spells them.
const KEYS = [
	"dialect",
	"indent",
	"indent-width",
	"max-width",
	"keyword-case",
	"quote-idents",
	"trailing-semicolons",
	"at-params",
	"question-params",
	"colon-params",
	"pyformat-params",
];

// A recipe the playground's modules can format: its host, the query and
// options its config gives the rule that covers `before`, and `before`
// itself. Null for a recipe without a config, or whose grammar is loaded
// from a URL.
export function liveRecipe(name) {
	const dir = join(recipesDir, name);
	const read = (path) => readFileSync(join(dir, path), "utf8");
	const all = readdirSync(dir);
	const before = all.find((file) => file.startsWith("before."));
	if (!all.includes("squill.toml") || !before) return null;
	const configText = read("squill.toml");
	const config = parseToml(configText);
	const extension = before.split(".").pop();
	const kind = extension === "sql" ? "files" : "embedded";
	const rules = config[kind] ?? [];
	const index = rules.findIndex((rule) =>
		rule.include?.some((glob) => glob.endsWith(`.${extension}`)),
	);
	const rule = rules[index] ?? {};
	const host = extension === "sql" ? "sql" : rule.grammar;
	if (host !== "sql" && !LIVE_HOSTS.includes(host)) return null;
	// Request options, top level first and the rule over it, as squill
	// layers them.
	const options = {};
	for (const layer of [config, rule]) {
		for (const key of KEYS) {
			if (layer[key] !== undefined)
				options[key.replaceAll("-", "_")] = layer[key];
		}
	}
	// The dialect squill reads before's SQL in, from `squill locate`
	// (spans.jsonl): the config's, or one the query pins (@sql.sqlite).
	const dialects = new Set(
		read("spans.jsonl")
			.split("\n")
			.filter(Boolean)
			.map((line) => JSON.parse(line))
			.filter((span) => span.path === before)
			.map((span) => span.dialect),
	);
	return {
		host,
		query: rule.query ? read(rule.query) : undefined,
		options,
		dialect: dialects.size === 1 ? [...dialects][0] : undefined,
		source: read(before),
		config: configText,
		// Where new keys go in the config: the rule's table, or the top.
		rule: index >= 0 ? { kind, index } : null,
	};
}

// Each recipe's title and page: the heading it sits under on its
// language's recipe page (src/pages/docs/recipes/<page>.astro), in page
// order.
export function recipeTitles() {
	const pagesDir = resolve(process.cwd(), "src/pages/docs/recipes");
	const titles = [];
	for (const file of readdirSync(pagesDir).sort()) {
		const page = file.replace(/\.astro$/, "");
		const source = readFileSync(join(pagesDir, file), "utf8");
		for (const match of source.matchAll(/<Recipe name="([^"]+)"/g)) {
			const headings = [
				...source
					.slice(0, match.index)
					.matchAll(/<h[23][^>]*>(.*?)<\/h[23]>/gs),
			];
			const heading = headings.at(-1)?.[1] ?? match[1];
			titles.push({
				name: match[1],
				page,
				title: heading.replace(/<[^>]+>/g, "").trim(),
			});
		}
	}
	return titles;
}
