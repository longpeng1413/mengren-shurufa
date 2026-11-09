package app.qingjian.inputmethod.voice

import android.content.Context
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.update.Updates
import org.json.JSONObject
import kotlin.concurrent.thread

/**
 * 本机离线识别模型（约 229 MB）的下载、删除与启动清理；下载、校验、续传都在 Rust 侧，这里只开后台线程、转述状态。
 * 下载跟着进程走：离开设置页照样下完，进程被杀则留下半截文件，下次从断点续传（超过 7 天没续上的启动时清掉）。
 */
object VoiceModel {
    class Status(
        val installed: Boolean,
        /** 占用的磁盘空间（含半截下载）。 */
        val bytes: Long,
        /** 要下载的总字节数。 */
        val total: Long,
        val downloading: Boolean,
        val received: Long,
    )

    /** 上一次下载的出错原因（给用户看的话）；成功或重新开始下载时清掉。 */
    @Volatile
    var lastError: String? = null
        private set

    /** 下载线程开着（从点下载到线程结束，含 Rust 侧还没真正开始的那一小会儿）。 */
    @Volatile
    private var busy = false

    fun status(context: Context): Status {
        val json = runCatching { JSONObject(Native.voiceModelStatus(VoiceConfig.modelRoot(context)) ?: "{}") }
            .getOrDefault(JSONObject())
        return Status(
            json.optBoolean("installed"),
            json.optLong("bytes"),
            json.optLong("total"),
            busy || json.optBoolean("downloading"),
            json.optLong("received"),
        )
    }

    /** 后台下载；[done] 在后台线程回调，参数是出错原因（成功为 null）。代理与检查更新同一个。已在下载时什么都不做。 */
    @Synchronized
    fun download(context: Context, done: (String?) -> Unit = {}) {
        if (busy) return
        busy = true
        val app = context.applicationContext
        lastError = null
        thread(name = "mengren-voice-model") {
            val result = runCatching {
                JSONObject(Native.voiceModelDownload(VoiceConfig.modelRoot(app), Updates.proxy(app)) ?: "{}")
            }.getOrNull()
            val error = when {
                result == null -> "下载时出错了"
                result.optBoolean("ok") -> null
                else -> result.optString("error").ifEmpty { "下载失败" }
            }
            lastError = error
            busy = false
            done(error)
        }
    }

    fun cancel() = Native.voiceModelCancel()

    /** 删除模型，回出错原因（成功为 null）。 */
    fun delete(context: Context): String? {
        val result = runCatching { JSONObject(Native.voiceModelDelete(VoiceConfig.modelRoot(context)) ?: "{}") }.getOrNull()
        lastError = null
        return when {
            result == null -> "删除时出错了"
            result.optBoolean("ok") -> null
            else -> result.optString("error").ifEmpty { "删除失败" }
        }
    }

    /** App 启动时在后台清理旧版本模型与过期的半截下载。 */
    fun cleanup(context: Context) {
        val root = VoiceConfig.modelRoot(context)
        thread(name = "mengren-voice-cleanup", isDaemon = true) { runCatching { Native.voiceModelCleanup(root) } }
    }
}
