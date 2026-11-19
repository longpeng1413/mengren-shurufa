package app.qingjian.inputmethod.clipboard.files

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import app.qingjian.inputmethod.R
import java.io.File
import kotlin.concurrent.thread

/** Android 8/9 通过系统创建文档，无需共享存储写权限。 */
class SaveActivity: Activity() {
    private fun source()=File(requireNotNull(intent.getStringExtra("path")))
    override fun onCreate(savedInstanceState:Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState==null) startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT)
            .addCategory(Intent.CATEGORY_OPENABLE).setType(LanFiles.mime(source().name))
            .putExtra(Intent.EXTRA_TITLE,source().name),1)
    }
    @Deprecated("Platform activity result")
    override fun onActivityResult(requestCode:Int,resultCode:Int,data:Intent?) {
        super.onActivityResult(requestCode,resultCode,data)
        val uri=data?.data
        if (requestCode==1 && resultCode==RESULT_OK && uri!=null) thread {
            runCatching { LanFiles.copy(this,source(),uri) }
                .onSuccess { LanFiles.notice(this,R.string.file_saved) }
                .onFailure { runCatching { contentResolver.delete(uri,null,null) }; LanFiles.notice(this,R.string.file_failed) }
        }
        finish()
    }
}
