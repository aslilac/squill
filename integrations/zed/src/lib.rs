//! squill for Zed: runs `squill lsp` for SQL, and for the languages
//! squill formats embedded SQL in.

use zed_extension_api::{
	self as zed, LanguageServerId, Result, settings::LspSettings,
};

struct Squill;

impl zed::Extension for Squill {
	fn new() -> Self {
		Squill
	}

	fn language_server_command(
		&mut self,
		language_server_id: &LanguageServerId,
		worktree: &zed::Worktree,
	) -> Result<zed::Command> {
		// `lsp.squill.binary` in Zed's settings wins; otherwise squill
		// from the worktree's PATH.
		let binary =
			LspSettings::for_worktree(language_server_id.as_ref(), worktree)
				.ok()
				.and_then(|settings| settings.binary);
		let command = match binary.as_ref().and_then(|binary| binary.path.clone()) {
			Some(path) => path,
			None => worktree.which("squill").ok_or_else(|| {
				"squill isn't on your PATH. Install it \
				 (https://github.com/aslilac/squill#installation), or set \
				 lsp.squill.binary.path in your settings."
					.to_string()
			})?,
		};
		let args = binary
			.and_then(|binary| binary.arguments)
			.unwrap_or_else(|| vec!["lsp".to_string()]);
		Ok(zed::Command { command, args, env: worktree.shell_env() })
	}

	fn language_server_initialization_options(
		&mut self,
		language_server_id: &LanguageServerId,
		worktree: &zed::Worktree,
	) -> Result<Option<zed::serde_json::Value>> {
		Ok(
			LspSettings::for_worktree(language_server_id.as_ref(), worktree)
				.ok()
				.and_then(|settings| settings.initialization_options),
		)
	}
}

zed::register_extension!(Squill);
