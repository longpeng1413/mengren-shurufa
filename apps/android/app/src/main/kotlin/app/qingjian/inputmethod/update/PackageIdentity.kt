package app.qingjian.inputmethod.update

/** 下载缓存必须同时匹配当前提供的版本与摘要，不能把旧包当成新版本安装。 */
internal fun matchesUpdatePackage(fileName: String, hash: String, packageVersion: String, offeredVersion: String): Boolean =
    offeredVersion.isNotBlank() && packageVersion == offeredVersion && hash.matches(Regex("[0-9a-f]{64}")) && fileName == "$hash.apk"
