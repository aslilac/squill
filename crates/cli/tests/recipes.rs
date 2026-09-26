//! The docs' recipes: each directory under `docs/recipes/` is a project
//! in miniature — a squill config, maybe a query, a `before.*` input, and
//! the `after.*` squill makes of it. The docs site shows those files as
//! written, so this keeps them honest: `after` must be exactly what
//! squill makes of `before`, and must itself be left alone.
//!
//! Recipes whose grammar is an https URL need the network, so they run
//! only with `--ignored`; the rest run with every `cargo test`.

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

fn recipes() -> Vec<PathBuf> {
	let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/recipes");
	let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root)
		.expect("docs/recipes")
		.map(|entry| entry.expect("entry").path())
		.filter(|path| path.is_dir())
		.collect();
	dirs.sort();
	assert!(!dirs.is_empty(), "no recipes under {}", root.display());
	dirs
}

fn downloads_grammar(recipe: &Path) -> bool {
	std::fs::read_to_string(recipe.join("squill.toml"))
		.expect("every recipe has a squill.toml")
		.contains("\"https://")
}

/// `before.*` and `after.*`, which must share an extension.
fn pair(recipe: &Path) -> (String, String) {
	let mut before = None;
	let mut after = None;
	for entry in std::fs::read_dir(recipe).expect("read recipe") {
		let name = entry.expect("entry").file_name();
		let name = name.to_string_lossy().into_owned();
		if let Some(ext) = name.strip_prefix("before.") {
			before = Some((name.clone(), ext.to_string()));
		} else if let Some(ext) = name.strip_prefix("after.") {
			after = Some((name.clone(), ext.to_string()));
		}
	}
	let name = recipe.display();
	let (before, before_ext) =
		before.unwrap_or_else(|| panic!("{name}: no before.*"));
	let (after, after_ext) =
		after.unwrap_or_else(|| panic!("{name}: no after.*"));
	assert_eq!(before_ext, after_ext, "{name}: before and after differ in kind");
	(before, after)
}

fn copy_dir(from: &Path, to: &Path) {
	std::fs::create_dir_all(to).expect("create dir");
	for entry in std::fs::read_dir(from).expect("read dir") {
		let entry = entry.expect("entry");
		let target = to.join(entry.file_name());
		if entry.file_type().expect("file type").is_dir() {
			copy_dir(&entry.path(), &target);
		} else {
			std::fs::copy(entry.path(), &target).expect("copy");
		}
	}
}

fn check(recipe: &Path) {
	let name = recipe.file_name().expect("name").to_string_lossy().into_owned();
	let (before, after) = pair(recipe);
	// A scratch copy, fenced off by a repo-root marker so config
	// discovery can't wander above it.
	let dir = std::env::temp_dir()
		.join(format!("squill-recipe-{name}-{}", std::process::id()));
	let _ = std::fs::remove_dir_all(&dir);
	copy_dir(recipe, &dir);
	std::fs::create_dir_all(dir.join(".git")).expect("create .git marker");

	let run = |args: &[&str]| {
		Command::new(env!("CARGO_BIN_EXE_squill"))
			.current_dir(&dir)
			.args(args)
			.output()
			.expect("run squill")
	};
	let formatted = run(&["fmt", "--strict", "--locked", "--stdout", &before]);
	let stderr = String::from_utf8_lossy(&formatted.stderr);
	assert!(formatted.status.success(), "{name}: squill failed: {stderr}");
	let expected =
		std::fs::read_to_string(recipe.join(&after)).expect("read after");
	assert_eq!(
		String::from_utf8_lossy(&formatted.stdout),
		expected,
		"{name}: {after} isn't what squill makes of {before}"
	);
	let again = run(&["fmt", "--strict", "--locked", "--check", &after]);
	assert!(
		again.status.success(),
		"{name}: squill would change {after}: {}",
		String::from_utf8_lossy(&again.stdout)
	);
	let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn recipes_are_squills_output() {
	for recipe in recipes().iter().filter(|r| !downloads_grammar(r)) {
		check(recipe);
	}
}

#[test]
#[ignore = "downloads grammars"]
fn recipes_with_downloaded_grammars_are_squills_output() {
	for recipe in recipes().iter().filter(|r| downloads_grammar(r)) {
		check(recipe);
	}
}
