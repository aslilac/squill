// Recipe view menus, and live recipes: a recipe whose grammar the
// playground's wasm module has carries its source, config, and options
// (Recipe.astro's `data-live`). Its menu reformats the after pane with
// other style options, and
// writes the keys that would do it into the config pane. The choices are
// page-wide — every live recipe on the page follows them — and last
// until the page reloads, so the docs always open on the defaults.
import { loadSquill } from "../playground/wasm.js";

// The style options changed from the recipe's own, by request key.
const overrides = {};
const numeric = new Set(["indent_width", "max_width"]);

const recipes = [...document.querySelectorAll(".recipe[data-live]")].map(
	(root) => {
		const after = root.querySelector('[data-pane="after"]');
		const config = root.querySelector('[data-file="squill.toml"] > div');
		return {
			root,
			live: JSON.parse(root.dataset.live),
			after,
			config,
			// What the page was built with: shown again when nothing is
			// changed.
			original: {
				sql: after.querySelector(".sql-on").innerHTML,
				plain: after.querySelector(".sql-off").innerHTML,
				config: config?.innerHTML,
			},
			version: 0,
		};
	},
);

// Every recipe's view menu: the popover sits in the top layer, out of
// the pane that would clip it; place it under its gear, right edges
// aligned.
for (const root of document.querySelectorAll(".recipe")) {
	const gear = root.querySelector(".recipe-gear");
	const popover = root.querySelector(".recipe-popover");
	if (!gear || !popover) continue;
	popover.addEventListener("toggle", (event) => {
		gear.setAttribute("aria-expanded", String(event.newState === "open"));
		if (event.newState !== "open") return;
		const at = gear.getBoundingClientRect();
		const left = Math.max(8, at.right + scrollX - popover.offsetWidth);
		popover.style.top = `${at.bottom + scrollY + 6}px`;
		popover.style.left = `${left}px`;
	});
}

for (const recipe of recipes) {
	const { root } = recipe;
	const popover = root.querySelector(".recipe-popover");
	for (const select of popover.querySelectorAll("select[data-option]")) {
		select.addEventListener("change", () => {
			const key = select.dataset.option;
			if (select.value === "") delete overrides[key];
			else overrides[key] = numeric.has(key) ? Number(select.value) : select.value;
			syncMenus();
			showAfter(root);
			for (const each of recipes) update(each);
		});
	}
	popover.querySelector(".live-reset").addEventListener("click", () => {
		for (const key of Object.keys(overrides)) delete overrides[key];
		syncMenus();
		for (const each of recipes) update(each);
	});
}

// Style options change the after pane: show it, if the before tab is up.
function showAfter(root) {
	const tabs = root.querySelector("[data-tabs]");
	if (tabs.dataset.current !== "after") {
		tabs.querySelector('[data-tab="after"]').click();
	}
}

function syncMenus() {
	for (const select of document.querySelectorAll(
		".recipe-popover select[data-option]",
	)) {
		const value = overrides[select.dataset.option];
		select.value = value === undefined ? "" : String(value);
	}
}

function setStatus(recipe, text) {
	recipe.root.querySelector(".live-status").textContent = text;
}

async function update(recipe) {
	const version = ++recipe.version;
	const { live, after, config, original } = recipe;
	const sqlPane = after.querySelector(".sql-on");
	const plainPane = after.querySelector(".sql-off");
	if (Object.keys(overrides).length === 0) {
		sqlPane.innerHTML = original.sql;
		plainPane.innerHTML = original.plain;
		if (config) config.innerHTML = original.config;
		setStatus(recipe, "");
		return;
	}

	setStatus(recipe, "loading the formatter…");
	const [format, { highlight, highlightHost }] = await Promise.all([
		loadSquill(),
		import("./highlight.js"),
	]);
	const result = format({
		source: live.source,
		host: live.host,
		query: live.query,
		options: { ...live.options, ...overrides },
	});
	if (result.output == null) {
		if (version === recipe.version) {
			setStatus(recipe, result.diagnostics.join("\n") || "formatting failed");
		}
		return;
	}
	const text = result.output.trimEnd();
	let html;
	if (live.host === "sql") {
		const sql = await highlight(text, "sql");
		html = { sql, plain: sql };
	} else {
		html = await highlightHost(text, live.lang, charSpans(text, result.spans));
	}
	const configHtml =
		config && (await highlight(configWith(live, overrides).trimEnd(), "toml"));
	// A later change has already started; its result wins.
	if (version !== recipe.version) return;
	sqlPane.innerHTML = html.sql;
	plainPane.innerHTML = html.plain;
	if (config) config.innerHTML = configHtml;
	setStatus(recipe, "");
}

// The wasm module reports byte offsets; the highlighter counts UTF-16
// units.
function charSpans(text, spans) {
	const bytes = new TextEncoder().encode(text);
	const decoder = new TextDecoder();
	const at = (byte) =>
		decoder.decode(bytes.subarray(0, Math.min(byte, bytes.length))).length;
	return spans.map(([start, end]) => [at(start), at(end)]);
}

// The recipe's config with the overrides written in: into the rule that
// covers the recipe's file (or at the top, for a config without one),
// replacing a key the config already sets.
function configWith(live, overrides) {
	const lines = live.config.replace(/\n$/, "").split("\n");
	const isHeader = (line) => line.trimStart().startsWith("[");
	let start = 0;
	if (live.rule) {
		const header = `[[${live.rule.kind}]]`;
		let seen = -1;
		start = lines.findIndex(
			(line) => line.trim() === header && ++seen === live.rule.index,
		);
		start += 1;
	}
	let end = lines.findIndex((line, i) => i >= start && isHeader(line));
	if (end === -1) end = lines.length;
	let insert = end;
	while (insert > start && lines[insert - 1].trim() === "") insert -= 1;
	for (const [option, value] of Object.entries(overrides)) {
		const key = option.replaceAll("_", "-");
		const line = `${key} = ${typeof value === "number" ? value : `"${value}"`}`;
		const existing = lines.findIndex(
			(text, i) => i >= start && i < end && new RegExp(`^\\s*${key}\\s*=`).test(text),
		);
		if (existing >= 0) {
			lines[existing] = line;
		} else {
			lines.splice(insert, 0, line);
			insert += 1;
			end += 1;
		}
	}
	return lines.join("\n") + "\n";
}
