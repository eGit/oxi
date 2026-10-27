package dev.oxi.android.ui

import android.content.ClipboardManager
import android.content.Context
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.rounded.ArrowBack
import androidx.compose.material.icons.rounded.Add
import androidx.compose.material.icons.rounded.Check
import androidx.compose.material.icons.rounded.ContentPaste
import androidx.compose.material.icons.rounded.Dns
import androidx.compose.material.icons.rounded.ExpandMore
import androidx.compose.material.icons.rounded.MoreVert
import androidx.compose.material.icons.rounded.QrCodeScanner
import androidx.compose.material.icons.rounded.SystemUpdate
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning
import dev.oxi.android.ConnState
import dev.oxi.android.Endpoint
import dev.oxi.android.OxiViewModel
import dev.oxi.android.R
import dev.oxi.android.Server
import dev.oxi.android.UiState
import dev.oxi.android.UpdateState
import dev.oxi.android.BuildConfig
import dev.oxi.android.MainActivity

/** Google's scanner (Play services): returns the QR code's text, no camera permission. */
fun scanPairCode(context: Context, vm: OxiViewModel) {
    val options = GmsBarcodeScannerOptions.Builder()
        .setBarcodeFormats(Barcode.FORMAT_QR_CODE)
        .enableAutoZoom()
        .build()
    GmsBarcodeScanning.getClient(context, options).startScan()
        .addOnSuccessListener { code ->
            if (!vm.pairFromText(code.rawValue.orEmpty())) vm.toast("That QR code isn't an oxi pair code")
        }
        .addOnFailureListener { vm.toast("Scanner unavailable: ${it.message}") }
}

private fun clipboardText(context: Context): String? {
    val clipboard = context.getSystemService(ClipboardManager::class.java) ?: return null
    return clipboard.primaryClip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.coerceToText(context)?.toString()
}

