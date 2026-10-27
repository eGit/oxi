package dev.oxi.android

import android.app.Application
import android.net.Uri
import android.provider.Settings
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.lifecycleScope
import androidx.lifecycle.repeatOnLifecycle
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import android.content.Intent
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.SystemBarStyle
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.activity.viewModels
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import dev.oxi.android.ui.ChatScreen
import dev.oxi.android.ui.ChatsScreen
import dev.oxi.android.ui.Oxi
import dev.oxi.android.ui.OxiTheme
import dev.oxi.android.ui.AddServerScreen
import dev.oxi.android.ui.WorkspacesScreen

class OxiApp : Application()

class MainActivity : ComponentActivity() {
    private val vm: OxiViewModel by viewModels()

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge(
            statusBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
            navigationBarStyle = SystemBarStyle.dark(android.graphics.Color.TRANSPARENT),
        )
        super.onCreate(savedInstanceState)
        handle(intent)
        lifecycleScope.launch {
            repeatOnLifecycle(Lifecycle.State.STARTED) {
                Updater.events.collect { event ->
                    when (event) {
                        is Updater.InstallEvent.Confirm -> runCatching { startActivity(event.intent) }
                            .onFailure { vm.updateFailed("Couldn't open the installer: ${it.message}") }
                        is Updater.InstallEvent.Failed -> vm.updateFailed(event.message)
                    }
                }
            }
        }
        setContent {
            OxiTheme {
                val state by vm.ui.collectAsStateWithLifecycle()
                val context = LocalContext.current
                LaunchedEffect(state.toast) {
                    state.toast?.let {
                        Toast.makeText(context, it, Toast.LENGTH_SHORT).show()
                        vm.toastShown()
                    }
                }
                val screen = when {
                    state.servers.isEmpty() || state.adding -> 0
                    state.chat != null -> 3
                    state.root != null -> 2
                    else -> 1
                }
                BackHandler(enabled = screen >= 2) {
                    if (screen == 3) vm.closeChat() else vm.openWorkspace(null)
                }
                Box(Modifier.fillMaxSize().background(Oxi.bg)) {
                    AnimatedContent(
                        targetState = screen,
                        transitionSpec = { fadeIn(tween(160)) togetherWith fadeOut(tween(120)) },
                        label = "screen",
                    ) { s ->
                        when (s) {
                            0 -> AddServerScreen(vm, state)
                            1 -> WorkspacesScreen(vm, state)
                            2 -> ChatsScreen(vm, state)
                            else -> ChatScreen(vm, state)
                        }
                    }
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        handle(intent)
    }

    override fun onResume() {
        super.onResume()
        vm.onResume()
        // Back from Android's install confirmation without installing: offer the update again.
        // (When the update goes ahead, this process is replaced before the delay is up.)
        lifecycleScope.launch {
            delay(4000)
            vm.updateDismissed()
        }
    }

    /** Install the downloaded update, first sending the user to allow installs if needed. */
    fun installUpdate() {
        if (!vm.installUpdate()) {
            vm.toast("Allow oxi to install updates, then tap Update again")
            startActivity(
                Intent(Settings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName")),
            )
        }
    }

    private fun handle(intent: Intent?) {
        intent?.data?.let { vm.pairFromUri(it) }
    }
}
