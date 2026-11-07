package app.qingjian.inputmethod.clipboard.sync

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.widget.Toast

/** 只保留当前一条无正文提示；连续操作不能排成过时的通知队列。 */
internal object PrivateNotice {
    private val main = Handler(Looper.getMainLooper())
    private var toast: Toast? = null
    private var lastMessage = ""
    private var lastAt = Long.MIN_VALUE

    fun show(context: Context, message: String) {
        main.post {
            val now = SystemClock.elapsedRealtime()
            if (message == lastMessage && lastAt != Long.MIN_VALUE && now - lastAt < 4_000L) return@post
            toast?.cancel()
            lastMessage = message; lastAt = now
            toast = Toast.makeText(context.applicationContext, message, Toast.LENGTH_SHORT).also { it.show() }
        }
    }
}