@Composable
fun AddServerScreen(vm: OxiViewModel, state: UiState) {
    val context = LocalContext.current
    val canGoBack = state.servers.isNotEmpty()
    var manual by remember { mutableStateOf(false) }
    var host by remember { mutableStateOf("") }
    var port by remember { mutableStateOf("47811") }
    var token by remember { mutableStateOf("") }
    BackHandler(enabled = canGoBack) { vm.showAddServer(false) }
    Column(
        Modifier
            .fillMaxSize()
            .statusBarsPadding()
            .navigationBarsPadding()
            .imePadding()
            .verticalScroll(rememberScrollState())
            .padding(horizontal = 24.dp, vertical = 8.dp),
        verticalArrangement = Arrangement.spacedBy(14.dp),
    ) {
        if (canGoBack) {
            IconButton(onClick = { vm.showAddServer(false) }, modifier = Modifier.padding(start = 0.dp)) {
                Icon(Icons.AutoMirrored.Rounded.ArrowBack, "Back")
            }
        } else {
            Spacer(Modifier.height(36.dp))
        }
        Image(painterResource(R.mipmap.ic_launcher_foreground), null, Modifier.size(88.dp))
        Text(
            if (canGoBack) "Add a server" else "Connect to your computer",
            style = MaterialTheme.typography.headlineSmall,
            fontWeight = FontWeight.SemiBold,
        )
        Text(
            "On the computer, open oxi → Settings → Server & phone and turn on “Allow phones to connect”, " +
                "or run this in a terminal:",
            color = Oxi.muted,
        )
        CodeBox("oxi server phone")
        Text(
            "Both devices need Tailscale (tailscale.com) signed in to the same account.",
            color = Oxi.faint,
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(4.dp))
        Button(
            onClick = { scanPairCode(context, vm) },
            modifier = Modifier.fillMaxWidth().height(54.dp),
            shape = RoundedCornerShape(14.dp),
        ) {
            Icon(Icons.Rounded.QrCodeScanner, null)
            Text("  Scan QR code", fontSize = 16.sp)
        }
        OutlinedButton(
            onClick = {
                val text = clipboardText(context)
                if (text == null || !vm.pairFromText(text)) vm.toast("Copy the pair link (oxi://pair…) first")
            },
            modifier = Modifier.fillMaxWidth().height(50.dp),
            shape = RoundedCornerShape(14.dp),
            colors = ButtonDefaults.outlinedButtonColors(contentColor = Oxi.text),
        ) {
            Icon(Icons.Rounded.ContentPaste, null, modifier = Modifier.size(18.dp))
            Text("  Paste pair link")
        }
        Row(
            Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp)).clickable { manual = !manual }.padding(vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("Enter manually", color = Oxi.muted, modifier = Modifier.weight(1f))
            Icon(Icons.Rounded.ExpandMore, null, tint = Oxi.faint, modifier = Modifier.rotate(if (manual) 180f else 0f))
        }
        AnimatedVisibility(manual) {
            Column(verticalArrangement = Arrangement.spacedBy(12.dp)) {
                OutlinedTextField(host, { host = it.trim() }, label = { Text("Host (Tailscale name or IP)") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                OutlinedTextField(
                    port, { port = it.filter(Char::isDigit) }, label = { Text("Port") }, singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number), modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(token, { token = it.trim() }, label = { Text("Token") }, singleLine = true, modifier = Modifier.fillMaxWidth())
                Button(
                    onClick = { vm.addServer(Endpoint(host, port.toIntOrNull() ?: 47811, token), null) },
                    enabled = host.isNotBlank() && token.isNotBlank(),
                    modifier = Modifier.fillMaxWidth().height(50.dp),
                    shape = RoundedCornerShape(14.dp),
                ) { Text("Connect") }
            }
        }
        Spacer(Modifier.height(24.dp))
    }
}

/** The server picker under the home screen's title. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServersSheet(vm: OxiViewModel, state: UiState, onDismiss: () -> Unit) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    var renaming by remember { mutableStateOf<Server?>(null) }
    var removing by remember { mutableStateOf<Server?>(null) }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet, containerColor = Oxi.surface) {
        Column(Modifier.fillMaxWidth().padding(bottom = 24.dp)) {
            Box(Modifier.padding(horizontal = 16.dp)) { SectionLabel("Servers") }
            state.servers.forEach { server ->
                val active = server.id == state.activeId
                var menu by remember { mutableStateOf(false) }
                Row(
                    Modifier
                        .fillMaxWidth()
                        .clickable { vm.switchServer(server.id); onDismiss() }
                        .padding(start = 20.dp, end = 8.dp, top = 6.dp, bottom = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(
                        Modifier.size(36.dp).clip(RoundedCornerShape(10.dp)).background(if (active) Oxi.copperDim else Oxi.raised),
                        contentAlignment = Alignment.Center,
                    ) { Icon(Icons.Rounded.Dns, null, tint = if (active) Oxi.copper else Oxi.muted, modifier = Modifier.size(18.dp)) }
                    Column(Modifier.weight(1f).padding(horizontal = 14.dp)) {
                        Text(server.name, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        val detail = if (active) when (val c = state.conn) {
                            is ConnState.Connected -> "Connected · oxi ${c.version}"
                            ConnState.Connecting -> "Connecting…"
                            is ConnState.Failed -> "Offline · ${c.message}"
                            ConnState.Idle -> ""
                        } else "${server.host}:${server.port}"
                        Text(detail, style = MaterialTheme.typography.bodySmall, color = Oxi.faint, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                    if (active) Icon(Icons.Rounded.Check, null, tint = Oxi.copper)
                    Box {
                        IconButton(onClick = { menu = true }) { Icon(Icons.Rounded.MoreVert, "More", tint = Oxi.muted) }
                        DropdownMenu(menu, { menu = false }) {
                            DropdownMenuItem(text = { Text("Rename") }, onClick = { menu = false; renaming = server })
                            DropdownMenuItem(text = { Text("Remove", color = Oxi.red) }, onClick = { menu = false; removing = server })
                        }
                    }
                }
            }
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { onDismiss(); vm.showAddServer(true) }
                    .padding(horizontal = 20.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.size(36.dp), contentAlignment = Alignment.Center) { Icon(Icons.Rounded.Add, null, tint = Oxi.copper) }
                Text("Add server", color = Oxi.copper, modifier = Modifier.padding(start = 14.dp))
            }
        }
    }
    renaming?.let { server ->
        var name by remember { mutableStateOf(server.name) }
        AlertDialog(
            onDismissRequest = { renaming = null },
            containerColor = Oxi.surface,
            title = { Text("Rename server") },
            text = { OutlinedTextField(name, { name = it }, singleLine = true) },
            confirmButton = { TextButton(onClick = { vm.renameServer(server.id, name); renaming = null }) { Text("Save") } },
            dismissButton = { TextButton(onClick = { renaming = null }) { Text("Cancel") } },
        )
    }
    removing?.let { server ->
        AlertDialog(
            onDismissRequest = { removing = null },
            containerColor = Oxi.surface,
            title = { Text("Remove ${server.name}?") },
            text = { Text("Chats stay on the server. Pair again to add it back.", color = Oxi.muted) },
            confirmButton = {
                TextButton(onClick = { vm.removeServer(server.id); removing = null; onDismiss() }) { Text("Remove", color = Oxi.red) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text("Cancel") } },
        )
    }
}

@Composable
fun ServerTitle(state: UiState, onClick: () -> Unit) {
    Row(
        Modifier.clip(RoundedCornerShape(10.dp)).clickable(onClick = onClick).padding(horizontal = 6.dp, vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        ConnDot(state.conn)
        Column(Modifier.padding(start = 10.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    state.active?.name ?: "oxi",
                    style = MaterialTheme.typography.titleMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Icon(Icons.Rounded.ExpandMore, "Switch server", tint = Oxi.muted, modifier = Modifier.size(20.dp))
            }
            val sub = when (val c = state.conn) {
                is ConnState.Connected -> "oxi ${c.version}"
                ConnState.Connecting -> "Connecting…"
                is ConnState.Failed -> "Offline · retrying"
                ConnState.Idle -> ""
            }
            Text(sub, style = MaterialTheme.typography.bodySmall, color = Oxi.muted, maxLines = 1)
        }
    }
}

/** A newer oxi for Android: downloading, ready to install, or failed. Hidden otherwise. */
@Composable
fun UpdateBanner(vm: OxiViewModel, state: UiState) {
    val update = state.update
    if (update is UpdateState.None) return
    val context = LocalContext.current
    Row(
        Modifier
            .fillMaxWidth()
            .padding(top = 4.dp, bottom = 4.dp)
            .clip(RoundedCornerShape(14.dp))
            .background(Oxi.copperDim)
            .padding(start = 16.dp, end = 8.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(Icons.Rounded.SystemUpdate, null, tint = Oxi.copper, modifier = Modifier.size(20.dp))
        val (title, detail) = when (update) {
            is UpdateState.Downloading -> "Downloading oxi ${update.version}" to "${(update.progress * 100).toInt()}%"
            is UpdateState.Ready -> "oxi ${update.version} is ready" to "You're on ${BuildConfig.VERSION_NAME}"
            is UpdateState.Installing -> "Installing oxi ${update.version}…" to "oxi restarts when it's done"
            is UpdateState.Failed -> "Couldn't update to ${update.version}" to update.message
            UpdateState.None -> "" to ""
        }
        Column(Modifier.weight(1f).padding(horizontal = 12.dp)) {
            Text(title, style = MaterialTheme.typography.bodyMedium, fontWeight = FontWeight.Medium)
            Text(detail, style = MaterialTheme.typography.bodySmall, color = Oxi.muted, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        when (update) {
            is UpdateState.Ready -> TextButton(onClick = { (context as? MainActivity)?.installUpdate() }) {
                Text("Update", color = Oxi.copper, fontWeight = FontWeight.SemiBold)
            }
            is UpdateState.Failed -> TextButton(onClick = { vm.checkForUpdate(force = true) }) { Text("Retry", color = Oxi.copper) }
            else -> CircularProgressIndicator(Modifier.size(18.dp).padding(end = 8.dp), strokeWidth = 2.dp, color = Oxi.copper)
        }
    }
}
