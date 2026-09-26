// Just enough of tar and zip to take one file out of a squill release
// archive, with Node's own zlib: no dependencies for a format this
// simple.

import * as zlib from "node:zlib";

// The contents of the entry named `name` in a gzipped tar archive.
export function fromTarGz(archive: Uint8Array, name: string): Buffer {
	const tar = zlib.gunzipSync(archive);
	let at = 0;
	// 512-byte headers, each followed by its entry's data padded to 512.
	while (at + 512 <= tar.length) {
		const header = tar.subarray(at, at + 512);
		if (header.every((byte) => byte === 0)) {
			break; // end-of-archive marker
		}
		const entry = cString(header.subarray(0, 100));
		const size = Number.parseInt(cString(header.subarray(124, 136)).trim(), 8);
		const data = at + 512;
		if (entry === name || entry === `./${name}`) {
			return tar.subarray(data, data + size);
		}
		at = data + Math.ceil(size / 512) * 512;
	}
	throw new Error(`no ${name} in the archive`);
}

// The contents of the entry named `name` in a zip archive.
export function fromZip(archive: Uint8Array, name: string): Buffer {
	const zip = Buffer.from(archive);
	// The end-of-central-directory record: the last 22 bytes, unless the
	// archive has a comment.
	let end = zip.length - 22;
	while (end >= 0 && zip.readUInt32LE(end) !== 0x06054b50) {
		end--;
	}
	if (end < 0) {
		throw new Error("not a zip archive");
	}
	const count = zip.readUInt16LE(end + 10);
	let at = zip.readUInt32LE(end + 16);
	for (let i = 0; i < count; i++) {
		if (zip.readUInt32LE(at) !== 0x02014b50) {
			throw new Error("corrupt zip central directory");
		}
		const method = zip.readUInt16LE(at + 10);
		const compressedSize = zip.readUInt32LE(at + 20);
		const nameLength = zip.readUInt16LE(at + 28);
		const extraLength = zip.readUInt16LE(at + 30);
		const commentLength = zip.readUInt16LE(at + 32);
		const localHeader = zip.readUInt32LE(at + 42);
		const entry = zip.toString("utf8", at + 46, at + 46 + nameLength);
		if (entry === name) {
			// The local header repeats the name and has its own extra field.
			const dataStart =
				localHeader +
				30 +
				zip.readUInt16LE(localHeader + 26) +
				zip.readUInt16LE(localHeader + 28);
			const data = zip.subarray(dataStart, dataStart + compressedSize);
			switch (method) {
				case 0:
					return data;
				case 8:
					return zlib.inflateRawSync(data);
				default:
					throw new Error(`unsupported zip compression method ${method}`);
			}
		}
		at += 46 + nameLength + extraLength + commentLength;
	}
	throw new Error(`no ${name} in the archive`);
}

// A NUL-terminated string field.
function cString(field: Uint8Array): string {
	const end = field.indexOf(0);
	return Buffer.from(end === -1 ? field : field.subarray(0, end)).toString(
		"utf8",
	);
}
