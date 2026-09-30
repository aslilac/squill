// Colors for the SQL embedded in other languages' strings, which squill's
// language server reports as semantic tokens. Each falls back to the
// color scheme's language defaults, and can be set on its own under
// Settings | Editor | Color Scheme | squill.
//
// Only the SQL's words, numbers, parameters and comments are colored; its
// strings, operators and punctuation keep the host's string color.

package dev.mckayla.squill

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.fileTypes.PlainSyntaxHighlighter
import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import javax.swing.Icon

object SqlColors {
	val KEYWORD = key("KEYWORD", DefaultLanguageHighlighterColors.KEYWORD)
	val NAME = key("NAME", DefaultLanguageHighlighterColors.LOCAL_VARIABLE)
	val FUNCTION = key("FUNCTION", DefaultLanguageHighlighterColors.FUNCTION_CALL)
	val TYPE = key("TYPE", DefaultLanguageHighlighterColors.CLASS_REFERENCE)
	val NUMBER = key("NUMBER", DefaultLanguageHighlighterColors.NUMBER)
	val PARAMETER = key("PARAMETER", DefaultLanguageHighlighterColors.PARAMETER)
	val COMMENT = key("COMMENT", DefaultLanguageHighlighterColors.LINE_COMMENT)

	private fun key(name: String, fallback: TextAttributesKey) =
		TextAttributesKey.createTextAttributesKey("SQUILL_SQL_$name", fallback)

	// The color for each of the server's token types; null for the ones
	// left in the host's string color.
	fun forTokenType(tokenType: String): TextAttributesKey? = when (tokenType) {
		"keyword" -> KEYWORD
		"variable" -> NAME
		"function" -> FUNCTION
		"type" -> TYPE
		"number" -> NUMBER
		"parameter" -> PARAMETER
		"comment" -> COMMENT
		else -> null
	}
}

class SqlColorSettingsPage : ColorSettingsPage {
	private val descriptors = arrayOf(
		AttributesDescriptor("Keyword", SqlColors.KEYWORD),
		AttributesDescriptor("Name (table, column, alias)", SqlColors.NAME),
		AttributesDescriptor("Function", SqlColors.FUNCTION),
		AttributesDescriptor("Type", SqlColors.TYPE),
		AttributesDescriptor("Number", SqlColors.NUMBER),
		AttributesDescriptor("Query parameter", SqlColors.PARAMETER),
		AttributesDescriptor("Comment", SqlColors.COMMENT),
	)

	private val tags = mapOf(
		"keyword" to SqlColors.KEYWORD,
		"name" to SqlColors.NAME,
		"function" to SqlColors.FUNCTION,
		"type" to SqlColors.TYPE,
		"number" to SqlColors.NUMBER,
		"parameter" to SqlColors.PARAMETER,
		"comment" to SqlColors.COMMENT,
	)

	override fun getDisplayName() = "squill"

	override fun getIcon(): Icon = SquillIcons.Squill

	override fun getHighlighter(): SyntaxHighlighter = PlainSyntaxHighlighter()

	override fun getDemoText() =
		"""
		<comment>-- The SQL in another language's strings</comment>
		<keyword>select</keyword> <name>u</name>.<name>id</name>, <function>lower</function>(<name>u</name>.<name>email</name>) <keyword>as</keyword> <name>email</name>
		<keyword>from</keyword> <name>users</name> <keyword>as</keyword> <name>u</name>
		<keyword>where</keyword> <name>u</name>.<name>created_at</name> > <parameter>${'$'}1</parameter>::<type>timestamptz</type>
		<keyword>limit</keyword> <number>10</number>
		""".trimIndent()

	override fun getAdditionalHighlightingTagToDescriptorMap() = tags

	override fun getAttributeDescriptors() = descriptors

	override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
}
