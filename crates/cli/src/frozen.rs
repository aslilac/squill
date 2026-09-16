//! Files that are immutable once they land on the baseline branch.
//!
//! Some files cannot be rewritten after they ship, even into an
//! identical-meaning form: sqlx records a checksum of every migration
//! and refuses to run when one changes. But a *new* migration should
//! still be formatted, and "ignore the whole directory" gives that up.
//!
//! So `frozen` globs mark paths that squill formats only while they are
//! new. A file matching one is skipped when it already exists in the
//! baseline ref's tree — the shape it shipped in is the shape it keeps,
//! however squill's own style drifts afterward.
//!
//! The lookup is one `git ls-tree` per repository. Only the baseline
//! tip's tree is needed, never its history, so a depth-1 CI clone is
//! enough as long as the ref itself was fetched. The ref is discovered
//! from the remote rather than guessed from a list of branch names.

use std::collections::HashSet;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

/// The set of paths a baseline ref already carries, for one repository.
pub struct Baseline {
	/// Absolute paths of every file tracked at the ref.
	tracked: HashSet<PathBuf>,
}

impl Baseline {
	/// Is `path` already on the baseline, and so immutable?
	pub fn contains(&self, path: &Path) -> bool {
		std::path::absolute(path)
			.map(|absolute| self.tracked.contains(&absolute))
			.unwrap_or(false)
	}
}

/// The repository containing `dir`, or `None` when it is not in one.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
	let out = Command::new("git")
		.arg("-C")
		.arg(dir)
		.args(["rev-parse", "--show-toplevel"])
		.output()
		.ok()?;
	if !out.status.success() {
		return None;
	}
	let root = String::from_utf8(out.stdout).ok()?;
	let root = root.trim_end_matches('\n');
	if root.is_empty() {
		return None;
	}
	Some(PathBuf::from(root))
}

/// Read every path the baseline ref tracks in `root`.
///
/// Errors are hard on purpose: silently formatting a frozen file is the
/// exact failure this feature exists to prevent, so an unresolvable ref
/// has to stop the run rather than quietly let writes through.
pub fn load(
	root: &Path,
	requested_ref: Option<&str>,
	fetch: bool,
) -> Result<Baseline, String> {
	let resolved = match requested_ref {
		Some(name) => {
			if !ref_exists(root, name) {
				return Err(format!(
					"frozen-ref `{name}` does not resolve in {}; fetch it, or set \
					 a different `frozen-ref`",
					root.display()
				));
			}
			name.to_string()
		}
		None => discover_ref(root, fetch)?,
	};

	// -z: NUL-separated and never quoted, so paths with spaces, quotes,
	// or newlines survive intact.
	let out = Command::new("git")
		.arg("-C")
		.arg(root)
		.args(["ls-tree", "-r", "-z", "--name-only", &resolved])
		.output()
		.map_err(|err| {
			format!("running git ls-tree in {}: {err}", root.display())
		})?;
	if !out.status.success() {
		return Err(format!(
			"git ls-tree {resolved} failed in {}: {}",
			root.display(),
			String::from_utf8_lossy(&out.stderr).trim()
		));
	}

	let mut tracked = HashSet::new();
	for name in out.stdout.split(|byte| *byte == 0) {
		if name.is_empty() {
			continue;
		}
		let name = match std::str::from_utf8(name) {
			Ok(name) => name,
			// A path we cannot read is a path we cannot match; it can
			// only ever be a file we would have left alone anyway.
			Err(_) => continue,
		};
		tracked.insert(root.join(name));
	}
	Ok(Baseline { tracked })
}

/// Which remote to ask about the default branch: the only one if there
/// is only one, else `origin`. Naming a remote is unavoidable once a
/// repo has several, but the single-remote case — nearly all of them —
/// needs no convention at all.
fn remote(root: &Path) -> Option<String> {
	let out =
		Command::new("git").arg("-C").arg(root).arg("remote").output().ok()?;
	if !out.status.success() {
		return None;
	}
	let text = String::from_utf8(out.stdout).ok()?;
	let names: Vec<&str> =
		text.lines().map(str::trim).filter(|line| !line.is_empty()).collect();
	match names.as_slice() {
		[only] => Some((*only).to_string()),
		many if many.contains(&"origin") => Some("origin".to_string()),
		_ => None,
	}
}

/// Find the baseline ref without assuming what the default branch is
/// called.
///
/// In order: the remote's recorded HEAD, which `git clone` sets and
/// `git remote set-head` refreshes; then the sole remote-tracking
/// branch, if there is exactly one — the shape a CI checkout that
/// fetched one branch leaves behind. Only with `fetch` does squill go to
/// the network, because a formatter that quietly makes network calls is
/// one that hangs on a bad link or fails in an offline build.
fn discover_ref(root: &Path, fetch: bool) -> Result<String, String> {
	let Some(remote) = remote(root) else {
		return Err(format!(
			"`frozen` is set but {} has no remote to take a baseline from; name a \
			 ref with `frozen-ref`",
			root.display()
		));
	};

	let head = format!("refs/remotes/{remote}/HEAD");
	if let Some(target) = symbolic_ref(root, &head) {
		return Ok(target);
	}

	let branches = remote_branches(root, &remote);
	if let [only] = branches.as_slice() {
		return Ok(only.clone());
	}

	if fetch {
		// --depth=1: the tip tree is all a baseline needs.
		let out = Command::new("git")
			.arg("-C")
			.arg(root)
			.args(["fetch", "--quiet", "--depth=1", &remote, "HEAD"])
			.output()
			.map_err(|err| {
				format!("running git fetch in {}: {err}", root.display())
			})?;
		if !out.status.success() {
			return Err(format!(
				"git fetch {remote} HEAD failed in {}: {}",
				root.display(),
				String::from_utf8_lossy(&out.stderr).trim()
			));
		}
		return Ok("FETCH_HEAD".to_string());
	}

	Err(format!(
		"`frozen` is set but no baseline ref is available in {}: `{head}` is unset \
		 and {} remote-tracking branches were found. Record it with `git remote \
		 set-head {remote} --auto`, name one with `frozen-ref`, or pass \
		 --frozen-fetch to let squill fetch {remote}/HEAD itself",
		root.display(),
		branches.len()
	))
}

/// What a symbolic ref points at, as a name git can resolve later.
fn symbolic_ref(root: &Path, name: &str) -> Option<String> {
	let out = Command::new("git")
		.arg("-C")
		.arg(root)
		.args(["symbolic-ref", "--short", name])
		.output()
		.ok()?;
	if !out.status.success() {
		return None;
	}
	let target = String::from_utf8(out.stdout).ok()?;
	let target = target.trim();
	(!target.is_empty()).then(|| target.to_string())
}

/// Remote-tracking branches under `refs/remotes/<remote>/`, minus HEAD.
fn remote_branches(root: &Path, remote: &str) -> Vec<String> {
	let Ok(out) = Command::new("git")
		.arg("-C")
		.arg(root)
		.args([
			"for-each-ref",
			"--format=%(refname:short)",
			&format!("refs/remotes/{remote}/"),
		])
		.output()
	else {
		return Vec::new();
	};
	String::from_utf8(out.stdout)
		.map(|text| {
			text
				.lines()
				.map(str::trim)
				.filter(|name| !name.is_empty() && !name.ends_with("/HEAD"))
				.map(str::to_string)
				.collect()
		})
		.unwrap_or_default()
}

fn ref_exists(root: &Path, name: &str) -> bool {
	Command::new("git")
		.arg("-C")
		.arg(root)
		.args(["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")])
		.output()
		.is_ok_and(|out| out.status.success())
}
