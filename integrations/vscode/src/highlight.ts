// Highlighting for the SQL embedded in other languages' strings. VS Code
// takes one server's semantic tokens per document, and there the host
// language's server provides them, so squill's are asked for directly
// and drawn as decorations in the `squill.sql.*` theme colors.
//
// Only the SQL's words, numbers, parameters and comments are colored;
// its strings, operators and punctuation keep the host's string color.

import * as vscode from "vscode";
import {
	type LanguageClient,
	SemanticTokensRequest,
} from "vscode-languageclient/node";

// Each semantic token type squill draws, and the theme color it's drawn
// in (see `contributes.colors`).
const COLORS: Record<string, string> = {
	keyword: "squill.sql.keyword",
	variable: "squill.sql.name",
	function: "squill.sql.function",
	type: "squill.sql.type",
	number: "squill.sql.number",
	parameter: "squill.sql.parameter",
	comment: "squill.sql.comment",
};

// How long typing must pause before a document is highlighted again.
const DELAY_MS = 150;

type Highlights = { version: number; ranges: Map<string, vscode.Range[]> };

export class SqlHighlighter implements vscode.Disposable {
	private readonly decorations = new Map<
		string,
		vscode.TextEditorDecorationType
	>();
	// The last highlights of each document, by URI, to redraw an editor
	// that shows it again without asking squill.
	private readonly highlights = new Map<string, Highlights>();
	private readonly timers = new Map<string, NodeJS.Timeout>();
	private readonly subscriptions: vscode.Disposable[];
	private readonly legend: string[];

	constructor(
		private readonly client: LanguageClient,
		private readonly languages: string[],
	) {
		const provider =
			client.initializeResult?.capabilities.semanticTokensProvider;
		this.legend = provider?.legend.tokenTypes ?? [];
		for (const [type, color] of Object.entries(COLORS)) {
			this.decorations.set(
				type,
				vscode.window.createTextEditorDecorationType({
					color: new vscode.ThemeColor(color),
				}),
			);
		}
		this.subscriptions = [
			vscode.window.onDidChangeVisibleTextEditors((editors) => {
				for (const editor of editors) {
					this.show(editor.document);
				}
			}),
			vscode.workspace.onDidChangeTextDocument((event) => {
				if (event.contentChanges.length > 0) {
					this.schedule(event.document, DELAY_MS);
				}
			}),
			// A saved squill.toml (or query) can change what's SQL.
			vscode.workspace.onDidSaveTextDocument(() => {
				this.highlights.clear();
				for (const editor of vscode.window.visibleTextEditors) {
					this.show(editor.document);
				}
			}),
			vscode.workspace.onDidCloseTextDocument((document) => {
				const key = document.uri.toString();
				this.highlights.delete(key);
				clearTimeout(this.timers.get(key));
				this.timers.delete(key);
			}),
		];
		for (const editor of vscode.window.visibleTextEditors) {
			this.show(editor.document);
		}
	}

	dispose() {
		for (const subscription of this.subscriptions) {
			subscription.dispose();
		}
		for (const timer of this.timers.values()) {
			clearTimeout(timer);
		}
		// Disposing a decoration type removes it from every editor.
		for (const decoration of this.decorations.values()) {
			decoration.dispose();
		}
	}

	// Draw a document's highlights: the ones it has, if they're current,
	// or new ones.
	private show(document: vscode.TextDocument) {
		const known = this.highlights.get(document.uri.toString());
		if (known?.version === document.version) {
			this.draw(document, known.ranges);
		} else {
			this.schedule(document, 0);
		}
	}

	private schedule(document: vscode.TextDocument, delay: number) {
		if (!this.languages.includes(document.languageId)) {
			return;
		}
		const key = document.uri.toString();
		clearTimeout(this.timers.get(key));
		this.timers.set(
			key,
			setTimeout(() => {
				this.timers.delete(key);
				void this.refresh(document);
			}, delay),
		);
	}

	private async refresh(document: vscode.TextDocument) {
		const version = document.version;
		let tokens;
		try {
			tokens = await this.client.sendRequest(SemanticTokensRequest.type, {
				textDocument: { uri: document.uri.toString() },
			});
		} catch {
			// The server is stopping, or failed; its diagnostics say why.
			return;
		}
		// Edited while squill was answering: a newer request is coming.
		if (document.version !== version || document.isClosed) {
			return;
		}
		const ranges = this.decode(tokens?.data ?? []);
		this.highlights.set(document.uri.toString(), { version, ranges });
		this.draw(document, ranges);
	}

	// The ranges of each token type, from LSP's relative encoding.
	private decode(data: number[]): Map<string, vscode.Range[]> {
		const ranges = new Map<string, vscode.Range[]>();
		let line = 0;
		let character = 0;
		for (let at = 0; at + 4 < data.length; at += 5) {
			const [deltaLine, deltaStart, length, type] = data.slice(at, at + 4);
			if (deltaLine > 0) {
				character = 0;
			}
			line += deltaLine;
			character += deltaStart;
			const name = this.legend[type];
			if (!name || !this.decorations.has(name)) {
				continue;
			}
			let list = ranges.get(name);
			if (!list) {
				list = [];
				ranges.set(name, list);
			}
			list.push(new vscode.Range(line, character, line, character + length));
		}
		return ranges;
	}

	private draw(
		document: vscode.TextDocument,
		ranges: Map<string, vscode.Range[]>,
	) {
		for (const editor of vscode.window.visibleTextEditors) {
			if (editor.document !== document) {
				continue;
			}
			for (const [type, decoration] of this.decorations) {
				editor.setDecorations(decoration, ranges.get(type) ?? []);
			}
		}
	}
}
