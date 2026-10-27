package dev.oxi.android.ui

import android.content.ClipData
import android.content.ClipboardManager
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.IntrinsicSize
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.CheckBox
import androidx.compose.material.icons.rounded.CheckBoxOutlineBlank
import androidx.compose.material.icons.rounded.ContentCopy
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.delay

// ---- blocks ---------------------------------------------------------------------------------

private sealed interface MdBlock {
    data class Para(val text: String) : MdBlock
    data class Heading(val level: Int, val text: String) : MdBlock
    /** `checked` is null for a plain item, true/false for `- [x]` / `- [ ]`. */
    data class Item(val marker: String, val text: String, val indent: Int, val checked: Boolean?) : MdBlock
    data class Quote(val text: String) : MdBlock
    data class Code(val lang: String, val text: String) : MdBlock
    data class Table(val header: List<String>, val rows: List<List<String>>, val align: List<Char>) : MdBlock
    data object Rule : MdBlock
}

private val listRe = Regex("""^(\s*)([-*+]|\d+[.)])\s+(.*)$""")
private val taskRe = Regex("""^\[([ xX])]\s+(.*)$""")
private val headingRe = Regex("""^(#{1,6})\s+(.*?)\s*#*$""")
private val ruleRe = Regex("""^([-*_])(\s*\1){2,}$""")
private val separatorCellRe = Regex("""^:?-{2,}:?$""")

private fun parse(src: String): List<MdBlock> {
    val out = mutableListOf<MdBlock>()
    val para = StringBuilder()
    fun flush() {
        if (para.isNotBlank()) out += MdBlock.Para(para.toString().trim())
        para.clear()
    }
    val lines = src.lines()
    var i = 0
    while (i < lines.size) {
        val line = lines[i]
        val trimmed = line.trimStart()
        when {
            trimmed.startsWith("```") || trimmed.startsWith("~~~") -> {
                flush()
                val fence = trimmed.take(3)
                val lang = trimmed.removePrefix(fence).trim()
                val body = StringBuilder()
                i++
                // A reply still streaming may not have closed its fence yet.
                while (i < lines.size && !lines[i].trimStart().startsWith(fence)) {
                    body.appendLine(lines[i]); i++
                }
                out += MdBlock.Code(lang, body.toString().trimEnd('\n'))
            }
            line.isBlank() -> flush()
            headingRe.matches(trimmed) -> {
                flush()
                val m = headingRe.find(trimmed)!!
                out += MdBlock.Heading(m.groupValues[1].length, m.groupValues[2])
            }
            ruleRe.matches(trimmed) -> { flush(); out += MdBlock.Rule }
            listRe.matches(line) -> {
                flush()
                val m = listRe.find(line)!!
                val raw = m.groupValues[2]
                var text = m.groupValues[3]
                var checked: Boolean? = null
                taskRe.find(text)?.let { t -> checked = t.groupValues[1] != " "; text = t.groupValues[2] }
                val marker = if (raw[0].isDigit()) raw.trimEnd(')').trimEnd('.') + "." else "•"
                // Continuation lines indented under the item belong to it.
                while (i + 1 < lines.size && lines[i + 1].isNotBlank() && lines[i + 1].startsWith("  ") &&
                    !listRe.matches(lines[i + 1]) && !lines[i + 1].trimStart().startsWith("```")
                ) {
                    text += " " + lines[i + 1].trim(); i++
                }
                out += MdBlock.Item(marker, text, m.groupValues[1].replace("\t", "    ").length / 2, checked)
            }
            trimmed.startsWith(">") -> {
                flush()
                val body = StringBuilder()
                while (i < lines.size && lines[i].trimStart().startsWith(">")) {
                    if (body.isNotEmpty()) body.append('\n')
                    body.append(lines[i].trimStart().removePrefix(">").removePrefix(" ")); i++
                }
                out += MdBlock.Quote(body.toString())
                continue
            }
            trimmed.startsWith("|") -> {
                flush()
                val rows = mutableListOf<String>()
                while (i < lines.size && lines[i].trimStart().startsWith("|")) {
                    rows += lines[i].trim(); i++
                }
                out += table(rows)
                continue
            }
            else -> {
                if (para.isNotEmpty()) para.append(if (line.endsWith("  ")) "\n" else " ")
                para.append(line.trim())
            }
        }
        i++
    }
    flush()
    return out
}

