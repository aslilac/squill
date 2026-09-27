// squill itself, at build time: the plain-SQL wasm module the browser
// loads (public/wasm/squill.wasm, from scripts/build-wasm.sh), run in Node,
// so a page's examples are always what squill does today.
import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { instantiate } from "../playground/wasm.js";

let loading;

// Format `source` as plain SQL with `options` (request keys, as the
// playground sends them). An example squill can't format cleanly fails
// the build rather than showing something squill wouldn't print.
export async function formatSql(source, options = {}) {
	loading ??= readFile(join(process.cwd(), "public/wasm/squill.wasm"))
		.then((bytes) => WebAssembly.compile(bytes))
		.then(instantiate);
	const format = await loading;
	const result = format({ source, host: "sql", options });
	if (result.output == null || result.diagnostics.length > 0) {
		throw new Error(
			`squill couldn't format an example:\n${source}\n${result.diagnostics.join("\n")}`,
		);
	}
	return result.output;
}
