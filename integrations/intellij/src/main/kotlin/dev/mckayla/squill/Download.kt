// Fetching squill from its latest GitHub release, for when it isn't
// installed. Downloads go in the IDE's system directory, one directory per
// release; each archive must match the SHA-256 digest GitHub reports for
// the asset, or it isn't installed.

package dev.mckayla.squill

import com.google.gson.JsonParser
import com.intellij.ide.util.PropertiesComponent
import com.intellij.openapi.application.PathManager
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.util.SystemInfo
import com.intellij.util.io.Decompressor
import com.intellij.util.system.CpuArch
import com.intellij.util.io.HttpRequests
import java.io.IOException
import java.nio.file.Files
import java.nio.file.Path
import java.nio.file.StandardCopyOption
import java.security.MessageDigest
import java.util.zip.GZIPInputStream
import kotlin.io.path.ExperimentalPathApi
import kotlin.io.path.deleteRecursively
import kotlin.io.path.isRegularFile
import kotlin.io.path.listDirectoryEntries
import kotlin.io.path.name
import kotlin.time.Duration.Companion.days

private const val REPO = "aslilac/squill"
private const val USER_AGENT = "squill-intellij"

// Look for a newer release at most this often.
private val CHECK_INTERVAL = 1.days

// What the last check found.
private const val TAG_KEY = "squill.download.tag"
private const val CHECKED_AT_KEY = "squill.download.checkedAt"

// This platform's release archive: the target in its name, and what's
// inside.
private class Target(val triple: String, val zip: Boolean, val executable: String)

private fun target(): Target? {
	val arm = CpuArch.isArm64()
	val triple = when {
		SystemInfo.isMac && arm -> "aarch64-apple-darwin"
		SystemInfo.isLinux && arm -> "aarch64-unknown-linux-gnu"
		SystemInfo.isLinux && CpuArch.isIntel64() -> "x86_64-unknown-linux-gnu"
		SystemInfo.isWindows && arm -> "aarch64-pc-windows-msvc"
		SystemInfo.isWindows && CpuArch.isIntel64() -> "x86_64-pc-windows-msvc"
		else -> return null
	}
	return Target(
		triple,
		zip = SystemInfo.isWindows,
		executable = if (SystemInfo.isWindows) "squill.exe" else "squill",
	)
}

@OptIn(ExperimentalPathApi::class)
object Download {
	private val root: Path get() = PathManager.getSystemDir().resolve("squill")
	private val properties get() = PropertiesComponent.getInstance()

	// Whether there's a build for this platform to download at all.
	val available: Boolean get() = target() != null

	// The squill downloaded earlier, if it's still there, and whether it's
	// been checked against the latest release recently.
	fun installed(): Pair<Path, Boolean>? {
		val target = target() ?: return null
		val tag = properties.getValue(TAG_KEY) ?: return null
		val binary = root.resolve("squill-$tag").resolve(target.executable)
		if (!binary.isRegularFile()) {
			return null
		}
		val checkedAt = properties.getLong(CHECKED_AT_KEY, 0)
		val fresh =
			System.currentTimeMillis() - checkedAt < CHECK_INTERVAL.inWholeMilliseconds
		return binary to fresh
	}

	// The latest release's squill, downloading it if it isn't the one
	// already installed. Throws when there's no build for this platform, or
	// looking it up or downloading it fails.
	fun latest(indicator: ProgressIndicator): Path {
		val target = target() ?: throw IOException(
			"there's no prebuilt squill for this platform",
		)
		indicator.text = "Looking up squill's latest release"
		val release = JsonParser.parseString(
			HttpRequests.request("https://api.github.com/repos/$REPO/releases/latest")
				.accept("application/vnd.github+json")
				.userAgent(USER_AGENT)
				.readString(indicator),
		).asJsonObject
		val tag = release.get("tag_name").asString
		val dir = root.resolve("squill-$tag")
		val binary = dir.resolve(target.executable)
		if (!binary.isRegularFile()) {
			val archiveName =
				"squill-$tag-${target.triple}.${if (target.zip) "zip" else "tar.gz"}"
			val asset = release.getAsJsonArray("assets")
				.map { it.asJsonObject }
				.find { it.get("name").asString == archiveName }
				?: throw IOException("squill $tag has no $archiveName")
			// `sha256:<hex>`, computed by GitHub when the asset was uploaded.
			val digest = asset.get("digest")
				?.takeUnless { it.isJsonNull }
				?.asString
				?.takeIf { it.startsWith("sha256:") }
				?.removePrefix("sha256:")
				?.lowercase()
				?: throw IOException("squill $tag has no SHA-256 digest for $archiveName")

			indicator.text = "Downloading squill $tag"
			val bytes = HttpRequests.request(asset.get("browser_download_url").asString)
				.userAgent(USER_AGENT)
				.readBytes(indicator)
			val actual = MessageDigest.getInstance("SHA-256")
				.digest(bytes)
				.joinToString("") { "%02x".format(it) }
			if (actual != digest) {
				throw IOException(
					"$archiveName doesn't match the digest GitHub reports for it",
				)
			}
			install(bytes, target, dir)
		}
		properties.setValue(TAG_KEY, tag)
		properties.setValue(CHECKED_AT_KEY, System.currentTimeMillis().toString())
		// Only the release in use is kept.
		for (entry in root.listDirectoryEntries("squill-*")) {
			if (entry.name != dir.name) {
				runCatching { entry.deleteRecursively() }
			}
		}
		return binary
	}

	// Note that the installed release is current, when looking up a newer
	// one failed: try again at the next check, not at every start.
	fun checked() {
		properties.setValue(CHECKED_AT_KEY, System.currentTimeMillis().toString())
	}

	// Unpacked aside, then moved into place: a half-unpacked release is
	// never mistaken for an installed one.
	private fun install(archive: ByteArray, target: Target, dir: Path) {
		Files.createDirectories(root)
		val partial = Files.createTempDirectory(root, "partial-")
		try {
			val decompressor = if (target.zip) {
				val file = partial.resolve("archive.zip")
				Files.write(file, archive)
				Decompressor.Zip(file)
			} else {
				Decompressor.Tar(GZIPInputStream(archive.inputStream()))
			}
			val unpacked = partial.resolve("squill")
			decompressor.filter { it == target.executable }.extract(unpacked)
			val executable = unpacked.resolve(target.executable)
			if (!executable.isRegularFile()) {
				throw IOException("no ${target.executable} in the archive")
			}
			executable.toFile().setExecutable(true)
			Files.move(unpacked, dir, StandardCopyOption.ATOMIC_MOVE)
		} finally {
			runCatching { partial.deleteRecursively() }
		}
	}
}
