package app.qingjian.inputmethod.clipboard.sync

import android.app.KeyguardManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import android.os.PowerManager
import android.view.inputmethod.InputConnection
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.core.AppLog
import app.qingjian.inputmethod.settings.Settings

/** 私密内容留在 Rust 内存槽，仅用户点击后直接上屏；灭屏和锁屏禁止接收。 */
object PrivateReceive {
    const val KEY_ENABLED = "clipboard_private_receive"
    const val KEY_AUTO_FILL = "clipboard_private_auto_fill"
    private var app: Context? = null
    fun pending() = runCatching { Native.clipboardSyncPrivatePending() }.getOrDefault(false)

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == Intent.ACTION_SCREEN_OFF) {
                runCatching { Native.clipboardSyncPrivateState(enabled(context), false) }
            } else update(context)
        }
    }

    fun enabled(context: Context) = Settings.preferences(context).getBoolean(KEY_ENABLED, true)
    fun autoFillEnabled(context: Context) = Settings.preferences(context).getBoolean(KEY_AUTO_FILL, false)

    private fun unlocked(context: Context) = context.getSystemService(PowerManager::class.java)?.isInteractive == true &&
        context.getSystemService(KeyguardManager::class.java)?.isDeviceLocked == false

    fun start(context: Context) {
        stop()
        val application = context.applicationContext
        val filter = IntentFilter().apply {
            addAction(Intent.ACTION_SCREEN_OFF); addAction(Intent.ACTION_SCREEN_ON); addAction(Intent.ACTION_USER_PRESENT)
        }
        val registered = runCatching {
            if (Build.VERSION.SDK_INT >= 33) application.registerReceiver(receiver, filter, Context.RECEIVER_NOT_EXPORTED)
            else application.registerReceiver(receiver, filter)
        }.isSuccess
        if (!registered) { runCatching { Native.clipboardSyncPrivateState(false, false) }; return }
        app = application
        update(application)
    }

    fun stop() {
        app?.let { runCatching { it.unregisterReceiver(receiver) } }
        app = null
        runCatching { Native.clipboardSyncPrivateState(false, false) }
    }

    fun update(context: Context) {
        runCatching { Native.clipboardSyncPrivateState(enabled(context) && app != null, unlocked(context)) }
    }

    fun input(context: Context, connection: InputConnection?, passwordField: Boolean = false) {
        update(context)
        if (connection == null || app == null || !enabled(context) || !unlocked(context)) { notice(context, R.string.private_input_unavailable); return }
        val text = runCatching { Native.clipboardSyncPrivateTake() }.getOrNull()
        if (text == null) { notice(context, R.string.private_input_empty); return }
        fill(context, connection, text, passwordField)
    }

    private fun fill(context: Context, connection: InputConnection, text: String, replace: Boolean) {
        val committed = unlocked(context) && runCatching {
            if (replace) {
                connection.beginBatchEdit()
                try { connection.performContextMenuAction(android.R.id.selectAll); connection.commitText(text, 1) }
                finally { connection.endBatchEdit() }
            } else connection.commitText(text, 1)
        }.getOrDefault(false)
        AppLog.info("private input: committed=$committed")
        if (!committed) {
            notice(context, R.string.private_input_failed)
        }
    }

    fun onReceived(context: Context, connection: InputConnection?, passwordField: Boolean) {
        if (app == null || connection == null || !passwordField || !autoFillEnabled(context) || !enabled(context) || !unlocked(context)) return
        val text = runCatching { Native.clipboardSyncPrivateTake() }.getOrNull() ?: return
        fill(context, connection, text, true)
    }

    private fun notice(context: Context, message: Int) = PrivateNotice.show(context, context.getString(message))
}
