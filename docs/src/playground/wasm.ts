// The squill wasm modules (crates/playground), behind a minimal WASI
// stub, shared by the playground and the docs' live recipes. No
// wasm-bindgen: JSON in, JSON out, through three exported functions.
//
// There's a module per grammar (scripts/build-wasm.sh), each carrying
// the whole formatter, so a page downloads only the grammar it formats:
// squill.wasm for plain SQL, squill-rust.wasm for Rust, and so on.

const loading = new Map();

function moduleFor(host: string): string {
	switch (host) {
		case "sql":
			return "squill";
		case "tsx":
		case "typescript":
			return "squill-typescript";
		default:
			return `squill-${host}`;
	}
}

// The module reports byte offsets (a response's `spans`); JS strings
// count UTF-16 units. Converts `[start, end]` pairs into `text`.
export function charSpans(text: string, spans: any[]) {
	const bytes = new TextEncoder().encode(text);
	const decoder = new TextDecoder();
	const at = (byte: number) =>
		decoder.decode(bytes.subarray(0, Math.min(byte, bytes.length))).length;
	return spans.map(([start, end]) => [at(start), at(end)]);
}

// Load `host`'s module, once per page; resolves to `format(request)`,
// which returns the parsed response plus how long formatting took (`ms`).
export function loadSquill(host: string) {
	const module = moduleFor(host);
	if (!loading.has(module)) {
		const compiled = WebAssembly.compileStreaming(fetch(`wasm/${module}.wasm`));
		loading.set(module, compiled.then(instantiate));
	}
	return loading.get(module);
}

// A compiled module's `format(request)`; also used at build time, in
// Node, with the module read from disk.
export async function instantiate(module) {
	let memory;
	const dataView = () => new DataView(memory.buffer);
	const handlers = {
		fd_write: (fd, iovs, n, out) => {
			dataView().setUint32(out, 0, true);
			return 0;
		},
		random_get: (ptr, len) => {
			crypto.getRandomValues(new Uint8Array(memory.buffer, ptr, len));
			return 0;
		},
		clock_time_get: (id, prec, out) => {
			dataView().setBigUint64(out, BigInt(Date.now()) * 1000000n, true);
			return 0;
		},
		environ_sizes_get: (c, s) => {
			dataView().setUint32(c, 0, true);
			dataView().setUint32(s, 0, true);
			return 0;
		},
		environ_get: () => 0,
		args_sizes_get: (c, s) => {
			dataView().setUint32(c, 0, true);
			dataView().setUint32(s, 0, true);
			return 0;
		},
		args_get: () => 0,
		sched_yield: () => 0,
		proc_exit: (code) => {
			throw new Error("wasm exited: " + code);
		},
	};

	const wasi = {};
	for (const imp of WebAssembly.Module.imports(module)) {
		if (imp.module !== "wasi_snapshot_preview1") continue;
		wasi[imp.name] = handlers[imp.name] ?? (() => 8); // default: WASI EBADF
	}
	const instance = await WebAssembly.instantiate(module, {
		wasi_snapshot_preview1: wasi,
	});
	memory = instance.exports.memory;
	instance.exports._initialize?.();

	return (request) => {
		const bytes = new TextEncoder().encode(JSON.stringify(request));
		const ptr = instance.exports.squill_alloc(bytes.length);
		new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
		const start = performance.now();
		const packed = instance.exports.squill_format(ptr, bytes.length);
		const ms = performance.now() - start;
		instance.exports.squill_dealloc(ptr, bytes.length);
		const outPtr = Number(packed >> 32n);
		const outLen = Number(packed & 0xffffffffn);
		const text = new TextDecoder().decode(
			new Uint8Array(memory.buffer, outPtr, outLen),
		);
		instance.exports.squill_dealloc(outPtr, outLen);
		return { ms, ...JSON.parse(text) };
	};
}
