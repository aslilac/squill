//! Grammars from https URLs, locked by SHA-256.
//!
//! A `grammar = "https://…/tree-sitter-x.wasm"` is downloaded once and
//! its hash recorded in the `squill.lock` beside the config; every later
//! download must match it. Downloads are cached by hash in the user's
//! cache directory and re-checked when read, so a locked, cached grammar
//! never touches the network. With `--locked`, a URL the lockfile
//! doesn't have is an error instead of a new entry.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use sha2::Digest;
use sha2::Sha256;
use toml::de::DeTable;
use toml::de::DeValue;

/// Refuse anything bigger: grammars are a few MB at most.
const MAX_BYTES: usize = 64 * 1024 * 1024;

/// The bytes of the grammar at `url`, checked against (or recorded in)
/// the lockfile at `lockfile`.
pub fn grammar_bytes(
	url: &str,
	lockfile: &Path,
	locked: bool,
) -> Result<Vec<u8>, String> {
	grammar_bytes_with(url, lockfile, locked, &cache_dir()?, &download)
}

fn grammar_bytes_with(
	url: &str,
	lockfile: &Path,
	locked: bool,
	cache: &Path,
	fetch: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
	let mut lock = Lock::read(lockfile)?;
	if let Some(expected) = lock.grammars.get(url) {
		let cached = cache.join(format!("{expected}.wasm"));
		if let Ok(bytes) = std::fs::read(&cached)
			&& sha256(&bytes) == *expected
		{
			return Ok(bytes);
		}
		let bytes = fetch(url)?;
		let actual = sha256(&bytes);
		if actual != *expected {
			return Err(format!(
				"{url} doesn't match {}: it recorded sha256 {expected}, the \
				 download is {actual}. If the grammar changed on purpose, remove \
				 its line from the lockfile to accept the new one.",
				lockfile.display()
			));
		}
		store(cache, &actual, &bytes);
		return Ok(bytes);
	}
	if locked {
		return Err(format!(
			"{url} isn't in {}, and --locked won't add it; run squill without \
			 --locked to record it",
			lockfile.display()
		));
	}
	let bytes = fetch(url)?;
	let actual = sha256(&bytes);
	store(cache, &actual, &bytes);
	lock.grammars.insert(url.to_string(), actual);
	lock.write(lockfile)?;
	Ok(bytes)
}

/// `squill.lock`: each downloaded grammar's URL and SHA-256.
#[derive(Default)]
struct Lock {
	grammars: BTreeMap<String, String>,
}

impl Lock {
	fn read(path: &Path) -> Result<Lock, String> {
		let text = match std::fs::read_to_string(path) {
			Ok(text) => text,
			Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
				return Ok(Lock::default());
			}
			Err(err) => return Err(format!("{}: {err}", path.display())),
		};
		let invalid = |message: &str| format!("{}: {message}", path.display());
		let table = DeTable::parse(&text).map_err(|err| invalid(err.message()))?;
		let mut lock = Lock::default();
		if let Some(grammars) = table.get_ref().get("grammars") {
			let DeValue::Table(grammars) = grammars.get_ref() else {
				return Err(invalid("`grammars` must be a table"));
			};
			for (url, hash) in grammars {
				let DeValue::String(hash) = hash.get_ref() else {
					return Err(invalid("each grammar's hash must be a string"));
				};
				lock.grammars.insert(url.get_ref().to_string(), hash.to_string());
			}
		}
		Ok(lock)
	}

	fn write(&self, path: &Path) -> Result<(), String> {
		let mut out = String::from(
			"# Written by squill: the SHA-256 of each grammar squill.toml\n\
			 # downloads, which later downloads must match. Commit it.\n\n\
			 [grammars]\n",
		);
		for (url, hash) in &self.grammars {
			out.push_str(&format!("{} = \"{hash}\"\n", toml_string(url)));
		}
		std::fs::write(path, out)
			.map_err(|err| format!("{}: {err}", path.display()))
	}
}

/// A TOML basic string.
fn toml_string(text: &str) -> String {
	let mut out = String::from("\"");
	for c in text.chars() {
		match c {
			'"' => out.push_str("\\\""),
			'\\' => out.push_str("\\\\"),
			c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
			c => out.push(c),
		}
	}
	out.push('"');
	out
}

