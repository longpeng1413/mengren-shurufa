package app.qingjian.inputmethod.clipboard

import app.qingjian.inputmethod.clipboard.sync.ClipboardSync
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import app.qingjian.inputmethod.core.store.ClipboardStore
import app.qingjian.inputmethod.settings.Settings
import kotlin.concurrent.thread

/**
 * 盯着系统剪贴板，复制了文字或图片就记进剪贴板历史。默认输入法读剪贴板不要权限（安卓 10 起只有它和前台应用能读）。
 * 标了敏感的（密码管理器复制的密码，安卓 13 的 `EXTRA_IS_SENSITIVE`）不记；设置里关了「记录剪贴板」也不记。
 * 图片读字节放后台线程，太大的（超过 [MAX_IMAGE_BYTES]）不记。
 * 同一次复制只记一次（按剪贴板内容的时间戳认）：键盘每次弹出都会读一遍，用户删掉的那条若还是系统剪贴板当前内容，不能又收回来。
 */
class ClipboardWatcher(private val context: Context) {
    private val manager = context.getSystemService(ClipboardManager::class.java)
    private val prefs = context.getSharedPreferences("clipboard", Context.MODE_PRIVATE)
    private val privateCopy = PrivateCopyState(prefs.getLong(LAST_PRIVATE, 0L))
    private val main = Handler(Looper.getMainLooper())
    private val listener = ClipboardManager.OnPrimaryClipChangedListener { capture() }

    fun start() {
        manager?.addPrimaryClipChangedListener(listener)
    }

    fun stop() {
        manager?.removePrimaryClipChangedListener(listener)
    }

    /** 读一次当前剪贴板记下来；键盘弹出时也调（监听在输入法没起来时收不到）。 */
    fun capture() {
        val settings = Settings.load(context)
        val clip = runCatching { manager?.primaryClip }.getOrNull() ?: return
        if (clip.itemCount == 0) return
        if (sensitive(clip.description)) {
            val timestamp = clip.description.timestamp
            if (privateCopy.shouldAttempt(timestamp, System.currentTimeMillis())) {
                app.qingjian.inputmethod.core.AppLog.info("private capture: sensitive_event")
                clip.getItemAt(0).text?.toString()?.let {
                    if (app.qingjian.inputmethod.clipboard.sync.PrivateSend.automatic(context, it)) {
                        privateCopy.accepted = timestamp
                        prefs.edit().putLong(LAST_PRIVATE, timestamp).apply()
                    }
                }
            }
            return
        }
        if (!settings.recordClipboard) return
        if (!firstSeen(clip.description.timestamp)) return
        val item = clip.getItemAt(0)
        val uri = item.uri
        if (uri?.authority == "${context.packageName}.clipboard") return
        val mime = uri?.let { runCatching { context.contentResolver.getType(it) }.getOrNull() }
        if (uri != null && mime?.startsWith("image/") == true) {
            thread(name = "qingjian-clipboard-image") {
                val directory = java.io.File(ClipboardStore.userDir(context), "clipboard-import").apply { mkdirs() }
                val stored = ClipboardImports.use(directory) { temporary ->
                    runCatching {
                        context.contentResolver.openInputStream(uri)?.use { input ->
                            temporary.outputStream().use { output ->
                                val buffer = ByteArray(64 * 1024)
                                var total = 0L
                                while (true) {
                                    val read = input.read(buffer)
                                    if (read < 0) break
                                    total += read
                                    require(total <= MAX_IMAGE_BYTES)
                                    output.write(buffer, 0, read)
                                }
                            }
                            app.qingjian.inputmethod.core.Native.clipboardAddImageFile(ClipboardStore.userDir(context), temporary.path, ClipboardImageProvider.extensionOf(mime), settings.clipboardLimit)
                        } == true
                    }.getOrDefault(false)
                }
                if (!stored) return@thread
                ClipboardSync.changed()
                main.post { onChanged?.invoke() }
            }
            return
        }
        val text = item.text?.toString() ?: return
        // 同步刚放上来的不再记：记了时刻变新，又传回去，两边来回传
        if (text != ClipboardSync.lastReceived) {
            ClipboardStore.add(context, text, settings.clipboardLimit)
            ClipboardSync.changed()
        }
        onChanged?.invoke()
    }

    /** 记进去了一条（剪贴板面板开着时重读）。 */
    var onChanged: (() -> Unit)? = null

    /** 时间戳记在磁盘上：输入法进程会被杀，重起后系统剪贴板里还是那一份。 */
    private fun firstSeen(timestamp: Long): Boolean {
        val prefs = context.getSharedPreferences("clipboard", Context.MODE_PRIVATE)
        if (prefs.getLong(LAST_SEEN, 0L) == timestamp) return false
        prefs.edit().putLong(LAST_SEEN, timestamp).apply()
        return true
    }

    private fun sensitive(description: ClipDescription): Boolean =
        description.extras?.getBoolean("android.content.extra.IS_SENSITIVE") == true

    private companion object {
        const val MAX_IMAGE_BYTES = 99_999_999
        const val LAST_PRIVATE = "last_private_timestamp"
        const val LAST_SEEN = "last_seen_timestamp"
    }
}
