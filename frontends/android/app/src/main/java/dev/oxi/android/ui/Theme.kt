package dev.oxi.android.ui

import androidx.compose.material3.LocalContentColor
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.sp

/** The desktop app's palette: graphite surfaces, copper accent. */
object Oxi {
    val bg = Color(0xFF101114)
    val surface = Color(0xFF17181C)
    val raised = Color(0xFF1F2126)
    val line = Color(0xFF2A2C32)
    val text = Color(0xFFE9E7E4)
    val muted = Color(0xFF8E9098)
    val faint = Color(0xFF5E6068)
    val copper = Color(0xFFE8833A)
    val copperDim = Color(0xFF3A2516)
    val green = Color(0xFF5DBB7A)
    val red = Color(0xFFE5645A)
    val yellow = Color(0xFFE2B84F)
    val codeBg = Color(0xFF0C0D10)
    val mono = FontFamily.Monospace
}

private val colors = darkColorScheme(
    primary = Oxi.copper,
    onPrimary = Color(0xFF1A0E05),
    primaryContainer = Oxi.copperDim,
    onPrimaryContainer = Color(0xFFFFD9BF),
    secondary = Oxi.muted,
    background = Oxi.bg,
    onBackground = Oxi.text,
    surface = Oxi.bg,
    onSurface = Oxi.text,
    surfaceVariant = Oxi.raised,
    onSurfaceVariant = Oxi.muted,
    surfaceContainer = Oxi.surface,
    surfaceContainerHigh = Oxi.raised,
    surfaceContainerHighest = Color(0xFF26282E),
    surfaceContainerLow = Oxi.surface,
    outline = Oxi.line,
    outlineVariant = Oxi.line,
    error = Oxi.red,
)

private val base = Typography()
private val typography = base.copy(
    titleLarge = base.titleLarge.copy(fontWeight = FontWeight.SemiBold, fontSize = 20.sp),
    titleMedium = base.titleMedium.copy(fontWeight = FontWeight.SemiBold),
    bodyLarge = base.bodyLarge.copy(fontSize = 15.5.sp, lineHeight = 23.sp),
    bodyMedium = base.bodyMedium.copy(fontSize = 14.sp, lineHeight = 20.sp),
)

val MonoSmall = TextStyle(fontFamily = FontFamily.Monospace, fontSize = 12.sp, lineHeight = 17.sp)

@Composable
fun OxiTheme(content: @Composable () -> Unit) {
    MaterialTheme(colorScheme = colors, typography = typography) {
        CompositionLocalProvider(LocalContentColor provides Oxi.text, content = content)
    }
}
