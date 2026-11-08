package app.qingjian.inputmethod.clipboard

import android.content.ClipboardManager
import android.content.Context
import android.content.ClipDescription
import android.os.Build
import android.os.PowerManager
import android.app.KeyguardManager
import android.view.inputmethod.InputConnection
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.core.AppLog
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.clipboard.sync.PrivateNotice
import java.security.MessageDigest

/** 从当前系统剪贴板提供点击填入；不读短信、不写历史、不另外发送到设备。 */
class ClipboardSuggestions(private val context: Context) {
    private val manager = context.getSystemService(ClipboardManager::class.java)
    private val state = ClipboardSuggestionState()
    private var signature: Triple<Long, Boolean, Int>? = null

    fun label(): String? {
        if (!unlocked()) { hide(); return null }
        val now = System.currentTimeMillis()
        val description = runCatching { manager?.primaryClipDescription }.getOrNull()
        if (description == null || !ClipboardSuggestionState.fresh(description.timestamp, now)) {
            hide(); return null
        }
        val status = if (Build.VERSION.SDK_INT >= 31) description.classificationStatus else 0
        val next = Triple(description.timestamp, sensitive(description), status)
        if (next != signature) {
            signature = next
            state.offer(read(now), now)
        }
        val item = state.current(now) ?: return null
        return when {
            item.sensitive && item.code -> context.getString(R.string.clipboard_code_masked)
            item.sensitive -> context.getString(R.string.clipboard_sensitive_fill)
            else -> context.getString(R.string.clipboard_code_fill, item.text)
        }
    }

    private fun read(now: Long): ClipboardSuggestionState.Item? {
        val clip = runCatching { manager?.primaryClip }.getOrNull() ?: return null
        if (clip.itemCount != 1 || !ClipboardSuggestionState.fresh(clip.description.timestamp, now)) return null
        val source = clip.getItemAt(0).text ?: return null
        if (source.isEmpty() || source.length > 2048) return null
        val text = source.toString()
        val masked = sensitive(clip.description)
        val classified = Build.VERSION.SDK_INT >= 36 &&
            runCatching { clip.description.getConfidenceScore(android.view.textclassifier.TextClassifier.TYPE_OTP) >= 0.5f }.getOrDefault(false)
        val code = runCatching { Native.clipboardVerificationCode(text, clip.description.label?.toString().orEmpty(), classified) }
            .getOrNull()?.takeIf { !masked || it == text || classified }
        if (code == null && !masked) return null
        val digest = MessageDigest.getInstance("SHA-256").digest(text.toByteArray()).joinToString("") { "%02x".format(it) }
        return ClipboardSuggestionState.Item("${clip.description.timestamp}:$digest", clip.description.timestamp,
            code ?: text, masked, code != null)
    }

    fun input(connection: InputConnection?) {
        if (!unlocked()) { hide(); notice(R.string.private_input_unavailable); return }
        val now = System.currentTimeMillis()
        val expected = state.current(now) ?: return
        val current = read(now)
        if (current?.token != expected.token || current.text != expected.text || current.sensitive != expected.sensitive) {
            hide()
            notice(R.string.clipboard_suggestion_changed)
            return
        }
        if (connection == null) { notice(R.string.private_input_unavailable); return }
        val committed = runCatching { connection.commitText(current.text, 1) }.getOrDefault(false)
        AppLog.info("clipboard suggestion input: committed=$committed, sensitive=${current.sensitive}, code=${current.code}")
        state.dismiss()
        if (!committed) notice(R.string.private_input_failed)
    }

    fun dismiss() = state.dismiss()
    fun hide() { state.hide(); signature = null }
    fun blocked() { AppLog.info("clipboard suggestion blocked: obscured_touch"); notice(R.string.clipboard_suggestion_blocked) }
    private fun notice(message: Int) = PrivateNotice.show(context, context.getString(message))
    private fun sensitive(description: ClipDescription) = description.extras?.getBoolean("android.content.extra.IS_SENSITIVE") == true
    private fun unlocked() = context.getSystemService(PowerManager::class.java)?.isInteractive == true &&
        context.getSystemService(KeyguardManager::class.java)?.isDeviceLocked == false
}
