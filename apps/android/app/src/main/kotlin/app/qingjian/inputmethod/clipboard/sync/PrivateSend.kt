package app.qingjian.inputmethod.clipboard.sync

import android.content.ClipboardManager
import android.content.Context
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.settings.Settings

/** 私密文字只交给内存中的独立通道；不调用 ClipboardStore，不保存或补发。 */
object PrivateSend {
    const val KEY_MODE = "clipboard_private_mode"
    val MODES = arrayOf("auto", "manual", "off")

    fun mode(context: Context) = Settings.preferences(context).getString(KEY_MODE, "auto").orEmpty()

    fun automatic(context: Context, text: String): Boolean =
        mode(context) == "auto" && send(context, text)

    fun manual(context: Context) {
        if (mode(context) == "off") { notice(context, R.string.private_disabled); return }
        val clip = runCatching { context.getSystemService(ClipboardManager::class.java)?.primaryClip }.getOrNull()
        val text = clip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString()
        if (text.isNullOrEmpty()) { notice(context, R.string.private_empty); return }
        send(context, text)
    }

    private fun send(context: Context, text: String): Boolean {
        ClipboardSync.apply(context)
        val reason = when {
            !ClipboardSync.isRunning -> "sync_inactive"
            context.getSystemService(android.app.KeyguardManager::class.java)?.isDeviceLocked != false || context.getSystemService(android.os.PowerManager::class.java)?.isInteractive != true -> "device_locked"
            text.isEmpty() || text.length > 16384 || text.any { it.code < 32 || it.code == 127 } -> "unsupported_content"
            else -> null
        }
        if (reason != null) {
            app.qingjian.inputmethod.core.AppLog.info("private capture: $reason")
            notice(context, R.string.private_failed); return false
        }
        // JNI 只启动后台发送，成功受理才允许源端标记此复制已处理。
        val accepted = runCatching { Native.clipboardSyncPrivateSend(text) }.getOrDefault(false)
        if (!accepted) notice(context, R.string.private_failed)
        app.qingjian.inputmethod.core.AppLog.info("private capture: task_${if (accepted) "accepted" else "rejected"}")
        return accepted
    }

    private fun notice(context: Context, message: Int) = PrivateNotice.show(context, context.getString(message))
}
