package dev.oxi.android

import android.app.Application
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.Uri
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import org.json.JSONArray
import org.json.JSONObject

/** The chat on screen. `id == null` until the first prompt of a new chat comes back. */
data class OpenChat(
    val root: String,
    val id: String?,
    val title: String,
    val config: ChatConfig?,
    val messages: List<Message> = emptyList(),
    val running: Boolean = false,
    val queued: Int = 0,
    val approval: Approval? = null,
    val status: String? = null,
    val error: String? = null,
    val loading: Boolean = false,
    val sending: Boolean = false,
)

/** A paired machine running `oxi server`. */
data class Server(val id: String, val name: String, val host: String, val port: Int, val token: String) {
    val endpoint get() = Endpoint(host, port, token)

    fun toJson(): JSONObject = JSONObject().put("id", id).put("name", name).put("host", host)
        .put("port", port).put("token", token)

    companion object {
        fun fromJson(o: JSONObject) = Server(
            o.optString("id"), o.optString("name"), o.optString("host"), o.optInt("port", 47811), o.optString("token"),
        )
    }
}

sealed interface UpdateState {
    data object None : UpdateState
    data class Downloading(val version: String, val progress: Float) : UpdateState
    data class Ready(val version: String, val apk: java.io.File) : UpdateState
    data class Installing(val version: String) : UpdateState
    data class Failed(val version: String, val message: String) : UpdateState
}

data class UiState(
    val update: UpdateState = UpdateState.None,
    val servers: List<Server> = emptyList(),
    val activeId: String? = null,
    /** The add-server screen is open (it is the only screen while no server is paired). */
    val adding: Boolean = false,
    val conn: ConnState = ConnState.Idle,
    val host: String = "",
    val home: String? = null,
    val providers: List<Provider> = emptyList(),
    val workspaces: List<Workspace> = emptyList(),
    val workspacesLoaded: Boolean = false,
    val root: String? = null,
    val chats: List<ChatSummary> = emptyList(),
    val chatsLoading: Boolean = false,
    val chat: OpenChat? = null,
    val models: Map<String, List<String>> = emptyMap(),
    val quota: Map<String, List<QuotaWindow>> = emptyMap(),
    val toast: String? = null,
) {
    val active: Server? get() = servers.firstOrNull { it.id == activeId }
    val endpoint: Endpoint? get() = active?.endpoint
}

class OxiViewModel(app: Application) : AndroidViewModel(app) {
    private val prefs = app.getSharedPreferences("oxi", Context.MODE_PRIVATE)
    private val client = OxiClient(viewModelScope)
    private val _ui = MutableStateFlow(UiState())
    val ui: StateFlow<UiState> = _ui

    init {
        viewModelScope.launch { client.state.collect { s -> _ui.update { it.copy(conn = s) } } }
        viewModelScope.launch { client.connected.collect { onConnected() } }
        viewModelScope.launch { client.notes.collect(::onNote) }
        val servers = loadServers()
        val active = prefs.getString("active", null)?.takeIf { id -> servers.any { it.id == id } } ?: servers.firstOrNull()?.id
        _ui.update { it.copy(servers = servers) }
        active?.let { switchServer(it) }
        watchNetwork(app)
    }

    // ---- servers ---------------------------------------------------------------------------

    private fun loadServers(): List<Server> {
        prefs.getString("servers", null)?.let { text ->
            val arr = runCatching { JSONArray(text) }.getOrNull() ?: return emptyList()
            return (0 until arr.length()).mapNotNull { arr.optJSONObject(it)?.let(Server::fromJson) }
        }
        // The first version kept a single server.
        val host = prefs.getString("host", null) ?: return emptyList()
        val token = prefs.getString("token", null) ?: return emptyList()
        val server = Server(newId(), host, host, prefs.getInt("port", 47811), token)
        prefs.edit().remove("host").remove("token").remove("port")
            .putString("root:${server.id}", prefs.getString("root", null)).remove("root").apply()
        saveServers(listOf(server))
        return listOf(server)
    }

    private fun saveServers(servers: List<Server>) {
        prefs.edit().putString("servers", JSONArray(servers.map { it.toJson() }).toString()).apply()
    }

