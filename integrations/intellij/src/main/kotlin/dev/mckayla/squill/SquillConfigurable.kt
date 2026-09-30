package dev.mckayla.squill

import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.BoundSearchableConfigurable
import com.intellij.openapi.project.ProjectManager
import com.intellij.openapi.ui.DialogPanel
import com.intellij.ui.dsl.builder.AlignX
import com.intellij.ui.dsl.builder.bindSelected
import com.intellij.ui.dsl.builder.bindText
import com.intellij.ui.dsl.builder.panel

// Settings | Tools | squill.
class SquillConfigurable : BoundSearchableConfigurable("squill", "squill", ID) {
	companion object {
		const val ID = "dev.mckayla.squill"
	}

	private val state = SquillSettings.getInstance().state

	override fun createPanel(): DialogPanel = panel {
		row("Executable:") {
			textFieldWithBrowseButton(
				FileChooserDescriptorFactory.singleFile().withTitle("squill Executable"),
			)
				.bindText({ state.path.orEmpty() }, { state.path = it.trim() })
				.align(AlignX.FILL)
				.comment(
					"Empty uses the <code>squill</code> on your PATH, or else " +
						"downloads the latest release.",
				)
		}
		row {
			checkBox("Highlight the SQL embedded in other languages' strings")
				.bindSelected(state::highlightEmbeddedSql)
				.comment("Its colors are under Editor | Color Scheme | squill.")
		}
		row {
			comment(
				"To format SQL when you save, turn on <b>$FORMAT_ON_SAVE</b> under " +
					"Tools | Actions on Save.",
			)
		}
	}

	override fun apply() {
		val path = state.path
		val highlight = state.highlightEmbeddedSql
		super.apply()
		if (state.path != path) {
			SquillExecutable.getInstance().reset()
		}
		if (state.highlightEmbeddedSql != highlight) {
			for (project in ProjectManager.getInstance().openProjects) {
				DaemonCodeAnalyzer.getInstance(project).restart("squill settings changed")
			}
		}
	}
}
