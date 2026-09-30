package dev.mckayla.squill

import com.intellij.openapi.vfs.VirtualFile

// The files squill's language server hears about: SQL, and the languages
// squill has built-in grammars for, by extension. The server resolves each
// one against squill.toml, so a file no rule covers is left alone.
private val LANGUAGE_IDS = buildMap {
	put("sql", "sql")
	put("rs", "rust")
	put("go", "go")
	for (ext in listOf("py", "pyi")) put(ext, "python")
	for (ext in listOf("js", "mjs", "cjs")) put(ext, "javascript")
	put("jsx", "javascriptreact")
	for (ext in listOf("ts", "mts", "cts")) put(ext, "typescript")
	put("tsx", "typescriptreact")
	put("gleam", "gleam")
	for (ext in listOf("cc", "cpp", "cxx", "c++", "h", "hh", "hpp", "hxx")) {
		put(ext, "cpp")
	}
	put("cs", "csharp")
	put("java", "java")
	for (ext in listOf("kt", "kts")) put(ext, "kotlin")
	put("swift", "swift")
}

// The language id squill's server expects for `file`, or null when squill
// has nothing to do with it.
fun languageId(file: VirtualFile): String? =
	file.extension?.lowercase()?.let(LANGUAGE_IDS::get)

fun isSqlFile(file: VirtualFile): Boolean = languageId(file) == "sql"
