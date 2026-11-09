package app.qingjian.inputmethod

import android.app.Application
import app.qingjian.inputmethod.core.AppLog

/** 应用进程的入口：输入法、设置页、翻译都在这一个进程里，日志在这里先装上。 */
class QingjianApp : Application() {
    val clipboardWatcher by lazy { app.qingjian.inputmethod.clipboard.ClipboardWatcher(this) }

    override fun onCreate() {
        super.onCreate()
        AppLog.init(this)
        // 上次进程中断遗留的普通图片暂存；应用尚未开始监听，当前没有导入任务。
        app.qingjian.inputmethod.clipboard.ClipboardImports.recover(java.io.File(filesDir, "user/clipboard-import"))
        app.qingjian.inputmethod.clipboard.sync.ClipboardSync.apply(this)
        clipboardWatcher.start()
        app.qingjian.inputmethod.update.Updates.start(this)
        app.qingjian.inputmethod.voice.VoiceModel.cleanup(this)
        app.qingjian.inputmethod.cloud.MemberAccount.start(this)
    }
}
