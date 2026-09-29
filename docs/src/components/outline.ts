// A docs page's outline: its h2 and h3 headings, read from the page's
// rendered HTML, each given an id to link to if it has none. So a page
// needs nothing of its own to get one.

export type Heading = {
	level: 2 | 3;
	id: string;
	// The heading's text as HTML: tags stripped, entities kept.
	label: string;
};

// Every <section> opening tag, and every h2 or h3 with its contents.
const TAGS = /<section\b([^>]*)>|<h([23])\b([^>]*)>([\s\S]*?)<\/h\2>/g;

// A heading that opens a <section id> links to the section (the anchor
// other pages already use, like `configuration/#rules`); any other gets
// its own id, from its text.
export function outline(html: string): { html: string; headings: Heading[] } {
	const taken = new Set(
		Array.from(html.matchAll(/\bid="([^"]*)"/g), (match) => match[1]),
	);
	const headings: Heading[] = [];
	// The id of the section just opened, until its heading claims it.
	let section: string | undefined;
	const rewritten = html.replace(
		TAGS,
		(
			tag: string,
			sectionAttrs: string | undefined,
			level: string | undefined,
			attrs: string,
			inner: string,
		) => {
			if (sectionAttrs !== undefined) {
				section = idOf(sectionAttrs);
				return tag;
			}
			const label = inner.replace(/<[^>]*>/g, "").trim();
			let id = idOf(attrs) ?? (level === "2" ? section : undefined);
			section = undefined;
			if (!id) {
				id = unique(slug(label), taken);
				tag = `<h${level} id="${id}"${attrs}>${inner}</h${level}>`;
			}
			headings.push({ level: level === "2" ? 2 : 3, id, label });
			return tag;
		},
	);
	return { html: rewritten, headings };
}

function idOf(attrs: string): string | undefined {
	return /\bid="([^"]*)"/.exec(attrs)?.[1];
}

// `How it works` → `how-it-works`; `[[files]]` → `files`.
function slug(label: string): string {
	return label
		.replace(/&[#\w]+;/g, "")
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, "-")
		.replace(/^-|-$/g, "");
}

function unique(slug: string, taken: Set<string>): string {
	const base = slug || "section";
	let id = base;
	for (let n = 2; taken.has(id); n++) {
		id = `${base}-${n}`;
	}
	taken.add(id);
	return id;
}
