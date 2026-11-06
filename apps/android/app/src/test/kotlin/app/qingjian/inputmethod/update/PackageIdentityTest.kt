package app.qingjian.inputmethod.update

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PackageIdentityTest {
    @Test fun onlyCurrentVersionAndHashCanReuseDownload() {
        val hash = "a".repeat(64)
        assertTrue(matchesUpdatePackage("$hash.apk", hash, "0.3.5", "0.3.5"))
        assertFalse(matchesUpdatePackage("$hash.apk", hash, "0.3.5", "0.3.6"))
        assertFalse(matchesUpdatePackage("$hash.apk", "b".repeat(64), "0.3.5", "0.3.5"))
    }
    @Test fun malformedAndUnassociatedCacheIsRejected() {
        val hash = "a".repeat(64)
        assertFalse(matchesUpdatePackage("$hash.apk", hash, "", "0.3.5"))
        assertFalse(matchesUpdatePackage("wrong.apk", "../unsafe", "0.3.5", "0.3.5"))
    }
}
