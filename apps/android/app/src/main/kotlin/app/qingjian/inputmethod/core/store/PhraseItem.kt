package app.qingjian.inputmethod.core.store

/** 一条自定义短语；[index] 是它在全部自定义短语里的位置（改、删、移到最前都按它）。 */
class PhraseItem(val index: Int, val text: String, val group: String, val code: String)
