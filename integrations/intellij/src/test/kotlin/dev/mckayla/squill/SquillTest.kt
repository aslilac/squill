package dev.mckayla.squill

import com.intellij.ide.trustedProjects.TrustedProjects
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.progress.EmptyProgressIndicator
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspClientManager
import com.intellij.platform.lsp.api.LspServerState
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.PsiTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.testFramework.fixtures.IdeaTestFixtureFactory
import com.intellij.testFramework.fixtures.TempDirTestFixture
import java.io.File
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.DelicateCoroutinesApi
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.GlobalScope
import kotlinx.coroutines.async

// Runs the real squill ($SQUILL) behind the IDE's LSP client.
class SquillTest : BasePlatformTestCase() {
	private val squill = System.getenv("SQUILL") ?: "squill"
	private lateinit var root: VirtualFile

	// Files on disk, as squill needs: it reads squill.toml beside them.
	override fun createTempDirTestFixture(): TempDirTestFixture =
		IdeaTestFixtureFactory.getFixtureFactory().createTempDirTestFixture()

	override fun setUp() {
		super.setUp()
		SquillSettings.getInstance().state.path = squill
		// The IDE runs language servers only in trusted projects.
		TrustedProjects.setProjectTrusted(project, true)
		// And only for files in the project.
		root = myFixture.tempDirFixture.getFile("")!!
		PsiTestUtil.addContentRoot(module, root)
	}

	override fun tearDown() {
		try {
			LspClientManager.getInstance(project)
				.stopClients(SquillLspIntegrationProvider::class.java)
			PsiTestUtil.removeContentEntry(module, root)
			SquillSettings.getInstance().loadState(SquillState())
			SquillProjectSettings.getInstance(project).loadState(SquillProjectState())
		} catch (e: Throwable) {
			addSuppressedException(e)
		} finally {
			super.tearDown()
		}
	}

	fun testReformatCodeFormatsSqlFiles() {
		val text = "SELECT  id,name FROM users WHERE id=1"
		val file = open("query.sql", text)
		// Formatting waits for the IDE to hand squill the file, just after
		// it starts; until then, Reformat Code does nothing.
		PlatformTestUtil.waitWithEventsDispatching(
			"Reformat Code never applied squill's edit",
			{
				WriteCommandAction.runWriteCommandAction(project) {
					CodeStyleManager.getInstance(project).reformat(myFixture.file)
				}
				PlatformTestUtil.dispatchAllEventsInIdeEventQueue()
				document(file).text != text
			},
			30,
		)
		assertEquals(fmt(file, text), document(file).text)
	}

	@OptIn(DelicateCoroutinesApi::class, ExperimentalCoroutinesApi::class)
	fun testFormatsEmbeddedSqlOnSave() {
		myFixture.tempDirFixture.createFile(
			"squill.toml",
			"[[embedded]]\ninclude = [\"**/*.py\"]\ngrammar = \"python\"\n",
		)
		val text = "def q(db):\n" +
			"    return db.execute(\"\"\"SELECT id, name FROM users\n" +
			"        WHERE id = 1\"\"\")\n"
		val file = open("app.py", text)
		SquillProjectSettings.getInstance(project).state.formatOnSave = true
		val action = SquillFormatOnSave()
		assertTrue(action.isEnabledForProject(project))
		val document = document(file)
		// As with Reformat Code, the first saves may come before squill has
		// the file.
		PlatformTestUtil.waitWithEventsDispatching(
			"formatting on save never changed the file",
			{
				val update = GlobalScope.async(Dispatchers.Default) {
					action.updateDocument(project, document)
				}
				while (!update.isCompleted) {
					PlatformTestUtil.dispatchAllEventsInIdeEventQueue()
				}
				update.getCompletionExceptionOrNull()?.let { throw it }
				document.text != text
			},
			30,
		)
		assertEquals(fmt(file, text), document(file).text)
		assertFalse(text == document(file).text)
	}

	fun testHighlightsEmbeddedSql() {
		myFixture.tempDirFixture.createFile(
			"squill.toml",
			"[[embedded]]\ninclude = [\"**/*.py\"]\ngrammar = \"python\"\n",
		)
		open(
			"app.py",
			"def q(db):\n" +
				"    return db.execute(\"\"\"select id from users\n" +
				"        where id = 1\"\"\")\n",
		)
		// The keywords in the string, in squill's keyword color.
		PlatformTestUtil.waitWithEventsDispatching(
			"squill's highlighting never arrived",
			{
				val keywords = myFixture.doHighlighting()
					.filter { it.forcedTextAttributesKey == SqlColors.KEYWORD }
					.map { myFixture.editor.document.getText(it.textRange()) }
				keywords.sorted() == listOf("from", "select", "where")
			},
			30,
		)
	}

	fun testDownloadsTheLatestRelease() {
		// Talks to GitHub, so only when asked.
		if (System.getenv("SQUILL_TEST_DOWNLOAD") == null) {
			return
		}
		val binary = Download.latest(EmptyProgressIndicator())
		assertEquals(binary to true, Download.installed())
		val process = ProcessBuilder(binary.toString(), "--version").start()
		assertTrue(process.waitFor(30, TimeUnit.SECONDS))
		assertTrue(process.inputStream.readAllBytes().decodeToString().startsWith("squill "))
	}

	fun testSettingsPageRoundTrips() {
		val configurable = SquillConfigurable()
		try {
			configurable.createComponent()
			configurable.reset()
			assertFalse(configurable.isModified)
			configurable.apply()
			assertEquals(squill, SquillSettings.getInstance().state.path)
		} finally {
			configurable.disposeUIResources()
		}
	}

	fun testLeavesFilesSquillDoesNotKnowAlone() {
		val file = myFixture.tempDirFixture.createFile("notes.txt", "select 1")
		assertFalse(isSupported(file))
		assertTrue(isSupported(myFixture.tempDirFixture.createFile("a.sql", "")))
	}

	// Open a file in an editor, as a user would, and wait for squill to
	// start for it.
	private fun open(name: String, text: String): VirtualFile {
		val file = myFixture.tempDirFixture.createFile(name, text)
		myFixture.configureFromExistingVirtualFile(file)
		// The test editor doesn't tell the LSP support about the file.
		LspClientManager.getInstance(project)
			.startClientsIfNeeded(SquillLspIntegrationProvider::class.java)
		PlatformTestUtil.waitWithEventsDispatching(
			"squill's language server never started",
			{
				LspClientManager.getInstance(project)
					.getClients(SquillLspIntegrationProvider::class.java)
					.any { it.state == LspServerState.Running }
			},
			30,
		)
		return file
	}

	private fun com.intellij.codeInsight.daemon.impl.HighlightInfo.textRange() =
		TextRange(startOffset, endOffset)

	private fun document(file: VirtualFile) =
		FileDocumentManager.getInstance().getDocument(file)!!

	// What `squill fmt` makes of `text`, as `file`.
	private fun fmt(file: VirtualFile, text: String): String {
		val process = ProcessBuilder(squill, "fmt", "--stdin-filepath", file.path)
			.directory(File(file.parent.path))
			.start()
		process.outputStream.use { it.write(text.toByteArray()) }
		val output = process.inputStream.readAllBytes().decodeToString()
		assertTrue(process.waitFor(30, TimeUnit.SECONDS))
		assertEquals(0, process.exitValue())
		return output
	}
}
