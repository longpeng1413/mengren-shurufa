package app.qingjian.inputmethod.clipboard

import java.io.File

/** 受管的图片导入暂存：正常或异常结束都回收，进程中断由下次启动清理；不递归删除。 */
object ClipboardImports {
    fun <T> use(directory: File, action: (File) -> T): T {
        directory.mkdirs()
        val temporary = File.createTempFile("image-", ".tmp", directory)
        return try { action(temporary) } finally { temporary.delete() }
    }

    fun recover(directory: File) {
        directory.listFiles()?.filter {
            it.isFile && it.name.startsWith("image-") && it.name.endsWith(".tmp")
        }?.forEach { it.delete() }
    }
}