private fun cells(row: String): List<String> = row.trim().removePrefix("|").removeSuffix("|").split('|').map { it.trim() }

private fun table(rows: List<String>): MdBlock {
    val parsed = rows.map(::cells)
    val sepIndex = parsed.indexOfFirst { r -> r.isNotEmpty() && r.all { separatorCellRe.matches(it) } }
    val align = if (sepIndex >= 0) parsed[sepIndex].map { c ->
        when {
            c.startsWith(":") && c.endsWith(":") -> 'c'
            c.endsWith(":") -> 'r'
            else -> 'l'
        }
    } else emptyList()
    val body = parsed.filterIndexed { n, _ -> n != sepIndex }
    val header = if (sepIndex == 1) body.first() else emptyList()
    return MdBlock.Table(header, if (sepIndex == 1) body.drop(1) else body, align)
}

// ---- inline ---------------------------------------------------------------------------------

private val codeInline = Color(0xFFF0B68A)
private val urlRe = Regex("""https?://[^\s<>()\[\]]+[^\s<>()\[\].,;:!?'"]""")

/** `**bold**`, `*italic*`, `***both***`, `~~strike~~`, `` `code` ``, `[text](url)`, bare URLs. */
fun inline(text: String): AnnotatedString = buildAnnotatedString { appendInline(text) }

private fun AnnotatedString.Builder.link(label: String, url: String) {
    withLink(
        LinkAnnotation.Url(
            url,
            TextLinkStyles(SpanStyle(color = Oxi.copper, textDecoration = TextDecoration.Underline)),
        ),
    ) { append(label) }
}

private fun AnnotatedString.Builder.appendInline(text: String) {
    var i = 0
    val plain = StringBuilder()
    fun flushPlain() {
        if (plain.isEmpty()) return
        // Bare links inside the plain run.
        val s = plain.toString()
        var last = 0
        for (m in urlRe.findAll(s)) {
            append(s.substring(last, m.range.first))
            link(m.value, m.value)
            last = m.range.last + 1
        }
        append(s.substring(last))
        plain.clear()
    }
    while (i < text.length) {
        val c = text[i]
        when {
            c == '\\' && i + 1 < text.length && text[i + 1] in "\\`*_[]()#+-.!~|" -> { plain.append(text[i + 1]); i += 2 }
            c == '`' -> {
                val ticks = text.drop(i).takeWhile { it == '`' }.length
                val end = text.indexOf("`".repeat(ticks), i + ticks)
                if (end < 0) { plain.append(c); i++; continue }
                flushPlain()
                withStyle(SpanStyle(fontFamily = Oxi.mono, background = Oxi.raised, fontSize = 13.5.sp, color = codeInline)) {
                    val code = text.substring(i + ticks, end).trim()
                    // Short code stays on one line: no break at its spaces.
                    append("\u2009" + (if (code.length <= 28) code.replace(' ', '\u00A0') else code) + "\u2009")
                }
                i = end + ticks
            }
            text.startsWith("***", i) -> {
                val end = text.indexOf("***", i + 3)
                if (end < 0) { plain.append("***"); i += 3; continue }
                flushPlain()
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, fontStyle = FontStyle.Italic, color = Oxi.text)) {
                    appendInline(text.substring(i + 3, end))
                }
                i = end + 3
            }
            text.startsWith("**", i) || text.startsWith("__", i) -> {
                val mark = text.substring(i, i + 2)
                val end = text.indexOf(mark, i + 2)
                if (end < 0) { plain.append(mark); i += 2; continue }
                flushPlain()
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, color = Oxi.text)) { appendInline(text.substring(i + 2, end)) }
                i = end + 2
            }
            text.startsWith("~~", i) -> {
                val end = text.indexOf("~~", i + 2)
                if (end < 0) { plain.append("~~"); i += 2; continue }
                flushPlain()
                withStyle(SpanStyle(textDecoration = TextDecoration.LineThrough, color = Oxi.muted)) { appendInline(text.substring(i + 2, end)) }
                i = end + 2
            }
            (c == '*' || c == '_') && i + 1 < text.length && !text[i + 1].isWhitespace() &&
                (i == 0 || !text[i - 1].isLetterOrDigit()) -> {
                val end = text.indexOf(c, i + 1)
                if (end < 0 || text[end - 1].isWhitespace()) { plain.append(c); i++; continue }
                flushPlain()
                withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { appendInline(text.substring(i + 1, end)) }
                i = end + 1
            }
            c == '[' -> {
                val close = text.indexOf("](", i)
                val end = if (close > 0) text.indexOf(')', close) else -1
                if (close < 0 || end < 0) { plain.append(c); i++; continue }
                flushPlain()
                val label = text.substring(i + 1, close)
                val url = text.substring(close + 2, end).substringBefore(' ')
                if (url.startsWith("http")) link(label, url)
                else withStyle(SpanStyle(color = Oxi.copper)) { append(label) }
                i = end + 1
            }
            else -> { plain.append(c); i++ }
        }
    }
    flushPlain()
}

