// Fetching squill from its latest GitHub release, for when it isn't
// installed. Downloads go in the extension's global storage, one
// directory per release; each archive must match the SHA-256 digest
// GitHub reports for the asset, or it isn't installed.

import * as crypto from "node:crypto";
import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";
import { fromTarGz, fromZip } from "./archive";

const REPO = "aslilac/squill";

// Look for a newer release at most this often.
const CHECK_INTERVAL = 24 * 60 * 60 * 1000;

// What the last check found, in the extension's global state.
const STATE_KEY = "squill.download";

interface State {
	tag: string;
	checkedAt: number;
}

interface Release {
	tag_name: string;
	assets: Array<{
		name: string;
		browser_download_url: string;
		// `sha256:<hex>`, computed by GitHub when the asset was uploaded.
		digest?: string | null;
	}>;
}

// This platform's release archive: the target in its name, and what's
// inside.
function platform():
	{ target: string; zip: boolean; executable: string } | undefined {
	const targets: Record<string, string> = {
		"darwin-arm64": "aarch64-apple-darwin",
		"linux-x64": "x86_64-unknown-linux-gnu",
		"linux-arm64": "aarch64-unknown-linux-gnu",
		"win32-x64": "x86_64-pc-windows-msvc",
		"win32-arm64": "aarch64-pc-windows-msvc",
	};
	const target = targets[`${process.platform}-${process.arch}`];
	if (!target) {
		return undefined;
	}
	const windows = process.platform === "win32";
	return {
		target,
		zip: windows,
		executable: windows ? "squill.exe" : "squill",
	};
}

// The downloaded squill, fetching or updating it as needed. Undefined
// when there's no build for this platform; throws when a download fails
// and there's nothing already downloaded to fall back on.
export async function downloadedSquill(
	context: vscode.ExtensionContext,
): Promise<string | undefined> {
	const target = platform();
	if (!target) {
		return undefined;
	}
	const root = context.globalStorageUri.fsPath;
	const state = context.globalState.get<State>(STATE_KEY);
	const installed = state
		? path.join(root, `squill-${state.tag}`, target.executable)
		: undefined;
	const haveInstalled = installed !== undefined && fs.existsSync(installed);
	if (haveInstalled && Date.now() - state!.checkedAt < CHECK_INTERVAL) {
		return installed;
	}

	let release: Release;
	try {
		release = await latestRelease();
	} catch (err) {
		// Offline, or GitHub is unhappy: keep what we have.
		if (haveInstalled) {
			return installed;
		}
		throw err;
	}
	const tag = release.tag_name;
	const dir = path.join(root, `squill-${tag}`);
	const binary = path.join(dir, target.executable);
	if (!fs.existsSync(binary)) {
		const archiveName = `squill-${tag}-${target.target}.${target.zip ? "zip" : "tar.gz"}`;
		const archive = release.assets.find((asset) => asset.name === archiveName);
		const expected = archive?.digest?.startsWith("sha256:")
			? archive.digest.slice("sha256:".length).toLowerCase()
			: undefined;
		if (!archive || !expected) {
			if (haveInstalled) {
				return installed;
			}
			throw new Error(
				`squill ${tag} has no ${archive ? "SHA-256 digest for " : ""}${archiveName}`,
			);
		}
		const install = vscode.window.withProgress(
			{
				location: vscode.ProgressLocation.Notification,
				title: `Downloading squill ${tag}`,
			},
			async () => {
				const bytes = await fetchBytes(archive.browser_download_url);
				const actual = crypto.createHash("sha256").update(bytes).digest("hex");
				if (actual !== expected) {
					throw new Error(
						`${archiveName} doesn't match the digest GitHub reports for it`,
					);
				}
				const contents = target.zip
					? fromZip(bytes, target.executable)
					: fromTarGz(bytes, target.executable);
				fs.mkdirSync(dir, { recursive: true });
				// Written aside, then moved into place: a half-written binary
				// is never mistaken for an installed one.
				const partial = `${binary}.partial`;
				fs.writeFileSync(partial, contents, { mode: 0o755 });
				fs.renameSync(partial, binary);
			},
		);
		try {
			await install;
		} catch (err) {
			if (!haveInstalled) {
				throw err;
			}
			// An update that fails leaves the working copy in use; try again
			// at the next check.
			const message = err instanceof Error ? err.message : String(err);
			void vscode.window.showWarningMessage(
				`Couldn't update squill to ${tag}: ${message}. Still using ${state!.tag}.`,
			);
			await context.globalState.update(STATE_KEY, {
				tag: state!.tag,
				checkedAt: Date.now(),
			} satisfies State);
			return installed;
		}
		// Only the release in use is kept.
		for (const entry of fs.readdirSync(root)) {
			if (entry.startsWith("squill-") && entry !== `squill-${tag}`) {
				fs.rmSync(path.join(root, entry), { recursive: true, force: true });
			}
		}
	}
	await context.globalState.update(STATE_KEY, {
		tag,
		checkedAt: Date.now(),
	} satisfies State);
	return binary;
}

async function latestRelease(): Promise<Release> {
	const response = await fetch(
		`https://api.github.com/repos/${REPO}/releases/latest`,
		{
			headers: {
				"Accept": "application/vnd.github+json",
				"User-Agent": "squill-vscode",
			},
		},
	);
	if (!response.ok) {
		throw new Error(
			`looking up squill's latest release: GitHub said ${response.status}`,
		);
	}
	return (await response.json()) as Release;
}

async function fetchBytes(url: string): Promise<Uint8Array> {
	const response = await fetch(url, {
		headers: { "User-Agent": "squill-vscode" },
	});
	if (!response.ok) {
		throw new Error(`downloading ${url}: ${response.status}`);
	}
	return new Uint8Array(await response.arrayBuffer());
}
