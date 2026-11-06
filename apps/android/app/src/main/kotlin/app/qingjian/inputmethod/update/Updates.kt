package app.qingjian.inputmethod.update

import android.content.Context
import android.net.ConnectivityManager
import android.os.Handler
import android.os.Looper
import app.qingjian.inputmethod.BuildConfig
import app.qingjian.inputmethod.core.Native
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

/** 检查和下载都在后台；只保存版本状态，不接触键盘输入或账号凭据。 */
object Updates {
    private val executor = Executors.newSingleThreadScheduledExecutor { task -> Thread(task, "mengren-update").apply { isDaemon = true } }
    private val main = Handler(Looper.getMainLooper())
    private val busy = AtomicBoolean(false)
    private var started = false
    fun prefs(context: Context) = context.getSharedPreferences("mengren-updates", Context.MODE_PRIVATE)
    fun cache(context: Context) = File(context.cacheDir, "updates")
    fun pending(context: Context): File? {
        val store = prefs(context)
        val offer = offer(context) ?: return null
        val hash = offer.optJSONObject("asset")?.optString("sha256").orEmpty().lowercase()
        return store.getString("package", null)?.let(::File)?.takeIf {
            it.isFile && it.parentFile?.canonicalPath == cache(context).canonicalPath &&
                matchesUpdatePackage(it.name, hash, store.getString("package_version", "").orEmpty(), offer.optString("version"))
        }
    }
    fun offer(context: Context): JSONObject? = runCatching { JSONObject(prefs(context).getString("offer", null) ?: return null) }.getOrNull()
    fun status(context: Context, value: String) { prefs(context).edit().putString("status", value).apply() }
    fun fail(context: Context, code: String) = status(context, "error_$code")
    fun proxy(context: Context): String {
        val explicit = prefs(context).getString("proxy", "").orEmpty().trim()
        if (explicit.isNotEmpty()) return explicit
        val system = context.getSystemService(ConnectivityManager::class.java).defaultProxy
        return system?.takeIf { !it.host.isNullOrBlank() && it.port > 0 }?.let { "http://${it.host}:${it.port}" }.orEmpty()
    }
    @Synchronized fun start(context: Context) {
        if (started) return
        started = true
        val app = context.applicationContext
        if (offer(app)?.optString("version") == BuildConfig.VERSION_NAME) {
            pending(app)?.delete()
            prefs(app).edit().remove("offer").remove("package").putBoolean("install_requested",false).apply()
        }
        // 被系统结束后不伪装成仍在下载；部分文件在下一次下载时重建。
        if (prefs(app).getString("status", "") in listOf("checking", "downloading", "installing")) status(app, "idle")
        executor.scheduleWithFixedDelay({
            val store = prefs(app)
            val now = System.currentTimeMillis()
            if (store.getBoolean("auto", true) && (now - store.getLong("checked", 0) >= TimeUnit.HOURS.toMillis(24) || now < store.getLong("checked", 0))) check(app)
        }, 2, 3600, TimeUnit.SECONDS)
    }
    fun check(context: Context) {
        if (!busy.compareAndSet(false, true)) return
        val app = context.applicationContext
        status(app, "checking")
        executor.execute {
            try {
                val result = JSONObject(Native.updateCheck(BuildConfig.VERSION_NAME, proxy(app)) ?: "{}")
                if (!result.has("available") && !result.has("error")) fail(app, "network")
                else if (result.has("error")) fail(app, result.optString("error"))
                else {
                    val offer = result.optJSONObject("available")
                    prefs(app).edit().putLong("checked", System.currentTimeMillis()).putString("offer", offer?.toString())
                        .putString("status", if (offer == null) "latest" else "available").apply()
                }
            } catch (_: Exception) { fail(app, "network") }
            finally { busy.set(false) }
        }
    }
    fun download(context: Context, version: String) {
        if (!busy.compareAndSet(false, true)) return
        val app = context.applicationContext
        prefs(app).edit().putString("status", "downloading").putInt("percent", 0).putBoolean("install_requested", true).apply()
        val poll = object : Runnable {
            override fun run() {
                if (prefs(app).getString("status", "") != "downloading") return
                if (!prefs(app).getBoolean("install_requested", false)) Native.updateCancel()
                prefs(app).edit().putInt("percent", Native.updateProgress().toInt()).apply()
                main.postDelayed(this, 700)
            }
        }
        main.post(poll)
        executor.execute {
            try {
                val result = JSONObject(Native.updateDownload(BuildConfig.VERSION_NAME, version, proxy(app), cache(app).absolutePath) ?: "{}")
                val path = result.optString("path")
                if (path.isNotEmpty()) prefs(app).edit().putString("package", path).putString("package_version", version).putString("status", "ready").apply()
                else { prefs(app).edit().putBoolean("install_requested", false).apply(); fail(app, result.optString("error", "network")) }
            } catch (_: Exception) { fail(app, "network") }
            finally { busy.set(false); main.removeCallbacks(poll) }
        }
    }
    fun cancel(context: Context) {
        prefs(context).edit().putBoolean("install_requested", false).apply()
        Native.updateCancel()
    }
}
