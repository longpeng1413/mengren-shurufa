package app.qingjian.inputmethod.update

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.os.Build

/** 安装器通过显式 PendingIntent 回传状态，不接受外部应用的任意广播。 */
class UpdateInstallReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val id = intent.getIntExtra(PackageInstaller.EXTRA_SESSION_ID, -1)
        // 重新安装时作废的旧会话可能补发「已取消」，不能盖掉新会话的状态
        if (id != -1 && UpdateInstaller.session != -1 && id != UpdateInstaller.session) return
        when (intent.getIntExtra(PackageInstaller.EXTRA_STATUS, PackageInstaller.STATUS_FAILURE)) {
            PackageInstaller.STATUS_PENDING_USER_ACTION -> {
                val confirm = if (Build.VERSION.SDK_INT >= 33) intent.getParcelableExtra(Intent.EXTRA_INTENT, Intent::class.java)
                    else { @Suppress("DEPRECATION") intent.getParcelableExtra<Intent>(Intent.EXTRA_INTENT) }
                if (confirm == null) { UpdateInstaller.finished(); Updates.fail(context, "package"); return }
                val activity = UpdateInstaller.foreground?.get()?.takeUnless { it.isFinishing || it.isDestroyed }
                if (activity != null) runCatching { activity.startActivity(confirm) }.onFailure { Updates.fail(context, "package") }
                else { UpdateInstaller.confirmation = confirm; Updates.status(context, "confirmation") }
            }
            PackageInstaller.STATUS_SUCCESS -> { UpdateInstaller.finished(); Updates.status(context, "installed") }
            PackageInstaller.STATUS_FAILURE_ABORTED -> { UpdateInstaller.finished(); Updates.status(context, "error_cancelled") }
            else -> { UpdateInstaller.finished(); Updates.fail(context, "package") }
        }
    }
}