    private fun newId() = java.util.UUID.randomUUID().toString().take(8)

    /** Add a server, or update the one at the same address, and switch to it. */
    fun addServer(endpoint: Endpoint, name: String?) {
        val servers = _ui.value.servers
        val existing = servers.firstOrNull { it.host == endpoint.host && it.port == endpoint.port }
        val server = Server(
            existing?.id ?: newId(),
            name?.takeIf { it.isNotBlank() } ?: existing?.name ?: endpoint.host,
            endpoint.host, endpoint.port, endpoint.token,
        )
        val updated = if (existing != null) servers.map { if (it.id == server.id) server else it } else servers + server
        saveServers(updated)
        _ui.update { it.copy(servers = updated, adding = false) }
        switchServer(server.id, force = true)
    }

    /** `oxi://pair?host=..&port=..&token=..&name=..`, from a QR code, a link or a paste. */
    fun pairFromText(text: String): Boolean {
        val uri = runCatching { Uri.parse(text.trim()) }.getOrNull() ?: return false
        return pairFromUri(uri)
    }

    fun pairFromUri(uri: Uri): Boolean {
        if (uri.scheme != "oxi" || uri.host != "pair") return false
        val host = uri.getQueryParameter("host") ?: return false
        val token = uri.getQueryParameter("token") ?: return false
        val port = uri.getQueryParameter("port")?.toIntOrNull() ?: 47811
        addServer(Endpoint(host, port, token), uri.getQueryParameter("name"))
        return true
    }

    fun switchServer(id: String, force: Boolean = false) {
        val server = _ui.value.servers.firstOrNull { it.id == id } ?: return
        if (!force && _ui.value.activeId == id) return
        prefs.edit().putString("active", id).apply()
        _ui.update {
            UiState(
                update = it.update,
                servers = it.servers,
                activeId = id,
                root = prefs.getString("root:$id", null),
                toast = it.toast,
            )
        }
        client.stop()
        client.start(server.endpoint)
    }

    fun renameServer(id: String, name: String) {
        val updated = _ui.value.servers.map { if (it.id == id) it.copy(name = name.trim().ifBlank { it.name }) else it }
        saveServers(updated)
        _ui.update { it.copy(servers = updated) }
    }

    fun removeServer(id: String) {
        val updated = _ui.value.servers.filterNot { it.id == id }
        saveServers(updated)
        prefs.edit().remove("root:$id").apply()
        if (_ui.value.activeId == id) {
            client.stop()
            _ui.update { UiState(update = it.update, servers = updated) }
            updated.firstOrNull()?.let { switchServer(it.id) } ?: prefs.edit().remove("active").apply()
        } else {
            _ui.update { it.copy(servers = updated) }
        }
    }

    fun showAddServer(show: Boolean) = _ui.update { it.copy(adding = show) }

    fun onResume() {
        client.kick()
        checkForUpdate()
    }

    // ---- self-update -----------------------------------------------------------------------

    private var lastUpdateCheck = 0L

    /** Look for a newer release at most every few hours and download it in the background. */
    fun checkForUpdate(force: Boolean = false) {
        val now = System.currentTimeMillis()
        if (!force && now - lastUpdateCheck < 4 * 3600_000L) return
        if (_ui.value.update is UpdateState.Downloading || _ui.value.update is UpdateState.Installing) return
        lastUpdateCheck = now
        viewModelScope.launch {
            val release = runCatching { Updater.check() }.getOrNull() ?: return@launch
            val context = getApplication<Application>()
            _ui.update { it.copy(update = UpdateState.Downloading(release.version, 0f)) }
            runCatching {
                Updater.download(context, release.version) { p ->
                    _ui.update { it.copy(update = UpdateState.Downloading(release.version, p)) }
                }
            }.onSuccess { apk -> _ui.update { it.copy(update = UpdateState.Ready(release.version, apk)) } }
                .onFailure { e -> _ui.update { it.copy(update = UpdateState.Failed(release.version, e.message ?: "download failed")) } }
        }
    }

