package app.qingjian.inputmethod.clipboard.files

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import app.qingjian.inputmethod.clipboard.sync.ClipboardSync
import app.qingjian.inputmethod.R

/** 只接受系统分享的文件 URI。保持授权直到描述符已复制。 */
class ShareActivity: Activity() {
    @Suppress("DEPRECATION")
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState!=null) { finish(); return }
        val clip=intent.clipData
        if (intent.action!=Intent.ACTION_SEND || (clip?.itemCount ?: 0)>1) {
            LanFiles.single(this); finish(); return
        }
        val uri=intent.getParcelableExtra<Uri>(Intent.EXTRA_STREAM) ?: clip?.getItemAt(0)?.uri
        if (uri==null || uri.scheme!="content" || clip?.getItemAt(0)?.uri?.let { it!=uri }==true ||
            intent.flags and Intent.FLAG_GRANT_READ_URI_PERMISSION == 0) {
            LanFiles.notice(this,R.string.file_failed); finish(); return
        }
        ClipboardSync.apply(this)
        LanFiles.send(this,uri) { runOnUiThread { finish() } }
    }
}
