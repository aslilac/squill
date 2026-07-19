import { defineConfig } from "astro/config";

// Local dev mounts at /, production build mounts under /squill so the same
// codebase can serve from mckayla.dev/squill without `astro dev` needing to
// run at /squill/ locally.
const isProd = process.env.NODE_ENV === "production";

export default defineConfig({
	site: "https://mckayla.dev",
	base: isProd ? "/squill" : "/",
	trailingSlash: "ignore",
});
