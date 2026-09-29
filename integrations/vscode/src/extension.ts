// squill for VS Code: a thin client for squill's language server,
// `squill language-server start`.
//
// VS Code runs one formatter per document, so squill registers as the
// formatter only for SQL. In other languages it contributes its
// diagnostics about embedded SQL, a `source.formatSql` code action that
// runs on save after that language's own formatter, and highlighting
// for the embedded SQL (see ./highlight.ts).

import * as fs from "node:fs";
import * as path from "node:path";
import * as vscode from "vscode";
import { downloadedSquill } from "./download";
import { SqlHighlighter } from "./highlight";
import {
	LanguageClient,
	type LanguageClientOptions,
	type ServerOptions,
} from "vscode-languageclient/node";

// The languages squill has grammars for, plus SQL itself.
const LANGUAGES = [
	"sql",
	"rust",
	"go",
	"python",
	"javascript",
	"javascriptreact",
	"typescript",
	"typescriptreact",
	"gleam",
	"cpp",
	"csharp",
	"java",
	"kotlin",
	"swift",
];

let client: LanguageClient | undefined;
let highlighter: SqlHighlighter | undefined;
let extensionContext: vscode.ExtensionContext;

export async function activate(context: vscode.ExtensionContext) {
	extensionContext = context;
	context.subscriptions.push(
		vscode.commands.registerCommand("squill.restartServer", restart),
		vscode.workspace.onDidChangeConfiguration((event) => {
			if (event.affectsConfiguration("squill.path")) {
				void restart();
			} else if (event.affectsConfiguration("squill.highlightEmbeddedSql")) {
				updateHighlighter();
			}
		}),
	);
	await start();
}

export async function deactivate() {
	highlighter?.dispose();
	await client?.stop();
}

// Which squill: `squill.path` when set; else the `squill` on PATH, so
// the editor formats exactly as the CLI and CI do; else one downloaded
// from squill's latest release.
async function findSquill(): Promise<string | undefined> {
	const configured = vscode.workspace
		.getConfiguration("squill")
		.get<string>("path");
	if (configured) {
		return configured;
	}
	const executable = process.platform === "win32" ? "squill.exe" : "squill";
	for (const dir of (process.env.PATH ?? "").split(path.delimiter)) {
		if (dir && isExecutable(path.join(dir, executable))) {
			return path.join(dir, executable);
		}
	}
	return downloadedSquill(extensionContext);
}

function isExecutable(file: string): boolean {
	try {
		fs.accessSync(file, fs.constants.X_OK);
		return fs.statSync(file).isFile();
	} catch {
		return false;
	}
}

async function start() {
	let command: string | undefined;
	try {
		command = await findSquill();
	} catch (err) {
		const message = err instanceof Error ? err.message : String(err);
		void vscode.window.showErrorMessage(
			`squill isn't on your PATH, and downloading it failed: ${message}. Install it, or set squill.path to point at it.`,
		);
		return;
	}
	if (!command) {
		void vscode.window.showErrorMessage(
			"squill isn't on your PATH, and there's no prebuilt squill for this platform to download. Install it, or set squill.path to point at it.",
		);
		return;
	}
	const serverOptions: ServerOptions = {
		command,
		args: ["language-server", "start"],
		// Rules and ignore globs in a squill.toml are relative to it, but
		// anything given relative to the working directory resolves from
		// the workspace root, as it would in a terminal there.
		options: { cwd: vscode.workspace.workspaceFolders?.[0]?.uri.fsPath },
	};
	const clientOptions: LanguageClientOptions = {
		documentSelector: LANGUAGES.flatMap((language) => [
			{ scheme: "file", language },
			{ scheme: "untitled", language },
		]),
		initializationOptions: {
			formattingSelector: [{ language: "sql" }],
			// No semantic tokens anywhere: in SQL files VS Code's own
			// highlighting is fine, and elsewhere they'd replace the host
			// language server's. The highlighter asks for them itself.
			semanticTokensSelector: [],
		},
	};
	client = new LanguageClient("squill", "squill", serverOptions, clientOptions);
	try {
		await client.start();
		updateHighlighter();
	} catch (err) {
		client = undefined;
		const message = err instanceof Error ? err.message : String(err);
		void vscode.window.showErrorMessage(
			`squill couldn't start \`${command} language-server start\`: ${message}`,
		);
	}
}

// Highlight embedded SQL while the server runs, unless turned off.
function updateHighlighter() {
	const enabled = vscode.workspace
		.getConfiguration("squill")
		.get<boolean>("highlightEmbeddedSql", true);
	if (enabled && client && !highlighter) {
		highlighter = new SqlHighlighter(
			client,
			LANGUAGES.filter((language) => language !== "sql"),
		);
	} else if (!enabled && highlighter) {
		highlighter.dispose();
		highlighter = undefined;
	}
}

async function restart() {
	highlighter?.dispose();
	highlighter = undefined;
	await client?.stop();
	client = undefined;
	await start();
}