    /** Install the downloaded update. Returns false when Android first needs the user's OK. */
    fun installUpdate(): Boolean {
        val ready = _ui.value.update as? UpdateState.Ready ?: return true
        val context = getApplication<Application>()
        if (!Updater.canInstall(context)) return false
        _ui.update { it.copy(update = UpdateState.Installing(ready.version)) }
        runCatching { Updater.install(context, ready.apk) }
            .onFailure { e -> _ui.update { it.copy(update = UpdateState.Failed(ready.version, e.message ?: "install failed")) } }
        return true
    }

    fun updateFailed(message: String) {
        val version = when (val u = _ui.value.update) {
            is UpdateState.Installing -> u.version
            is UpdateState.Ready -> u.version
            else -> return toast(message)
        }
        _ui.update { it.copy(update = UpdateState.Failed(version, message)) }
    }

    /** Back to "ready" after the user dismissed Android's confirmation. */
    fun updateDismissed() {
        val u = _ui.value.update as? UpdateState.Installing ?: return
        val context = getApplication<Application>()
        val apk = java.io.File(context.cacheDir, "update/oxi-${u.version}.apk")
        if (apk.exists()) _ui.update { it.copy(update = UpdateState.Ready(u.version, apk)) }
    }

    /** Reconnect as soon as the default network changes (Wi-Fi ↔ mobile data, Tailscale up). */
    private fun watchNetwork(app: Application) {
        val cm = app.getSystemService(ConnectivityManager::class.java) ?: return
        var current: Network? = null
        val callback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                val previous = current
                current = network
                if (previous != null && previous != network) client.reconnect()
            }

