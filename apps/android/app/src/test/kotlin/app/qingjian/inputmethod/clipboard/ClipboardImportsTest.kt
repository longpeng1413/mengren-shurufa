package app.qingjian.inputmethod.clipboard

import java.nio.file.Files
import org.junit.Assert.*
import org.junit.Test

class ClipboardImportsTest {
    @Test fun failedImportAndProcessRecoveryDoNotDeleteOtherFiles() {
        val directory = Files.createTempDirectory("qj-import-fixture").toFile()
        try {
            val result = runCatching { ClipboardImports.use(directory) { file ->
                file.writeText("synthetic partial")
                error("synthetic stream failure")
            } }
            assertTrue(result.isFailure)
            assertTrue(directory.listFiles()!!.isEmpty())
            val stale = directory.resolve("image-crashed.tmp").apply { writeText("fixture") }
            val keep = directory.resolve("user-notes.txt").apply { writeText("keep") }
            val nested = directory.resolve("image-directory.tmp").apply { mkdir() }
            val inside = nested.resolve("keep.txt").apply { writeText("keep") }
            ClipboardImports.recover(directory)
            assertFalse(stale.exists())
            assertTrue(keep.exists()); assertTrue(inside.exists())
        } finally { directory.deleteRecursively() }
    }
}
