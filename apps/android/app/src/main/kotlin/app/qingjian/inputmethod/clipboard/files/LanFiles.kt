package app.qingjian.inputmethod.clipboard.files

import android.content.*
import android.net.Uri
import android.os.Build
import android.provider.DocumentsContract
import android.provider.MediaStore
import android.provider.OpenableColumns
import android.os.PowerManager
import android.system.Os
import android.system.OsConstants
import android.webkit.MimeTypeMap
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.clipboard.sync.PrivateNotice
import app.qingjian.inputmethod.core.Native
import java.io.File
import kotlin.concurrent.thread
import org.json.JSONObject

internal object LanFiles {
    @Volatile var lastClipboardChange = 0L
    private const val LIMIT = 300_000_000L
    fun root(c:Context) = File(c.filesDir,"user/clipboard-sync/received-files")
    fun received(c:Context): List<File> = root(c).listFiles().orEmpty()
        .filter { it.name.startsWith("item-") && it.isDirectory }
        .sortedByDescending { it.lastModified() }.mapNotNull { it.listFiles()?.singleOrNull()?.takeIf(File::isFile) }

    fun single(c:Context) = notice(c,R.string.file_single)
    fun notice(c:Context,id:Int) = PrivateNotice.show(c,c.getString(id))
    fun mime(name:String) = MimeTypeMap.getSingleton().getMimeTypeFromExtension(name.substringAfterLast('.', "").lowercase()) ?: "application/octet-stream"

    /** 系统截图服务提供的是图片内容。普通文档或具名文件 URI 优先保留原件；读取失败时不降级为公网图片。 */
    fun isFile(c:Context,uri:Uri):Boolean {
        if (uri.scheme != "content") return true
        return runCatching {
            val resolver=c.contentResolver
            if (DocumentsContract.isDocumentUri(c,uri) || resolver.getType(uri)?.startsWith("image/") != true) return true
            val provider=c.packageManager.resolveContentProvider(uri.authority.orEmpty(),0)
            if (provider?.packageName == "com.android.systemui") return false
            resolver.query(uri,arrayOf(OpenableColumns.DISPLAY_NAME),null,null,null)?.use {
                it.moveToFirst() && it.getColumnIndex(OpenableColumns.DISPLAY_NAME) >= 0 && !it.getString(it.getColumnIndexOrThrow(OpenableColumns.DISPLAY_NAME)).isNullOrEmpty()
            } ?: true
        }.getOrDefault(true)
    }

    fun send(context:Context,uri:Uri,done:()->Unit = {}) {
        val c=context.applicationContext
        thread(name="mengren-lan-file-open") {
            try {
                check(Native.lanFileEnabled())
                require(uri.scheme == "content" && !uri.authority.isNullOrEmpty() && '@' !in uri.authority.orEmpty())
                val provider=c.packageManager.resolveContentProvider(uri.authority!!,0)
                require(provider?.applicationInfo?.uid != android.os.Process.myUid() && !uri.authority.orEmpty().startsWith(c.packageName+"."))
                val resolver = c.contentResolver
                if (resolver.getType(uri) == DocumentsContract.Document.MIME_TYPE_DIR) { single(c); return@thread }
                var name:String? = null
                var size:Long? = null
                resolver.query(uri,arrayOf(OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE),null,null,null)?.use {
                    if (it.moveToFirst()) {
                        val label=it.getColumnIndex(OpenableColumns.DISPLAY_NAME)
                        val length=it.getColumnIndex(OpenableColumns.SIZE)
                        if (label>=0) name=it.getString(label)
                        if (length>=0 && !it.isNull(length)) size=it.getLong(length).takeIf { n -> n>=0 }
                    }
                }
                if ((size ?: 0) >= LIMIT) { notice(c,R.string.file_invalid); return@thread }
                val filename=name ?: uri.lastPathSegment?.substringAfterLast('/') ?: error("Missing file name")
                resolver.openFileDescriptor(uri,"r")?.use { fd ->
                    val stat=Os.fstat(fd.fileDescriptor)
                    if (OsConstants.S_ISDIR(stat.st_mode)) { single(c); return@thread }
                    require(OsConstants.S_ISREG(stat.st_mode) || OsConstants.S_ISFIFO(stat.st_mode) || OsConstants.S_ISSOCK(stat.st_mode))
                    LanNetwork.refresh()
                    check(Native.lanFileSend(fd.fd,filename,size ?: fd.statSize))
                } ?: error("Cannot open URI")
            } catch (_: Exception) { notice(c,R.string.file_failed) }
            finally { done() }
        }
    }

    fun event(c:Context,type:String,event:JSONObject) {
        if (type=="file_status") {
            notice(c,when(event.optString("status")) {
                "sent" -> R.string.file_sent
                "partial" -> R.string.file_partial
                "invalid" -> R.string.file_invalid
                "failed" -> R.string.file_failed
                else -> R.string.file_unavailable
            })
            return
        }
        val file=File(event.getString("path"))
        runCatching {
            val board=c.getSystemService(ClipboardManager::class.java) ?: return@runCatching
            if (c.getSystemService(PowerManager::class.java)?.isInteractive != true) return@runCatching
            val previous=board.primaryClipDescription
            if (maxOf(lastClipboardChange,previous?.timestamp ?: 0) >= event.getLong("stamp")) return@runCatching
            val uri=Uri.Builder().scheme("content").authority(c.packageName+".lanfiles")
                .appendPath(file.parentFile!!.name).appendPath(file.name).build()
            board.setPrimaryClip(ClipData.newUri(c.contentResolver,file.name,uri))
        }
        notice(c,R.string.file_received)
    }

    fun save(c:Context,file:File) {
        if (Build.VERSION.SDK_INT < 29) {
            c.startActivity(Intent(c,SaveActivity::class.java).putExtra("path",file.path).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            return
        }
        thread(name="mengren-lan-save") {
            val resolver=c.contentResolver
            var pending:Uri?=null
            try {
                val values=ContentValues().apply {
                    put(MediaStore.MediaColumns.DISPLAY_NAME,file.name)
                    put(MediaStore.MediaColumns.MIME_TYPE,mime(file.name))
                    put(MediaStore.MediaColumns.RELATIVE_PATH,android.os.Environment.DIRECTORY_DOWNLOADS)
                    put(MediaStore.MediaColumns.IS_PENDING,1)
                }
                val uri=resolver.insert(MediaStore.Downloads.EXTERNAL_CONTENT_URI,values) ?: error("Cannot create download")
                pending=uri
                copy(c,file,uri)
                values.clear()
                values.put(MediaStore.MediaColumns.IS_PENDING,0)
                check(resolver.update(uri,values,null,null)==1)
                pending=null
                notice(c,R.string.file_saved)
            } catch (_:Exception) { notice(c,R.string.file_failed) }
            finally { pending?.let { runCatching { resolver.delete(it,null,null) } } }
        }
    }

    fun copy(c:Context,file:File,uri:Uri) {
        file.inputStream().use { input ->
            c.contentResolver.openOutputStream(uri,"w")?.use { output -> check(input.copyTo(output,64*1024)==file.length()) }
                ?: error("Cannot write")
        }
    }
}
