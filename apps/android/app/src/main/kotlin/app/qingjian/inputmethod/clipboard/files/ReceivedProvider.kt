package app.qingjian.inputmethod.clipboard.files

import android.content.*
import android.database.MatrixCursor
import android.net.Uri
import android.os.ParcelFileDescriptor
import android.provider.OpenableColumns
import java.io.File
import java.io.FileNotFoundException

/** 精确 URI 临时授权；只读已发布的一个文件，不暴露半成品或同级目录。 */
class ReceivedProvider:ContentProvider() {
    override fun onCreate()=true
    private fun file(uri:Uri):File {
        val parts=uri.pathSegments
        if(parts.size!=2 || !Regex("item-[A-Za-z0-9]+").matches(parts[0]) || '/' in parts[1]) throw FileNotFoundException()
        val root=LanFiles.root(requireNotNull(context)).canonicalFile
        val path=File(File(root,parts[0]),parts[1]).canonicalFile
        if(path.parentFile?.parentFile!=root || path.parentFile?.name!=parts[0] || !path.isFile) throw FileNotFoundException()
        return path
    }
    override fun getType(uri:Uri)=LanFiles.mime(file(uri).name)
    override fun openFile(uri:Uri,mode:String):ParcelFileDescriptor {
        if(mode!="r") throw FileNotFoundException()
        return ParcelFileDescriptor.open(file(uri),ParcelFileDescriptor.MODE_READ_ONLY)
    }
    override fun query(uri:Uri,projection:Array<out String>?,selection:String?,selectionArgs:Array<out String>?,sortOrder:String?):MatrixCursor {
        val f=file(uri)
        val cols=projection ?: arrayOf(OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE)
        return MatrixCursor(cols,1).apply {addRow(cols.map {when(it) {
            OpenableColumns.DISPLAY_NAME -> f.name
            OpenableColumns.SIZE -> f.length()
            "mime_type" -> LanFiles.mime(f.name)
            else -> null
        }})}
    }
    override fun insert(uri:Uri,values:ContentValues?):Uri?=throw UnsupportedOperationException()
    override fun delete(uri:Uri,selection:String?,selectionArgs:Array<out String>?):Int=throw UnsupportedOperationException()
    override fun update(uri:Uri,values:ContentValues?,selection:String?,selectionArgs:Array<out String>?):Int=throw UnsupportedOperationException()
}
