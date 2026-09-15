// Hosted at mckayla.dev/squill/, but using a base locally is inconvenient.
// Load-bearing trailing / btw (because we use it in a `<base>`)
const base = process.env.NODE_ENV === "production" ? "/squill/" : "/";

export default {
	site: "https://mckayla.dev",
	base,
	trailingSlash: "ignore",
} satisfies import("astro").AstroUserConfig;
