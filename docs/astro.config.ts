// Hosted at mckayla.dev/squill/, but using a base locally is inconvenient.
// Load-bearing trailing / btw (because we use it in a `<base>`)
const base = process.env.NODE_ENV === "production" ? "/squill/" : "/";

export default {
	site: "https://mckayla.dev",
	base,
	trailingSlash: "ignore",
	vite: {
		resolve: {
			tsconfigPaths: true,
		},
		// Pages import shiki in the browser only once they need it (a style
		// option changed), and it loads each language lazily too. Found that
		// late, the dev server re-bundles its dependencies mid-page, and the
		// import that found it fails; bundling it up front avoids that.
		optimizeDeps: {
			include: ["shiki"],
		},
	},
} satisfies import("astro").AstroUserConfig;
