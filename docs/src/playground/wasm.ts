// The squill wasm modules (crates/playground), behind a minimal WASI
// stub, shared by the playground and the docs' live recipes. No
// wasm-bindgen: JSON in, JSON out, through three exported functions.
//
// There's a module per grammar (scripts/build-wasm.sh), each carrying
// the whole formatter, so a page downloads only the grammar it formats:
// squill.wasm for plain SQL, squill-rust.wasm for Rust, and so on.

// A request to squill_format, as crates/playground's `Request` reads it.
export type Request = {
	source: string;
	/** `"sql"`, or a built-in grammar by its config name (`rust`, `c++`, …). */
	host: string;
	/** A tree-sitter query to find the SQL with, instead of the default. */
	query?: string | undefined;
	options?: RequestOptions;
};

export type RequestOptions = {
	dialect?: string;
	indent?: string;
	indent_width?: number;
	max_width?: number;
	keyword_case?: string;
	quote_idents?: string;
	trailing_semicolons?: string;
	at_params?: boolean;
	question_params?: boolean;
	colon_params?: boolean;
	pyformat_params?: boolean;
};

// crates/playground's `Response`, plus how long formatting took.
export type Response = {
	/** The formatted text; null on a hard error, with only diagnostics. */
	output: string | null;
	diagnostics: string[];
	/** Where the SQL is in `output`, as `[start, end)` byte offsets. */
	spans: Span[];
	ms: number;
};

export type Span = [start: number, end: number];

export type Format = (request: Request) => Response;

// What crates/playground exports, besides WASI's `_initialize`.
type Exports = {
	memory: WebAssembly.Memory;
	_initialize?: () => void;
	squill_alloc: (len: number) => number;
	squill_dealloc: (ptr: number, len: number) => void;
	/** The response's pointer in the high 32 bits, its length in the low. */
	squill_format: (ptr: number, len: number) => bigint;
};

const loading = new Map<string, Promise<Format>>();

function moduleFor(host: string): string {
	switch (host) {
		case "sql":
			return "squill";
		case "tsx":
		case "typescript":
			return "squill-typescript";
		case "c++":
			return "squill-cxx";
		case "c#":
			return "squill-csharp";
		default:
			return `squill-${host}`;
	}
}

// The module reports byte offsets (a response's `spans`); JS strings
// count UTF-16 units. Converts `[start, end]` pairs into `text`.
export function charSpans(text: string, spans: Span[]): Span[] {
	const bytes = new TextEncoder().encode(text);
	const decoder = new TextDecoder();
	const at = (byte: number) =>
		decoder.decode(bytes.subarray(0, Math.min(byte, bytes.length))).length;
	return spans.map(([start, end]) => [at(start), at(end)]);
}

// Load `host`'s module, once per page; resolves to `format(request)`,
// which returns the parsed response plus how long formatting took (`ms`).
export function loadSquill(host: string): Promise<Format> {
	const module = moduleFor(host);
	let format = loading.get(module);
	if (!format) {
		const compiled = WebAssembly.compileStreaming(fetch(`wasm/${module}.wasm`));
		format = compiled.then(instantiate);
		loading.set(module, format);
	}
	return format;
}

// A compiled module's `format(request)`; also used at build time, in
// Node, with the module read from disk.
export async function instantiate(module: WebAssembly.Module): Promise<Format> {
	let memory: WebAssembly.Memory;
	const dataView = () => new DataView(memory.buffer);
	const handlers: Record<string, (...args: never[]) => number> = {
		fd_write: (_fd: number, _iovs: number, _n: number, out: number) => {
			dataView().setUint32(out, 0, true);
			return 0;
		},
		random_get: (ptr: number, len: number) => {
			crypto.getRandomValues(new Uint8Array(memory.buffer, ptr, len));
			return 0;
		},
		clock_time_get: (_id: number, _precision: bigint, out: number) => {
			dataView().setBigUint64(out, BigInt(Date.now()) * 1000000n, true);
			return 0;
		},
		environ_sizes_get: (c: number, s: number) => {
			dataView().setUint32(c, 0, true);
			dataView().setUint32(s, 0, true);
			return 0;
		},
		environ_get: () => 0,
		args_sizes_get: (c: number, s: number) => {
			dataView().setUint32(c, 0, true);
			dataView().setUint32(s, 0, true);
			return 0;
		},
		args_get: () => 0,
		sched_yield: () => 0,
		proc_exit: (code: number) => {
			throw new Error("wasm exited: " + code);
		},
	};

	const wasi: Record<string, (...args: never[]) => number> = {};
	for (const imp of WebAssembly.Module.imports(module)) {
		if (imp.module !== "wasi_snapshot_preview1") continue;
		wasi[imp.name] = handlers[imp.name] ?? (() => 8); // default: WASI EBADF
	}
	const instance = await WebAssembly.instantiate(module, {
		wasi_snapshot_preview1: wasi,
	});
	const exports = instance.exports as unknown as Exports;
	memory = exports.memory;
	exports._initialize?.();

	return (request) => {
		const bytes = new TextEncoder().encode(JSON.stringify(request));
		const ptr = exports.squill_alloc(bytes.length);
		new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
		const start = performance.now();
		const packed = exports.squill_format(ptr, bytes.length);
		const ms = performance.now() - start;
		exports.squill_dealloc(ptr, bytes.length);
		const outPtr = Number(packed >> 32n);
		const outLen = Number(packed & 0xffffffffn);
		const text = new TextDecoder().decode(
			new Uint8Array(memory.buffer, outPtr, outLen),
		);
		exports.squill_dealloc(outPtr, outLen);
		return { ms, ...JSON.parse(text) };
	};
}
