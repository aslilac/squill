// Formatting SQL on save, as one of the IDE's actions on save. Reformat
// Code keeps the IDE's formatter for other languages, so squill formats
// their embedded SQL here instead, after that formatter has run. It's off
// until turned on under Settings | Tools | Actions on Save; when a
// project's squill config has an [[embedded]] rule, a notification offers
// to turn it on.

package dev.mckayla.squill

import com.intellij.ide.actionsOnSave.ActionOnSaveComment
import com.intellij.ide.actionsOnSave.ActionOnSaveContext
import com.intellij.ide.actionsOnSave.ActionOnSaveInfo
import com.intellij.ide.actionsOnSave.ActionOnSaveInfoProvider
import com.intellij.ide.actionsOnSave.impl.ActionsOnSaveFileDocumentManagerListener.DocumentUpdatingActionOnSave
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.application.EDT
import com.intellij.openapi.application.readAction
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.platform.lsp.util.applyTextEdits
import com.intellij.ui.components.ActionLink
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.eclipse.lsp4j.DocumentFormattingParams
import org.eclipse.lsp4j.FormattingOptions

const val FORMAT_ON_SAVE = "Format SQL with squill"

class SquillFormatOnSave : DocumentUpdatingActionOnSave() {
	override val presentableName = FORMAT_ON_SAVE

	override fun isEnabledForProject(project: Project) =
		SquillProjectSettings.getInstance(project).state.formatOnSave

	override suspend fun updateDocument(project: Project, document: Document) {
		val file = readAction { FileDocumentManager.getInstance().getFile(document) }
			?: return
		if (!isSupported(file)) {
			return
		}
		val client = LspClientManager.getInstance(project)
			.getClients(SquillLspIntegrationProvider::class.java)
			.firstOrNull() ?: return
		val (stamp, params) = readAction {
			// squill ignores the options: its own come from squill.toml.
			document.modificationStamp to DocumentFormattingParams(
				client.getDocumentIdentifier(file),
				FormattingOptions(4, false),
			)
		}
		val edits = client.sendRequest { it.textDocumentService.formatting(params) }
		if (edits.isNullOrEmpty()) {
			return
		}
		withContext(Dispatchers.EDT) {
			// Edited while squill was answering: the edits are for older text.
			if (document.modificationStamp == stamp) {
				WriteCommandAction.runWriteCommandAction(project, FORMAT_ON_SAVE, null, {
					applyTextEdits(document, edits)
				})
			}
		}
	}
}

class SquillActionOnSaveInfoProvider : ActionOnSaveInfoProvider() {
	override fun getActionOnSaveInfos(context: ActionOnSaveContext): List<ActionOnSaveInfo> =
		listOf(SquillActionOnSaveInfo(context))

	override fun getSearchableOptions() = listOf(FORMAT_ON_SAVE)
}

private class SquillActionOnSaveInfo(context: ActionOnSaveContext) :
	ActionOnSaveInfo(context) {
	private val settings = SquillProjectSettings.getInstance(project)
	private var enabled = settings.state.formatOnSave

	override fun getActionOnSaveName() = FORMAT_ON_SAVE

	override fun isActionOnSaveEnabled() = enabled

	override fun setActionOnSaveEnabled(enabled: Boolean) {
		this.enabled = enabled
	}

	override fun isModified() = enabled != settings.state.formatOnSave

	override fun apply() {
		settings.state.formatOnSave = enabled
	}

	override fun getComment(): ActionOnSaveComment = ActionOnSaveComment.info(
		"SQL files, and the SQL embedded in files a squill.toml [[embedded]] " +
			"rule covers",
	)

	override fun getActionLinks(): List<ActionLink> =
		listOf(createGoToPageInSettingsLink(SquillConfigurable.ID))
}

// The config file names squill looks for, in a directory or its .config.
private val CONFIG_NAMES = listOf("squill.toml", "squill.yaml", "squill.yml")

// An [[embedded]] rule's `grammar` (`grammar = "rust"` in TOML, `grammar:
// rust` in YAML). Read by pattern rather than parsed: this only decides
// whether to offer, and squill itself reads the config.
private val GRAMMAR = Regex("""^\s*-?\s*grammar\s*[=:]""", RegexOption.MULTILINE)

class SuggestFormatOnSave : ProjectActivity {
	override suspend fun execute(project: Project) {
		val state = SquillProjectSettings.getInstance(project).state
		if (state.formatOnSave || state.formatOnSaveDeclined) {
			return
		}
		if (!readAction { hasEmbeddedRule(project) }) {
			return
		}
		NotificationGroupManager.getInstance()
			.getNotificationGroup("squill")
			.createNotification(
				"squill can format the SQL in your files when you save them. " +
					"Turn that on?",
				NotificationType.INFORMATION,
			)
			.addAction(
				NotificationAction.createSimpleExpiring("Turn on") {
					state.formatOnSave = true
				},
			)
			.addAction(
				NotificationAction.createSimpleExpiring("Don't ask again") {
					state.formatOnSaveDeclined = true
				},
			)
			.notify(project)
	}

	// Whether a squill config at the project's root has an [[embedded]]
	// rule.
	private fun hasEmbeddedRule(project: Project): Boolean {
		val root = project.basePath
			?.let(LocalFileSystem.getInstance()::findFileByPath)
			?: return false
		val dirs = listOfNotNull(root, root.findChild(".config"))
		return dirs.any { dir ->
			CONFIG_NAMES.any { name ->
				dir.findChild(name)?.let(::read)?.let(GRAMMAR::containsMatchIn) == true
			}
		}
	}

	private fun read(file: VirtualFile): String? =
		runCatching { VfsUtilCore.loadText(file) }.getOrNull()
}
