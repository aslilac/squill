// squill for JetBrains IDEs: a client for squill's language server,
// `squill language-server start`, on the IDE's own LSP support.
//
// The IDE formats most languages itself, so squill formats only SQL files
// through Reformat Code. In other languages it contributes its diagnostics
// about embedded SQL, highlighting for that SQL (see ./Highlighting.kt),
// and an action on save that formats it (see ./FormatOnSave.kt).

package dev.mckayla.squill

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.IconLoader
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspClient
import com.intellij.platform.lsp.api.LspIntegrationProvider
import com.intellij.platform.lsp.api.ProjectWideLspClientDescriptor
import com.intellij.platform.lsp.api.customization.LspCustomization
import com.intellij.platform.lsp.api.customization.LspFormattingSupport
import com.intellij.platform.lsp.api.customization.LspSemanticTokensSupport
import com.intellij.platform.lsp.api.lsWidget.LspClientWidgetItem
import com.intellij.psi.PsiFile
import java.io.File
import java.nio.charset.StandardCharsets

object SquillIcons {
	@JvmField
	val Squill = IconLoader.getIcon("/icons/squill.svg", SquillIcons::class.java)
}

class SquillLspIntegrationProvider : LspIntegrationProvider {
	override fun fileOpened(
		project: Project,
		file: VirtualFile,
		clientStarter: LspIntegrationProvider.LspClientStarter,
	) {
		if (!isSupported(file)) {
			return
		}
		val command = SquillExecutable.getInstance().get(project) ?: return
		clientStarter.ensureClientStarted(SquillClientDescriptor(project, command))
	}

	override fun createWidgetItem(lspClient: LspClient, currentFile: VirtualFile?) =
		LspClientWidgetItem(
			lspClient,
			currentFile,
			SquillIcons.Squill,
			SquillConfigurable::class.java,
		)
}

fun isSupported(file: VirtualFile): Boolean =
	file.isInLocalFileSystem && languageId(file) != null

private class SquillClientDescriptor(project: Project, private val command: String) :
	ProjectWideLspClientDescriptor(project, "squill") {
	override fun isSupportedFile(file: VirtualFile) = isSupported(file)

	override fun getLanguageId(file: VirtualFile) =
		languageId(file) ?: super.getLanguageId(file)

	override fun createCommandLine(): GeneralCommandLine =
		GeneralCommandLine(command, "language-server", "start")
			.withCharset(StandardCharsets.UTF_8)
			// The shell's environment, as squill would have in a terminal.
			.withParentEnvironmentType(GeneralCommandLine.ParentEnvironmentType.CONSOLE)
			// Rules and ignore globs in a squill.toml are relative to it, but
			// anything given relative to the working directory resolves from
			// the project root, as it would in a terminal there.
			.withWorkDirectory(project.basePath?.takeIf { File(it).isDirectory })

	override val lspCustomization = object : LspCustomization() {
		override val formattingCustomizer = object : LspFormattingSupport() {
			// Reformat Code is squill's for SQL files. Other languages keep
			// the IDE's formatter; their SQL formats on save.
			override fun shouldFormatThisFileExclusivelyByServer(
				file: VirtualFile,
				ideCanFormatThisFileItself: Boolean,
				serverExplicitlyWantsToFormatThisFile: Boolean,
			) = isSqlFile(file)
		}

		override val semanticTokensCustomizer = object : LspSemanticTokensSupport() {
			// Only embedded SQL: the IDE (or its SQL plugin) highlights SQL
			// files itself.
			override fun shouldAskServerForSemanticTokens(psiFile: PsiFile): Boolean {
				val file = psiFile.virtualFile ?: return false
				return SquillSettings.getInstance().state.highlightEmbeddedSql &&
					!isSqlFile(file)
			}

			override fun getTextAttributesKey(
				tokenType: String,
				modifiers: List<String>,
			): TextAttributesKey? = SqlColors.forTokenType(tokenType)
		}
	}
}