// ---- code -----------------------------------------------------------------------------------

private val keywords = setOf(
    "fn", "let", "mut", "pub", "struct", "enum", "impl", "trait", "use", "mod", "match", "if", "else",
    "for", "while", "loop", "return", "break", "continue", "in", "as", "const", "static", "async", "await",
    "def", "class", "import", "from", "with", "try", "except", "finally", "raise", "lambda", "yield", "pass",
    "function", "var", "export", "default", "new", "this", "self", "Self", "super", "extends", "interface",
    "type", "val", "fun", "object", "when", "is", "null", "nil", "None", "true", "false", "True", "False",
    "package", "private", "public", "protected", "override", "where", "dyn", "ref", "move", "unsafe", "go",
    "func", "defer", "chan", "select", "case", "switch", "do", "then", "fi", "done", "echo", "export",
)
private val hlKeyword = Color(0xFFE8915A)
private val hlString = Color(0xFF9FCB86)
private val hlComment = Color(0xFF6B6E78)
private val hlNumber = Color(0xFFD7A65F)
private val hlType = Color(0xFF8DB4E2)
private val tokenRe = Regex(
    """(//[^\n]*|#(?![\[!])[^\n]*|/\*[\s\S]*?\*/)|("(?:\\.|[^"\\])*"|'(?:\\.|[^'\\\n])*'|`[^`]*`)|\b(\d[\d_]*(?:\.\d+)?)\b|\b([A-Za-z_][A-Za-z0-9_]*)\b""",
)

/** A light, language-agnostic highlighter: comments, strings, numbers, keywords, Types. */
private fun highlight(code: String, lang: String): AnnotatedString = buildAnnotatedString {
    val hashComments = lang.lowercase() in setOf("", "sh", "bash", "zsh", "shell", "py", "python", "toml", "yaml", "yml", "rb", "ruby", "console")
    if (lang.lowercase() in setOf("diff", "patch")) {
        code.lines().forEachIndexed { n, line ->
            if (n > 0) append('\n')
            val color = when {
                line.startsWith("+") -> Oxi.green
                line.startsWith("-") -> Oxi.red
                line.startsWith("@@") -> hlType
                else -> Oxi.muted
            }
            withStyle(SpanStyle(color = color)) { append(line) }
        }
        return@buildAnnotatedString
    }
    var last = 0
    for (m in tokenRe.findAll(code)) {
        append(code.substring(last, m.range.first))
        val (comment, string, number, word) = m.destructured
        val style = when {
            comment.isNotEmpty() && (comment.startsWith("#") && !hashComments) -> null
            comment.isNotEmpty() -> SpanStyle(color = hlComment, fontStyle = FontStyle.Italic)
            string.isNotEmpty() -> SpanStyle(color = hlString)
            number.isNotEmpty() -> SpanStyle(color = hlNumber)
            word in keywords -> SpanStyle(color = hlKeyword)
            word.isNotEmpty() && word[0].isUpperCase() && word.any { it.isLowerCase() } -> SpanStyle(color = hlType)
            else -> null
        }
        if (style != null) withStyle(style) { append(m.value) } else append(m.value)
        last = m.range.last + 1
    }
    append(code.substring(last))
}

