package dev.mckayla.squill

import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.diagnostic.logger
import com.intellij.openapi.options.ShowSettingsUtil
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.util.SystemInfo
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.util.EnvironmentUtil
import java.io.File

private val LOG = logger<SquillExecutable>()

// Which squill: the configured path when there is one; else the `squill`
// on PATH, so the IDE formats exactly as the CLI and CI do; else one
// downloaded from squill's latest release.
@Service(Service.Level.APP)
class SquillExecutable {
	@Volatile
	private var resolved: String? = null

	// A download under way, or one that failed: either way, not another
	// until the settings change or the IDE restarts.
	@Volatile
	private var downloading = false

	companion object {
		fun getInstance(): SquillExecutable = service()
	}

	// The squill to run, when it's known now. When it isn't, it's found or
	// downloaded in the background, and squill then starts in every open
	// project.
	fun get(project: Project): String? {
		resolved?.let { return it }
		val configured = SquillSettings.getInstance().state.path
		if (!configured.isNullOrBlank()) {
			return configured.also { resolved = it }
		}
		onPath()?.let {
			return it.also { path -> resolved = path }
		}
		val installed = Download.installed()
		if (installed != null) {
			val (binary, fresh) = installed
			resolved = binary.toString()
			if (!fresh) {
				update(project)
			}
			return resolved
		}
		download(project)
		return null
	}

	// The `squill` on PATH, as the user's shell has it: an IDE started
	// from the dock still sees the shell's PATH here.
	private fun onPath(): String? {
		val name = if (SystemInfo.isWindows) "squill.exe" else "squill"
		val path = EnvironmentUtil.getValue("PATH") ?: return null
		return path.split(File.pathSeparator)
			.filter { it.isNotEmpty() }
			.map { File(it, name) }
			.find { it.isFile && it.canExecute() }
			?.path
	}

	// Forget which squill it was, and restart it everywhere: the setting
	// changed.
	fun reset() {
		resolved = null
		downloading = false
		restartEverywhere()
	}

	private fun download(project: Project) {
		synchronized(this) {
			if (downloading) return
			downloading = true
		}
		if (!Download.available) {
			notifyMissing(
				project,
				"squill isn't on your PATH, and there's no prebuilt squill for " +
					"this platform to download.",
			)
			return
		}
		object : Task.Backgroundable(project, "Downloading squill", true) {
			override fun run(indicator: ProgressIndicator) {
				resolved = Download.latest(indicator).toString()
			}

			override fun onSuccess() {
				downloading = false
				restartEverywhere()
			}

			override fun onThrowable(error: Throwable) {
				LOG.warn("downloading squill", error)
				notifyMissing(
					project,
					"squill isn't on your PATH, and downloading it failed: " +
						"${error.message}.",
				)
			}
		}.queue()
	}

	// Look for a newer release than the one downloaded, and switch to it.
	// The one there keeps running meanwhile, and if anything fails.
	private fun update(project: Project) {
		synchronized(this) {
			if (downloading) return
			downloading = true
		}
		object : Task.Backgroundable(project, "Updating squill", true) {
			var binary: String? = null

			override fun run(indicator: ProgressIndicator) {
				binary = Download.latest(indicator).toString()
			}

			override fun onSuccess() {
				downloading = false
				if (binary != resolved) {
					resolved = binary
					restartEverywhere()
				}
			}

			override fun onThrowable(error: Throwable) {
				// Offline, or GitHub is unhappy: keep what we have, and try
				// again at the next check.
				LOG.info("looking for a newer squill", error)
				Download.checked()
				downloading = false
			}
		}.queue()
	}

	private fun restartEverywhere() {
		for (project in ProjectManager.getInstance().openProjects) {
			if (!project.isDisposed) {
				LspClientManager.getInstance(project)
					.stopAndRestartClientsIfNeeded(SquillLspIntegrationProvider::class.java)
			}
		}
	}

	private fun notifyMissing(project: Project, message: String) {
		NotificationGroupManager.getInstance()
			.getNotificationGroup("squill")
			.createNotification(
				"$message Install it, or set its path in squill's settings.",
				NotificationType.ERROR,
			)
			.addAction(
				NotificationAction.createSimpleExpiring("Open settings") {
					ShowSettingsUtil.getInstance()
						.showSettingsDialog(project, SquillConfigurable::class.java)
				},
			)
			.notify(project)
	}
}
