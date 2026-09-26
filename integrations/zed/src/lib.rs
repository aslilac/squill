//! squill for Zed: runs `squill language-server start` for SQL, and for
//! the languages squill formats embedded SQL in.
//!
//! Which squill: `lsp.squill.binary.path` from Zed's settings, else the
//! `squill` on the worktree's PATH (so the editor formats exactly as
//! your CLI and CI do), else the latest release downloaded from GitHub.

use std::fs;

use zed_extension_api::settings::LspSettings;
use zed_extension_api::{self as zed, LanguageServerId, Result};

const REPO: &str = "aslilac/squill";

struct Squill {
	/// A downloaded binary found (or fetched) earlier this session.
	downloaded: Option<String>,
}

impl Squill {
	/// The squill downloaded from the latest GitHub release, fetching it
	/// the first time. Releases live in `squill-<version>/` in the
	/// extension's directory; older ones are removed once a newer one is
	/// in place.
	fn downloaded_binary(&mut self, id: &LanguageServerId) -> Result<String> {
		if let Some(path) = &self.downloaded
			&& fs::metadata(path).is_ok_and(|meta| meta.is_file())
		{
			return Ok(path.clone());
		}
		let status = |status: zed::LanguageServerInstallationStatus| {
			zed::set_language_server_installation_status(id, &status);
		};
		status(zed::LanguageServerInstallationStatus::CheckingForUpdate);
		let release = zed::latest_github_release(
			REPO,
			zed::GithubReleaseOptions { require_assets: true, pre_release: false },
		)?;
		let (os, arch) = zed::current_platform();
		let target = match (os, arch) {
			(zed::Os::Mac, zed::Architecture::Aarch64) => "aarch64-apple-darwin",
			(zed::Os::Linux, zed::Architecture::X8664) => "x86_64-unknown-linux-gnu",
			(zed::Os::Linux, zed::Architecture::Aarch64) => {
				"aarch64-unknown-linux-gnu"
			}
			(zed::Os::Windows, zed::Architecture::X8664) => "x86_64-pc-windows-msvc",
			(zed::Os::Windows, zed::Architecture::Aarch64) => {
				"aarch64-pc-windows-msvc"
			}
			_ => {
				return Err(
					"squill has no prebuilt binary for this platform; install it \
					 yourself (https://github.com/aslilac/squill#installation)"
						.to_string(),
				);
			}
		};
		let (archive, file_type, executable) = match os {
			zed::Os::Windows => ("zip", zed::DownloadedFileType::Zip, "squill.exe"),
			_ => ("tar.gz", zed::DownloadedFileType::GzipTar, "squill"),
		};
		// The release workflow names archives after the tag.
		let asset_name = format!("squill-{}-{target}.{archive}", release.version);
		let asset =
			release.assets.iter().find(|asset| asset.name == asset_name).ok_or_else(
				|| format!("the latest squill release has no {asset_name}"),
			)?;

		let dir = format!("squill-{}", release.version);
		let binary = format!("{dir}/{executable}");
		if !fs::metadata(&binary).is_ok_and(|meta| meta.is_file()) {
			status(zed::LanguageServerInstallationStatus::Downloading);
			zed::download_file(&asset.download_url, &dir, file_type)
				.map_err(|err| format!("downloading {asset_name}: {err}"))?;
			zed::make_file_executable(&binary)?;
			// Only the release in use is kept.
			if let Ok(entries) = fs::read_dir(".") {
				for entry in entries.flatten() {
					let name = entry.file_name();
					let name = name.to_string_lossy();
					if name.starts_with("squill-") && name != dir {
						let _ = fs::remove_dir_all(entry.path());
					}
				}
			}
		}
		status(zed::LanguageServerInstallationStatus::None);
		self.downloaded = Some(binary.clone());
		Ok(binary)
	}
}

impl zed::Extension for Squill {
	fn new() -> Self {
		Squill { downloaded: None }
	}

	fn language_server_command(
		&mut self,
		language_server_id: &LanguageServerId,
		worktree: &zed::Worktree,
	) -> Result<zed::Command> {
		let binary =
			LspSettings::for_worktree(language_server_id.as_ref(), worktree)
				.ok()
				.and_then(|settings| settings.binary);
		let args = binary
			.as_ref()
			.and_then(|binary| binary.arguments.clone())
			.unwrap_or_else(|| vec!["language-server".to_string(), "start".to_string()]);
		let command = match binary
			.and_then(|binary| binary.path)
			.or_else(|| worktree.which("squill"))
		{
			Some(path) => path,
			None => {
				self.downloaded_binary(language_server_id).inspect_err(|err| {
					zed::set_language_server_installation_status(
						language_server_id,
						&zed::LanguageServerInstallationStatus::Failed(err.clone()),
					);
				})?
			}
		};
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
