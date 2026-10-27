package dev.oxi.android

import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.withContext
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL
import java.security.MessageDigest

/**
 * Updates from GitHub releases, like the desktop app (src/update.rs): the newest release's
 * `oxi-android.apk`, checked against its `SHA256SUMS`, installed through PackageInstaller.
 * Asset names match `.github/workflows/release.yml`.
 */
object Updater {
    private val FEED = BuildConfig.UPDATE_FEED
    private val LATEST =
        if (FEED.startsWith("https://github.com/")) {
            FEED.replace("https://github.com/", "https://api.github.com/repos/") + "/latest"
        } else {
            "$FEED/latest"
        }
    private val DOWNLOAD = "$FEED/download"
    const val ASSET = "oxi-android.apk"

    data class Release(val version: String, val notes: String)

    /** What the installer said, for the activity: a confirmation to show, or a failure. */
    sealed interface InstallEvent {
        data class Confirm(val intent: Intent) : InstallEvent
        data class Failed(val message: String) : InstallEvent
    }

    private val _events = MutableSharedFlow<InstallEvent>(extraBufferCapacity = 4)
    val events: SharedFlow<InstallEvent> = _events

    fun parseVersion(s: String): Triple<Int, Int, Int>? {
        val parts = s.trim().removePrefix("v").substringBefore('-').substringBefore('+').split('.')
        if (parts.size != 3) return null
        val n = parts.map { it.toIntOrNull() ?: return null }
        return Triple(n[0], n[1], n[2])
    }

    fun isNewer(latest: String, current: String): Boolean {
        val a = parseVersion(latest) ?: return false
        val b = parseVersion(current) ?: return true
        return compareValuesBy(a, b, { it.first }, { it.second }, { it.third }) > 0
    }

    /** The newest release when it is newer than this app and ships an APK. */
    suspend fun check(current: String = BuildConfig.VERSION_NAME): Release? = withContext(Dispatchers.IO) {
        val json = JSONObject(String(get(LATEST), Charsets.UTF_8))
        val version = json.optString("tag_name").removePrefix("v")
        val assets = json.optJSONArray("assets")
        val hasApk = (0 until (assets?.length() ?: 0)).any { assets!!.optJSONObject(it)?.optString("name") == ASSET }
        if (hasApk && isNewer(version, current)) Release(version, json.optString("body")) else null
    }

    /** Download the release's APK into the cache, verified; reuses an earlier download. */
    suspend fun download(context: Context, version: String, progress: (Float) -> Unit): File = withContext(Dispatchers.IO) {
        val dir = File(context.cacheDir, "update").apply { mkdirs() }
        val apk = File(dir, "oxi-$version.apk")
        val sums = String(get("$DOWNLOAD/v$version/SHA256SUMS"), Charsets.UTF_8)
        val expected = sums.lineSequence()
            .map { it.trim().split(Regex("\\s+"), limit = 2) }
            .firstOrNull { it.size == 2 && it[1].trimStart('*') == ASSET }
            ?.get(0)?.lowercase()
            ?: error("$ASSET is not in SHA256SUMS")
        if (apk.exists() && sha256(apk) == expected) return@withContext apk
        dir.listFiles()?.forEach { it.delete() }
        val part = File(dir, "download.part")
        val conn = open("$DOWNLOAD/v$version/$ASSET")
        val total = conn.contentLengthLong
        conn.inputStream.use { input ->
            part.outputStream().use { output ->
                val buf = ByteArray(64 * 1024)
                var done = 0L
                while (true) {
                    val n = input.read(buf)
                    if (n < 0) break
                    output.write(buf, 0, n)
                    done += n
                    if (total > 0) progress(done.toFloat() / total)
                }
            }
        }
        if (sha256(part) != expected) {
            part.delete()
            error("Checksum mismatch for $ASSET")
        }
        part.renameTo(apk)
        apk
    }

    /** Whether the user let oxi install apps (asked once, in system settings). */
    fun canInstall(context: Context) = context.packageManager.canRequestPackageInstalls()

    /**
     * Hand the APK to PackageInstaller. Android asks the user to confirm the first time; once
     * oxi installed itself, later updates go through without asking (Android 12+).
     */
    fun install(context: Context, apk: File) {
        val installer = context.packageManager.packageInstaller
        val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
            setAppPackageName(context.packageName)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_NOT_REQUIRED)
            }
        }
        val id = installer.createSession(params)
        installer.openSession(id).use { session ->
            apk.inputStream().use { input ->
                session.openWrite("oxi.apk", 0, apk.length()).use { out ->
                    input.copyTo(out)
                    session.fsync(out)
                }
            }
            val intent = Intent(context, InstallReceiver::class.java)
            val pending = PendingIntent.getBroadcast(
                context, id, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
            )
            session.commit(pending.intentSender)
        }
    }

    internal fun emit(event: InstallEvent) {
        _events.tryEmit(event)
    }

    private fun open(url: String): HttpURLConnection {
        val conn = URL(url).openConnection() as HttpURLConnection
        conn.connectTimeout = 15_000
        conn.readTimeout = 30_000
        conn.setRequestProperty("User-Agent", "oxi-android/${BuildConfig.VERSION_NAME}")
        conn.setRequestProperty("Accept", "application/vnd.github+json")
        if (conn.responseCode !in 200..299) error("HTTP ${conn.responseCode} for $url")
        return conn
    }

    private fun get(url: String): ByteArray = open(url).inputStream.use { it.readBytes() }

    private fun sha256(file: File): String {
        val digest = MessageDigest.getInstance("SHA-256")
        file.inputStream().use { input ->
            val buf = ByteArray(64 * 1024)
            while (true) {
                val n = input.read(buf)
                if (n < 0) break
                digest.update(buf, 0, n)
            }
        }
        return digest.digest().joinToString("") { "%02x".format(it) }
    }
}

/** PackageInstaller's answer for an update session. */
class InstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (val status = intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                @Suppress("DEPRECATION")
                val confirm = intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) ?: return
                Updater.emit(Updater.InstallEvent.Confirm(confirm.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)))
            }
            PackageInstaller.STATUS_SUCCESS -> {}
            else -> {
                val message = intent.getStringExtra(PackageInstaller.EXTRA_STATUS_MESSAGE) ?: "status $status"
                Updater.emit(Updater.InstallEvent.Failed("Update failed: $message"))
            }
        }
    }
}