@Composable
fun CodeBox(text: String, modifier: Modifier = Modifier, label: String? = null) {
    val context = LocalContext.current
    var copied by remember { mutableStateOf(false) }
    LaunchedEffect(copied) { if (copied) { delay(1500); copied = false } }
    val highlighted = remember(text, label) { highlight(text, label.orEmpty()) }
    Column(
        modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .background(Oxi.codeBg)
            .border(1.dp, Oxi.line.copy(alpha = 0.6f), RoundedCornerShape(12.dp)),
    ) {
        if (label != null) {
            Row(
                Modifier.fillMaxWidth().background(Oxi.surface).padding(start = 12.dp, end = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(label.ifBlank { "code" }, style = MonoSmall, color = Oxi.faint, modifier = Modifier.weight(1f))
                Row(
                    Modifier
                        .clip(RoundedCornerShape(8.dp))
                        .clickable {
                            context.getSystemService(ClipboardManager::class.java)
                                ?.setPrimaryClip(ClipData.newPlainText("code", text))
                            copied = true
                        }
                        .padding(horizontal = 8.dp, vertical = 7.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(
                        if (copied) Icons.Rounded.Check else Icons.Rounded.ContentCopy,
                        "Copy",
                        tint = if (copied) Oxi.green else Oxi.faint,
                        modifier = Modifier.size(14.dp),
                    )
                    Text(if (copied) "Copied" else "Copy", style = MonoSmall, color = Oxi.faint, modifier = Modifier.padding(start = 5.dp))
                }
            }
        }
        Box(Modifier.horizontalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 10.dp)) {
            Text(highlighted, style = MonoSmall.copy(fontSize = 12.5.sp, lineHeight = 18.sp), color = Oxi.text)
        }
    }
}

// ---- table ----------------------------------------------------------------------------------

@Composable
private fun TableView(block: MdBlock.Table) {
    val columns = maxOf(block.header.size, block.rows.maxOfOrNull { it.size } ?: 0)
    if (columns == 0) return
    val measurer = rememberTextMeasurer()
    val density = LocalDensity.current
    val cellStyle = MaterialTheme.typography.bodyMedium
    val headerStyle = cellStyle.copy(fontWeight = FontWeight.SemiBold)
    // Each column as wide as its widest cell (capped), measured as rendered.
    val natural = remember(block, cellStyle) {
        (0 until columns).map { c ->
            val widest = (listOf(block.header) + block.rows).withIndex().maxOf { (n, row) ->
                val style = if (n == 0 && block.header.isNotEmpty()) headerStyle else cellStyle
                measurer.measure(inline(row.getOrNull(c).orEmpty()), style, softWrap = false).size.width
            }
            with(density) { widest.toDp() + 24.dp }.coerceIn(48.dp, 280.dp)
        }
    }
    BoxWithConstraints(
        Modifier
            .fillMaxWidth()
            .clip(RoundedCornerShape(12.dp))
            .border(1.dp, Oxi.line, RoundedCornerShape(12.dp)),
    ) {
        // A table narrower than the screen spreads the slack over its columns.
        val total = natural.fold(0.dp) { a, w -> a + w }
        val widths = if (total < maxWidth) natural.map { it + (maxWidth - total) / columns } else natural
        val full = widths.fold(0.dp) { a, w -> a + w }
        Column(Modifier.horizontalScroll(rememberScrollState())) {
            if (block.header.isNotEmpty()) TableRow(block.header, widths, block.align, header = true, shaded = false)
            block.rows.forEachIndexed { n, row ->
                if (n > 0 || block.header.isNotEmpty()) Box(Modifier.width(full).height(1.dp).background(Oxi.line))
                TableRow(row, widths, block.align, header = false, shaded = n % 2 == 1)
            }
        }
    }
}

@Composable
private fun TableRow(cells: List<String>, widths: List<Dp>, align: List<Char>, header: Boolean, shaded: Boolean) {
    Row(Modifier.background(if (header) Oxi.surface else if (shaded) Oxi.surface.copy(alpha = 0.5f) else Color.Transparent)) {
        widths.forEachIndexed { c, w ->
            Text(
                inline(cells.getOrNull(c).orEmpty()),
                style = MaterialTheme.typography.bodyMedium.copy(
                    fontWeight = if (header) FontWeight.SemiBold else FontWeight.Normal,
                    textAlign = when (align.getOrNull(c)) {
                        'r' -> TextAlign.End
                        'c' -> TextAlign.Center
                        else -> TextAlign.Start
                    },
                ),
                color = if (header) Oxi.text else Oxi.text.copy(alpha = 0.92f),
                modifier = Modifier.width(w).padding(horizontal = 12.dp, vertical = 9.dp),
            )
        }
    }
}

// ---- document -------------------------------------------------------------------------------

@Composable
fun Markdown(text: String, modifier: Modifier = Modifier) {
    val blocks = remember(text) { parse(text) }
    SelectionContainer(modifier) {
        Column {
            blocks.forEachIndexed { n, block ->
                val prev = blocks.getOrNull(n - 1)
                // Tighter between list items, roomier around headings and boxes.
                val gap = when {
                    n == 0 -> 0.dp
                    block is MdBlock.Heading -> if (block.level <= 2) 18.dp else 14.dp
                    block is MdBlock.Item && prev is MdBlock.Item -> 5.dp
                    prev is MdBlock.Heading -> 8.dp
                    else -> 11.dp
                }
                Box(Modifier.padding(top = gap)) { MdBlockView(block) }
            }
        }
    }
}

@Composable
private fun MdBlockView(block: MdBlock) {
    val body = MaterialTheme.typography.bodyLarge
    when (block) {
        is MdBlock.Para -> Text(inline(block.text), style = body)
        is MdBlock.Heading -> Column {
            Text(
                inline(block.text),
                style = when (block.level) {
                    1 -> MaterialTheme.typography.titleLarge.copy(fontSize = 21.sp, lineHeight = 27.sp, fontWeight = FontWeight.Bold)
                    2 -> MaterialTheme.typography.titleMedium.copy(fontSize = 18.sp, lineHeight = 24.sp, fontWeight = FontWeight.SemiBold)
                    3 -> MaterialTheme.typography.titleSmall.copy(fontSize = 16.sp, lineHeight = 22.sp, fontWeight = FontWeight.SemiBold)
                    else -> MaterialTheme.typography.titleSmall.copy(fontSize = 15.sp, color = Oxi.muted)
                },
            )
            if (block.level <= 2) {
                Box(Modifier.padding(top = 6.dp).fillMaxWidth().height(1.dp).background(Oxi.line.copy(alpha = 0.7f)))
            }
        }
        is MdBlock.Item -> Row(Modifier.padding(start = (block.indent * 18).dp)) {
            Box(Modifier.width(24.dp).padding(top = 1.dp), contentAlignment = Alignment.TopStart) {
                when {
                    block.checked != null -> Icon(
                        if (block.checked) Icons.Rounded.CheckBox else Icons.Rounded.CheckBoxOutlineBlank,
                        null,
                        tint = if (block.checked) Oxi.copper else Oxi.faint,
                        modifier = Modifier.size(18.dp).padding(top = 2.dp),
                    )
                    block.marker == "•" -> Text(
                        if (block.indent % 2 == 0) "•" else "◦",
                        style = body.copy(fontWeight = FontWeight.Bold),
                        color = Oxi.copper.copy(alpha = 0.85f),
                    )
                    else -> Text(block.marker, style = body.copy(fontFeatureSettings = "tnum"), color = Oxi.muted)
                }
            }
            Text(
                inline(block.text),
                style = body,
                color = if (block.checked == true) Oxi.muted else Oxi.text,
            )
        }
        is MdBlock.Quote -> Row(Modifier.height(IntrinsicSize.Min)) {
            Box(Modifier.width(3.dp).fillMaxHeight().clip(RoundedCornerShape(2.dp)).background(Oxi.copper.copy(alpha = 0.55f)))
            Column(Modifier.padding(start = 12.dp)) {
                parse(block.text).forEach { inner ->
                    when (inner) {
                        is MdBlock.Para -> Text(inline(inner.text), style = body.copy(fontStyle = FontStyle.Italic), color = Oxi.muted)
                        else -> MdBlockView(inner)
                    }
                }
            }
        }
        is MdBlock.Code -> CodeBox(block.text, label = block.lang)
        is MdBlock.Table -> TableView(block)
        MdBlock.Rule -> Box(Modifier.padding(vertical = 6.dp).fillMaxWidth().height(1.dp).background(Oxi.line))
    }
}
