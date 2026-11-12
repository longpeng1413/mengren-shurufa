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

    /**
     * 只在写包、提交会话的那几秒拦重复点击。不等系统回报结果：有的系统在确认框里点取消后不回调，
     * 等结果会让「立即更新」一直没反应（键盘与设置同进程，进程不退出就恢复不了）。
     */
    private val writing = AtomicBoolean(false)

    /** 本进程最近一次提交的安装会话；作废的旧会话补发的结果不认（-1 为本进程还没装过）。 */
    @Volatile var session: Int = -1
        private set

    /** 正在写包或提交；没在写时界面上残留的「正在准备安装」可以改回「可以安装」。 */
    val busy: Boolean get() = writing.get()

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
        if (!writing.compareAndSet(false, true)) return
        val app = context.applicationContext
        val apk = Updates.pending(app)
        if (apk == null) { writing.set(false); Updates.fail(app, "package"); return }
        confirmation = null
        Updates.status(app, "installing")
        thread(name = "mengren-package-install") {
            var sessionId: Int? = null
            val installer = app.packageManager.packageInstaller
            try {
                verify(app, apk)
                // 上次确认框被取消却没回调的会话还挂着，先作废，再开新的
                installer.mySessions.forEach { runCatching { installer.abandonSession(it.sessionId) } }
                val params = PackageInstaller.SessionParams(PackageInstaller.SessionParams.MODE_FULL_INSTALL).apply {
                    setAppPackageName(app.packageName); setSize(apk.length())
                    if (Build.VERSION.SDK_INT >= 31) setRequireUserAction(PackageInstaller.SessionParams.USER_ACTION_REQUIRED)
                }
                val id = installer.createSession(params); sessionId = id; session = id
                installer.openSession(id).use { session ->
                    apk.inputStream().use { input -> session.openWrite("base.apk", 0, apk.length()).use { output -> input.copyTo(output, 1024 * 1024); session.fsync(output) } }
                    val intent = Intent(app, UpdateInstallReceiver::class.java)
                    val flags = PendingIntent.FLAG_UPDATE_CURRENT or if (Build.VERSION.SDK_INT >= 31) PendingIntent.FLAG_MUTABLE else 0
                    session.commit(PendingIntent.getBroadcast(app, id, intent, flags).intentSender)
                }
            } catch (_: Exception) {
                sessionId?.let { runCatching { installer.abandonSession(it) } }
                Updates.fail(app, "package")
            } finally {
                writing.set(false)
            }
        }
    }

    /** 系统回报了结果（成功、取消或失败）：留着的确认界面不再有用。 */
    fun finished() { confirmation = null }
}
