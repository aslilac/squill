// Offer to turn on `source.formatSql` on save. VS Code runs one formatter
// per file, so squill formats a language's embedded SQL with that code
// action instead, which does nothing until settings ask for it on save.
// When the workspace's squill config has an [[embedded]] rule for a
// language whose settings don't, a notification offers to add it to the
// workspace settings (.vscode/settings.json).

import * as vscode from "vscode";

// The VS Code languages each built-in grammar formats. A grammar given
// as a wasm file or URL names no language VS Code knows, so it's skipped.
const LANGUAGES: Record<string, string[]> = {
	"rust": ["rust"],
	"go": ["go"],
	"python": ["python"],
	"javascript": ["javascript", "javascriptreact"],
	"typescript": ["typescript"],
	"tsx": ["typescriptreact"],
	"gleam": ["gleam"],
	"c++": ["cpp"],
	"c#": ["csharp"],
	"java": ["java"],
	"kotlin": ["kotlin"],
	"swift": ["swift"],
};

const ACTION = "source.formatSql";
const DECLINED = "squill.formatOnSaveDeclined";
const CONFIGS = "**/squill.{toml,yaml,yml}";

export function suggestFormatOnSave(context: vscode.ExtensionContext) {
	const watcher = vscode.workspace.createFileSystemWatcher(CONFIGS);
	context.subscriptions.push(
		watcher,
		watcher.onDidCreate(() => void suggest(context)),
		watcher.onDidChange(() => void suggest(context)),
	);
	void suggest(context);
}

// Only one notification at a time, however many configs change at once.
let asking = false;

async function suggest(context: vscode.ExtensionContext) {
	if (asking || context.workspaceState.get<boolean>(DECLINED)) {
		return;
	}
	const missing = (await embeddedLanguages()).filter(
		(language) => !formatsOnSave(language),
	);
	if (missing.length === 0) {
		return;
	}
	asking = true;
	try {
		const add = "Add to workspace settings";
		const never = "Don't ask again";
		const choice = await vscode.window.showInformationMessage(
			`squill can format the SQL in your ${list(missing)} files when you save them. Turn that on?`,
			add,
			never,
		);
		if (choice === add) {
			for (const language of missing) {
				await enable(language);
			}
		} else if (choice === never) {
			await context.workspaceState.update(DECLINED, true);
		}
	} finally {
		asking = false;
	}
}

// The languages the workspace's squill configs have [[embedded]] rules
// for, from each rule's `grammar` (`grammar = "rust"` in TOML, `grammar:
// rust` in YAML). Read by pattern rather than parsed: this only decides
// what to offer, and squill itself reads the config.
async function embeddedLanguages(): Promise<string[]> {
	const files = await vscode.workspace.findFiles(
		CONFIGS,
		"**/node_modules/**",
		20,
	);
	const languages = new Set<string>();
	for (const file of files) {
		let text: string;
		try {
			text = new TextDecoder().decode(await vscode.workspace.fs.readFile(file));
		} catch {
			continue;
		}
		for (const match of text.matchAll(
			/^\s*-?\s*grammar\s*[=:]\s*["']?([^"'\s,]+)/gm,
		)) {
			for (const language of LANGUAGES[match[1]] ?? []) {
				languages.add(language);
			}
		}
	}
	return [...languages];
}

// Whether any settings (user or workspace) already run the action on
// save for `language`.
function formatsOnSave(language: string): boolean {
	const actions = vscode.workspace
		.getConfiguration("editor", { languageId: language })
		.get<unknown>("codeActionsOnSave");
	if (Array.isArray(actions)) {
		return actions.includes(ACTION);
	}
	if (actions && typeof actions === "object") {
		const value = (actions as Record<string, unknown>)[ACTION];
		return value !== undefined && value !== false && value !== "never";
	}
	return false;
}

// Add the action to `language`'s block of the workspace settings,
// keeping the actions already there.
async function enable(language: string) {
	const config = vscode.workspace.getConfiguration("editor", {
		languageId: language,
	});
	const current =
		config.inspect<unknown>("codeActionsOnSave")?.workspaceLanguageValue;
	const actions: Record<string, unknown> = Array.isArray(current)
		? Object.fromEntries(current.map((action) => [action, "explicit"]))
		: { ...(current as Record<string, unknown> | undefined) };
	actions[ACTION] = "explicit";
	await config.update(
		"codeActionsOnSave",
		actions,
		vscode.ConfigurationTarget.Workspace,
		true,
	);
}

// `Rust`, `Rust and Go`, `Rust, Go, and Python`: the languages by
// name, where an id isn't one already.
function list(languages: string[]): string {
	const names = languages.map(
		(id) =>
			({
				cpp: "C++",
				csharp: "C#",
				javascriptreact: "JSX",
				typescriptreact: "TSX",
				javascript: "JavaScript",
				typescript: "TypeScript",
			})[id] ?? id.charAt(0).toUpperCase() + id.slice(1),
	);
	if (names.length <= 2) {
		return names.join(" and ");
	}
	return `${names.slice(0, -1).join(", ")}, and ${names[names.length - 1]}`;
}
