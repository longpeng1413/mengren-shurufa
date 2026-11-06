package app.qingjian.inputmethod.clipboard.sync

import android.content.ClipboardManager
import android.content.Context
import android.widget.Toast
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.settings.Settings
import kotlin.concurrent.thread

/** 私密文字只交给内存中的独立通道；不调用 ClipboardStore，不保存或补发。 */
object PrivateSend {
    const val KEY_MODE = "clipboard_private_mode"
    val MODES = arrayOf("auto", "manual", "off")

    fun mode(context: Context) = Settings.preferences(context).getString(KEY_MODE, "auto").orEmpty()

    fun automatic(context: Context, text: String) {
        if (mode(context) == "auto") send(context, text)
    }

    fun manual(context: Context) {
        if (mode(context) == "off") { notice(context, R.string.private_disabled); return }
        val clip = runCatching { context.getSystemService(ClipboardManager::class.java)?.primaryClip }.getOrNull()
        val text = clip?.takeIf { it.itemCount > 0 }?.getItemAt(0)?.text?.toString()
        if (text.isNullOrEmpty()) { notice(context, R.string.private_empty); return }
        send(context, text)
    }

    private fun send(context: Context, text: String) {
        if (!ClipboardSync.isRunning || context.getSystemService(android.app.KeyguardManager::class.java)?.isDeviceLocked != false || context.getSystemService(android.os.PowerManager::class.java)?.isInteractive != true || text.length > 16384 || text.any { it.code < 32 || it.code == 127 }) {
            notice(context, R.string.private_failed); return
        }
        thread(name = "mengren-private-send") {
            val accepted = runCatching { Native.clipboardSyncPrivateSend(text) }.getOrDefault(false)
            if (!accepted) android.os.Handler(android.os.Looper.getMainLooper()).post { notice(context, R.string.private_failed) }
        }
    }

    private fun notice(context: Context, message: Int) = Toast.makeText(context, message, Toast.LENGTH_LONG).show()
}
