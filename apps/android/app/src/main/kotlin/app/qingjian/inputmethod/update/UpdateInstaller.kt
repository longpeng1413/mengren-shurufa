package app.qingjian.inputmethod.update

import android.app.Activity
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageInstaller
import android.content.pm.PackageManager
import android.os.Build
import java.io.File
import java.lang.ref.WeakReference
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread

/** 复用猛人快传的 PackageInstaller 会话方式，保留系统确认和原 APK 签名。 */
object UpdateInstaller {
    var foreground: WeakReference<Activity>? = null
    var confirmation: Intent? = null
    private val installing = AtomicBoolean(false)
    @Suppress("DEPRECATION")
    private fun verify(context: Context, apk: File) {
        val flags = if (Build.VERSION.SDK_INT >= 28) PackageManager.GET_SIGNING_CERTIFICATES else PackageManager.GET_SIGNATURES
        val installed = context.packageManager.getPackageInfo(context.packageName, flags)
        val candidate = context.packageManager.getPackageArchiveInfo(apk.absolutePath, flags) ?: error("invalid APK")
        require(candidate.packageName == context.packageName)
        require(if (Build.VERSION.SDK_INT >= 28) candidate.longVersionCode > installed.longVersionCode else candidate.versionCode > installed.versionCode)
        val old = if (Build.VERSION.SDK_INT >= 28) installed.signingInfo?.apkContentsSigners else installed.signatures
        val next = if (Build.VERSION.SDK_INT >= 28) candidate.signingInfo?.apkContentsSigners else candidate.signatures
        require(!old.isNullOrEmpty() && old.toSet() == next?.toSet())
    }
    fun install(context: Context) {
        if (!installing.compareAndSet(false, true)) return
        val app = context.applicationContext
        val apk = Updates.pending(app)
        if (apk == null) { installing.set(false); Updates.fail(app, "package"); return }
        Updates.status(app, "installing")
        thread(name = "mengren-package-install") {
            var sessionId: Int? = null
            val installer = app.packageManager.packageInstaller
            try {
                verify(app, apk)
                val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                    setAppPackageName(app.packageName); setSize(apk.length())
                    if (Build.VERSION.SDK_INT >= 31) setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
                }
                val id = installer.createSession(params); sessionId = id
                installer.openSession(id).use { session ->
                    apk.inputStream().use { input -> session.openWrite("base.apk", 0, apk.length()).use { output -> input.copyTo(output, 1024 * 1024); session.fsync(output) } }
                    val intent = Intent(app, UpdateInstallReceiver::class.java)
                    val flags = PendingIntent.FLAG_UPDATE_CURRENT or if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0
                    session.commit(PendingIntent.getBroadcast(app, id, intent, flags).intentSender)
                }
            } catch (_: Exception) {
                sessionId?.let { runCatching { installer.abandonSession(it) } }
                installing.set(false); Updates.fail(app, "package")
            }
        }
    }
    fun finished() { installing.set(false) }
}