fn sha256(bytes: &[u8]) -> String {
	Sha256::digest(bytes).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Cache `bytes` under their hash. Best effort: a cache that can't be
/// written only means downloading again next time.
fn store(cache: &Path, hash: &str, bytes: &[u8]) {
	if std::fs::create_dir_all(cache).is_err() {
		return;
	}
	let file = cache.join(format!("{hash}.wasm"));
	let partial = cache.join(format!("{hash}.wasm.partial"));
	if std::fs::write(&partial, bytes).is_ok() {
		let _ = std::fs::rename(&partial, &file);
	}
}

/// Where downloaded grammars are cached: `$XDG_CACHE_HOME`, or the
/// platform's own cache directory.
fn cache_dir() -> Result<PathBuf, String> {
	let env = |name: &str| {
		std::env::var_os(name).map(PathBuf::from).filter(|path| path.is_absolute())
	};
	let base = env("XDG_CACHE_HOME")
		.or_else(|| {
			if cfg!(windows) {
				env("LOCALAPPDATA")
			} else if cfg!(target_os = "macos") {
				env("HOME").map(|home| home.join("Library/Caches"))
			} else {
				env("HOME").map(|home| home.join(".cache"))
			}
		})
		.ok_or("no cache directory: set XDG_CACHE_HOME")?;
	Ok(base.join("squill").join("grammars"))
}

fn download(url: &str) -> Result<Vec<u8>, String> {
	let fail = |err: &dyn std::fmt::Display| format!("downloading {url}: {err}");
	let client = reqwest::blocking::Client::builder()
		.https_only(true)
		.timeout(Duration::from_secs(60))
		.user_agent(concat!("squill/", env!("CARGO_PKG_VERSION")))
		.build()
		.map_err(|err| fail(&err))?;
	let response = client.get(url).send().map_err(|err| fail(&err))?;
	if !response.status().is_success() {
		return Err(fail(&response.status()));
	}
	if response.content_length().is_some_and(|len| len as usize > MAX_BYTES) {
		return Err(fail(&"larger than 64 MB"));
	}
	let bytes = response.bytes().map_err(|err| fail(&err))?;
	if bytes.len() > MAX_BYTES {
		return Err(fail(&"larger than 64 MB"));
	}
	Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::cell::Cell;

	const URL: &str = "https://example.com/grammars/tree-sitter-lua.wasm";

	struct Scratch(PathBuf);

	impl Scratch {
		fn new(name: &str) -> Scratch {
			let dir = std::env::temp_dir()
				.join(format!("squill-remote-{name}-{}", std::process::id()));
			let _ = std::fs::remove_dir_all(&dir);
			std::fs::create_dir_all(&dir).expect("scratch dir");
			Scratch(dir)
		}
	}

	impl Drop for Scratch {
		fn drop(&mut self) {
			let _ = std::fs::remove_dir_all(&self.0);
		}
	}

	#[test]
	fn records_then_enforces_then_caches() {
		let scratch = Scratch::new("lock");
		let lockfile = scratch.0.join("squill.lock");
		let cache = scratch.0.join("cache");
		let served = Cell::new(b"grammar v1".to_vec());
		let fetches = Cell::new(0);
		let fetch = |_: &str| {
			fetches.set(fetches.get() + 1);
			Ok(served.take_and_restore())
		};
		let get =
			|locked| grammar_bytes_with(URL, &lockfile, locked, &cache, &fetch);

		// --locked and unrecorded: refused, nothing written.
		let err = get(true).unwrap_err();
		assert!(err.contains("--locked won't add it"), "{err}");
		assert!(!lockfile.exists());

		// First fetch records the hash.
		assert_eq!(get(false).unwrap(), b"grammar v1");
		let lock = std::fs::read_to_string(&lockfile).unwrap();
		assert!(
			lock.contains(&format!("\"{URL}\" = \"{}\"", sha256(b"grammar v1")))
		);

		// Recorded and cached: no network, even --locked.
		assert_eq!(get(true).unwrap(), b"grammar v1");
		assert_eq!(fetches.get(), 1);

		// A corrupted cache is refetched and checked.
		let cached = cache.join(format!("{}.wasm", sha256(b"grammar v1")));
		std::fs::write(&cached, b"garbage").unwrap();
		assert_eq!(get(true).unwrap(), b"grammar v1");
		assert_eq!(fetches.get(), 2);

		// The URL now serves something else: refused, lockfile untouched.
		std::fs::remove_file(&cached).unwrap();
		served.set(b"grammar v2".to_vec());
		let err = get(false).unwrap_err();
		assert!(err.contains(&sha256(b"grammar v2")), "{err}");
		assert_eq!(std::fs::read_to_string(&lockfile).unwrap(), lock);
	}

	#[test]
	fn lockfile_round_trips_awkward_urls() {
		let scratch = Scratch::new("roundtrip");
		let path = scratch.0.join("squill.lock");
		let mut lock = Lock::default();
		let awkward = "https://example.com/a \"quoted\" \\path.wasm";
		lock.grammars.insert(awkward.to_string(), "ab".repeat(32));
		lock.grammars.insert(URL.to_string(), "cd".repeat(32));
		lock.write(&path).unwrap();
		let read = Lock::read(&path).unwrap();
		assert_eq!(read.grammars, lock.grammars);
	}

	trait TakeAndRestore<T> {
		fn take_and_restore(&self) -> T;
	}

	impl<T: Clone + Default> TakeAndRestore<T> for Cell<T> {
		fn take_and_restore(&self) -> T {
			let value = self.take();
			self.set(value.clone());
			value
		}
	}
}
