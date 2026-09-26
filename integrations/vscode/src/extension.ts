// squill for VS Code: a thin client for `squill lsp`.
//
// VS Code runs one formatter per document, so squill registers as the
// formatter only for SQL. In other languages it contributes its
// diagnostics about embedded SQL, and a `source.formatSql` code action
// that runs on save after that language's own formatter.

import * as vscode from "vscode";
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
];

let client: LanguageClient | undefined;

export async function activate(context: vscode.ExtensionContext) {
	context.subscriptions.push(
		vscode.commands.registerCommand("squill.restartServer", restart),
		vscode.workspace.onDidChangeConfiguration((event) => {
			if (event.affectsConfiguration("squill.path")) {
				void restart();
			}
		}),
	);
	await start();
}

export async function deactivate() {
	await client?.stop();
}

async function start() {
	const command =
		vscode.workspace.getConfiguration("squill").get<string>("path") ||
		"squill";
	const serverOptions: ServerOptions = {
		command,
		args: ["lsp"],
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
		},
	};
	client = new LanguageClient("squill", "squill", serverOptions, clientOptions);
	try {
		await client.start();
	} catch (err) {
		client = undefined;
		const message = err instanceof Error ? err.message : String(err);
		void vscode.window.showErrorMessage(
			`squill couldn't start \`${command} lsp\`: ${message}. Is squill installed? Set squill.path to point at it.`,
		);
	}
}

async function restart() {
	await client?.stop();
	client = undefined;
	await start();
}