            override fun onLost(network: Network) {
                if (network == current) client.reconnect()
            }
        }
        cm.registerDefaultNetworkCallback(callback)
        addCloseable { cm.unregisterNetworkCallback(callback) }
    }

    private suspend fun onConnected() {
        runCatching {
            val init = client.request("initialize") as JSONObject
            val info = client.request("server.info") as? JSONObject
            _ui.update {
                it.copy(
                    providers = parseProviders(init),
                    host = info?.optString("host").orEmpty(),
                    home = info?.optString("home"),
                )
            }
            val hostName = info?.optString("host").orEmpty()
            _ui.value.active?.let { server ->
                if (hostName.isNotBlank() && server.name == server.host) renameServer(server.id, hostName)
            }
        }.onFailure { toast(it.message) }
        refreshWorkspaces()
        _ui.value.root?.let { refreshChats(it) }
        _ui.value.chat?.let { chat -> if (chat.id != null) reopen(chat.root, chat.id) }
    }

    // ---- workspaces and chats --------------------------------------------------------------

    fun refreshWorkspaces() = viewModelScope.launch {
        runCatching { client.request("workspaces.list") as JSONArray }
            .onSuccess { arr -> _ui.update { it.copy(workspaces = parseWorkspaces(arr), workspacesLoaded = true) } }
            .onFailure { toast(it.message) }
    }

    fun openWorkspace(path: String?) {
        _ui.value.activeId?.let { prefs.edit().putString("root:$it", path).apply() }
        _ui.update { it.copy(root = path, chats = emptyList(), chat = null) }
        if (path != null) refreshChats(path)
    }

    fun refreshChats(root: String) = viewModelScope.launch {
        _ui.update { it.copy(chatsLoading = true) }
        runCatching { client.request("chats.list", JSONObject().put("root", root)) as JSONArray }
            .onSuccess { arr ->
                _ui.update { if (it.root == root) it.copy(chats = parseChats(arr), chatsLoading = false) else it }
            }
            .onFailure { e -> _ui.update { it.copy(chatsLoading = false) }; toast(e.message) }
    }

    fun listFolder(path: String, done: (String, String?, List<String>) -> Unit) = viewModelScope.launch {
        runCatching { client.request("fs.list", JSONObject().put("path", path)) as JSONObject }
            .onSuccess { obj ->
                val entries = obj.optJSONArray("entries")
                val dirs = (0 until (entries?.length() ?: 0)).mapNotNull { i ->
                    entries!!.optJSONObject(i)?.takeIf { it.optBoolean("dir") }?.optString("name")
                }
                done(obj.optString("path"), obj.optString("parent").takeIf { it.isNotEmpty() && !obj.isNull("parent") }, dirs)
            }
            .onFailure { toast(it.message) }
    }

    fun newChat() {
        val root = _ui.value.root ?: return
        val last = _ui.value.chat?.config
        _ui.update { it.copy(chat = OpenChat(root, null, "New chat", last)) }
        // The daemon's default for a new chat in this folder.
        viewModelScope.launch {
            runCatching { client.request("chat.set_config", JSONObject().put("root", root)) as JSONObject }
                .onSuccess { cfg ->
                    _ui.update { s -> s.copy(chat = s.chat?.takeIf { it.id == null }?.copy(config = parseConfig(cfg)) ?: s.chat) }
                }
        }
    }

    fun openChat(id: String) {
        val root = _ui.value.root ?: return
        val title = _ui.value.chats.firstOrNull { it.id == id }?.title ?: ""
        _ui.update { it.copy(chat = OpenChat(root, id, title, null, loading = true)) }
        viewModelScope.launch { reopen(root, id) }
    }

    private suspend fun reopen(root: String, id: String) {
        runCatching { client.request("chat.open", JSONObject().put("root", root).put("chat", id)) as JSONObject }
            .onSuccess { obj ->
                _ui.update { s ->
                    if (s.chat?.id != id) return@update s
                    s.copy(
                        chat = s.chat.copy(
                            title = obj.optString("title").ifBlank { s.chat.title },
                            config = parseConfig(obj.optJSONObject("config")),
                            messages = parseMessages(obj.optJSONArray("messages")),
                            running = obj.optBoolean("running"),
                            queued = obj.optInt("queued"),
                            approval = parseApproval(obj.optJSONObject("approval")),
                            status = obj.optString("status").takeIf { !obj.isNull("status") && it.isNotEmpty() },
                            loading = false,
                        )
                    )
                }
            }
            .onFailure { e -> _ui.update { s -> s.copy(chat = s.chat?.copy(loading = false, error = e.message)) } }
    }

    fun closeChat() {
        _ui.update { it.copy(chat = null) }
        _ui.value.root?.let { refreshChats(it) }
    }

    fun deleteChat(id: String) = viewModelScope.launch {
        val root = _ui.value.root ?: return@launch
        runCatching { client.request("chat.delete", JSONObject().put("root", root).put("chat", id)) }
            .onFailure { toast(it.message) }
    }

    fun renameChat(title: String) = viewModelScope.launch {
        val chat = _ui.value.chat ?: return@launch
        val id = chat.id ?: return@launch
        runCatching { client.request("chat.rename", JSONObject().put("root", chat.root).put("chat", id).put("title", title)) }
            .onFailure { toast(it.message) }
    }

    // ---- turns -----------------------------------------------------------------------------

    fun send(text: String) {
        val chat = _ui.value.chat ?: return
        val trimmed = text.trim()
        if (trimmed.isEmpty()) return
        val params = JSONObject().put("root", chat.root).put("text", trimmed)
        if (chat.id != null) params.put("chat", chat.id)
        else chat.config?.let { params.put("provider", it.provider).put("model", it.model).put("effort", it.effort) }
        _ui.update { s -> s.copy(chat = s.chat?.copy(sending = true, error = null)) }
        viewModelScope.launch {
            runCatching { client.request("prompt.send", params) as JSONObject }
                .onSuccess { r ->
                    val id = r.optString("chat")
                    val queued = r.optInt("queued")
                    _ui.update { s ->
                        val c = s.chat ?: return@update s
                        if (c.id != null && c.id != id) return@update s
                        val messages = if (queued == 0) c.messages + Message.User(trimmed, emptyList()) else c.messages
                        s.copy(
                            chat = c.copy(
                                id = id,
                                title = r.optString("title").ifBlank { c.title },
                                messages = messages,
                                running = true,
                                queued = queued,
                                sending = false,
                            )
                        )
                    }
                    if (chat.id == null) reopen(chat.root, id)
                }
                .onFailure { e -> _ui.update { s -> s.copy(chat = s.chat?.copy(sending = false, error = e.message)) } }
        }
    }

    fun cancel() = viewModelScope.launch {
        val id = _ui.value.chat?.id ?: return@launch
        runCatching { client.request("cancel", JSONObject().put("chat", id)) }.onFailure { toast(it.message) }
    }

    fun respond(decision: String) = viewModelScope.launch {
        val chat = _ui.value.chat ?: return@launch
        val id = chat.id ?: return@launch
        val params = JSONObject().put("chat", id).put("decision", decision)
        if (decision == "allow_prefix") params.put("prefix", chat.approval?.allowPrefix ?: return@launch)
        runCatching { client.request("approval.respond", params) }.onFailure { toast(it.message) }
    }

    fun setProvider(provider: String, model: String? = null) = viewModelScope.launch {
        val chat = _ui.value.chat ?: return@launch
        val params = JSONObject().put("root", chat.root).put("provider", provider)
        if (model != null) params.put("model", model)
        if (chat.id != null) params.put("chat", chat.id)
        runCatching { client.request("chat.set_config", params) as JSONObject }
            .onSuccess { cfg -> _ui.update { s -> s.copy(chat = s.chat?.copy(config = parseConfig(cfg))) } }
            .onFailure { toast(it.message) }
    }

    fun loadModels(provider: String) = viewModelScope.launch {
        val root = _ui.value.chat?.root ?: _ui.value.root ?: return@launch
        runCatching { client.request("models.list", JSONObject().put("root", root).put("provider", provider)) as JSONObject }
            .onSuccess { obj -> _ui.update { it.copy(models = it.models + (provider to parseModels(obj))) } }
            .onFailure { toast(it.message) }
    }

    fun loadQuota(provider: String) = viewModelScope.launch {
        runCatching { client.request("quota.get", JSONObject().put("provider", provider)) as JSONObject }
            .onSuccess { obj -> _ui.update { it.copy(quota = it.quota + (provider to parseQuota(obj))) } }
    }

    // ---- notifications ---------------------------------------------------------------------

    private fun onNote(note: Note) {
        val p = note.params
        val chatId = p.optString("chat")
        when (note.method) {
            "chats_changed" -> if (p.optString("root") == _ui.value.root) _ui.value.root?.let { refreshChats(it) }
            "chat_removed" -> {
                _ui.update { s -> s.copy(chats = s.chats.filterNot { it.id == chatId }, chat = s.chat?.takeIf { it.id != chatId }) }
            }
            "chat_reloaded" -> _ui.value.chat?.takeIf { it.id == chatId }?.let { c ->
                viewModelScope.launch { reopen(c.root, chatId) }
            }
            else -> updateOpenChat(chatId) { c -> applyChatNote(note.method, p, c) }
        }
    }

    private fun applyChatNote(method: String, p: JSONObject, c: OpenChat): OpenChat = when (method) {
        "message" -> {
            val index = p.optInt("index")
            val message = parseMessage(p.getJSONObject("message"))
            when {
                index < c.messages.size -> c.copy(messages = c.messages.toMutableList().also { it[index] = message })
                index == c.messages.size -> c.copy(messages = c.messages + message)
                else -> {
                    // Prompts we didn't see (another client, a queued one): read the chat again.
                    c.id?.let { id -> viewModelScope.launch { reopen(c.root, id) } }
                    c
                }
            }
        }
        "turn" -> c.copy(
            running = p.optBoolean("running"),
            queued = p.optInt("queued"),
            error = p.optString("error").takeIf { !p.isNull("error") && it.isNotEmpty() } ?: c.error.takeIf { p.optBoolean("running") },
        )
        "approval" -> c.copy(approval = parseApproval(p.optJSONObject("approval")))
        "status" -> c.copy(status = p.optString("text").takeIf { !p.isNull("text") && it.isNotEmpty() })
        "title" -> c.copy(title = p.optString("title"))
        "config" -> c.copy(config = parseConfig(p.optJSONObject("config")))
        else -> c
    }

    private fun updateOpenChat(id: String, change: (OpenChat) -> OpenChat) {
        if (id.isEmpty()) return
        _ui.update { s ->
            val c = s.chat
            if (c?.id == id) s.copy(chat = change(c)) else s
        }
    }

    fun toast(message: String?) {
        if (message != null) _ui.update { it.copy(toast = message) }
    }

    fun toastShown() = _ui.update { it.copy(toast = null) }
}
