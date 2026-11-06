package app.qingjian.inputmethod.service.input

import android.text.InputType
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection

/**
 * 猛人输入法：删除键的撤回记录。删除键上滑清空，下滑放回上一次删掉的文字，可以一直往前撤。
 *
 * 一次删除记一条：一次上滑清空、连着的几下单删（点按或长按连删，中间没打别的字）。只记已经上屏的文字；
 * 正在打的拼音不记——引擎只给显示用的拼音，双拼的按键还原不回去。
 *
 * 只放在内存里：最多 [MAX_ENTRIES] 条、合计 [MAX_CHARS] 个字符，超出丢最早的；换输入框、收起键盘时由服务清空；
 * 密码框不记（[enabled] 为 false），免得内存里留着密码。
 *
 * 撤回前核对光标两边的文字和删完那一刻一样；不一样（挪了光标、打了别的字、应用自己改了内容）就整份作废，
 * 免得把字放错地方。
 */
class DeleteHistory {
    /**
     * [before] / [after] 是删掉的光标前、后的文字，撤回时光标回到两者之间；
     * [anchorBefore] / [anchorAfter] 是删完那一刻光标前后各最多 [ANCHOR] 个字符，撤回前拿来核对；
     * [run] 是单删连成的一条，后面接着单删就并进来。
     */
    private class Entry(var before: String, val after: String, var anchorBefore: String, val anchorAfter: String, val run: Boolean)

    private val entries = ArrayDeque<Entry>()

    /** 这个输入框记不记；关掉时顺手清空。密码框关。 */
    var enabled = true
        set(value) {
            field = value
            if (!value) clear()
        }

    val isEmpty: Boolean get() = entries.isEmpty()

    fun clear() = entries.clear()

    /** 单删（发 DEL 之前）调：记下要删的那个字或选中的文字，连着删的并成一条。 */
    fun beforeBackspace(connection: InputConnection?) {
        if (!enabled || connection == null) return
        val textBefore = connection.getTextBeforeCursor(CONTEXT, 0)?.toString() ?: return
        val textAfter = connection.getTextAfterCursor(CONTEXT, 0)?.toString() ?: return
        val selected = connection.getSelectedText(0)?.toString().orEmpty()
        val removed: String
        val left: String
        if (selected.isNotEmpty()) {
            removed = selected
            left = textBefore
        } else {
            if (textBefore.isEmpty()) return
            // 按码点退一格：emoji 这类代理对算一个字
            val cut = textBefore.offsetByCodePoints(textBefore.length, -1)
            removed = textBefore.substring(cut)
            left = textBefore.substring(0, cut)
        }
        val top = entries.lastOrNull()
        if (selected.isEmpty() && top != null && top.run && top.anchorBefore == tail(textBefore) && top.anchorAfter == head(textAfter)) {
            top.before = removed + top.before
            top.anchorBefore = tail(left)
        } else {
            entries.addLast(Entry(removed, "", tail(left), head(textAfter), run = selected.isEmpty()))
        }
        trim()
    }

    /** 上滑清空：删掉光标两边（连同选中的）全部文字并记一条。返回删没删掉东西。 */
    fun clearField(connection: InputConnection?): Boolean {
        connection ?: return false
        val before = connection.getTextBeforeCursor(MAX_CLEAR, 0)?.toString().orEmpty()
        val selected = connection.getSelectedText(0)?.toString().orEmpty()
        val after = connection.getTextAfterCursor(MAX_CLEAR, 0)?.toString().orEmpty()
        if (before.isEmpty() && selected.isEmpty() && after.isEmpty()) return false
        connection.beginBatchEdit()
        if (selected.isNotEmpty()) connection.commitText("", 1)
        connection.deleteSurroundingText(before.length, after.length)
        connection.endBatchEdit()
        if (enabled) {
            // 超长的输入框只删得掉光标两边各 MAX_CLEAR 个字，剩下的不知道是什么，锚点留空：那种情况下撤回会因核对不上而作废
            entries.addLast(Entry(before + selected, after, "", "", run = false))
            trim()
        }
        return true
    }

    /** 下滑撤回：放回最近一次删掉的文字，光标回到删之前的位置。返回撤没撤；核对不上时整份作废。 */
    fun undo(connection: InputConnection?): Boolean {
        connection ?: return false
        val top = entries.lastOrNull() ?: return false
        val textBefore = connection.getTextBeforeCursor(ANCHOR, 0)?.toString()
        val textAfter = connection.getTextAfterCursor(ANCHOR, 0)?.toString()
        val selected = connection.getSelectedText(0)?.toString().orEmpty()
        if (textBefore == null || textAfter == null || selected.isNotEmpty() ||
            tail(textBefore) != top.anchorBefore || head(textAfter) != top.anchorAfter
        ) {
            clear()
            return false
        }
        entries.removeLast()
        connection.beginBatchEdit()
        connection.commitText(top.before, 1)
        // newCursorPosition 为 0：光标停在这段文字的开头，也就是 before 与 after 之间
        if (top.after.isNotEmpty()) connection.commitText(top.after, 0)
        connection.endBatchEdit()
        return true
    }

    private fun trim() {
        while (entries.size > MAX_ENTRIES || (entries.size > 1 && entries.sumOf { it.before.length + it.after.length } > MAX_CHARS)) {
            entries.removeFirst()
        }
    }

    private fun tail(text: String) = text.takeLast(ANCHOR)

    private fun head(text: String) = text.take(ANCHOR)

    private companion object {
        /** 撤回前核对光标两边多少个字符。 */
        const val ANCHOR = 64

        /** 单删时往前读多少个字符（要比 [ANCHOR] 长，才算得出删完后的锚点）。 */
        const val CONTEXT = ANCHOR + 8

        /** 上滑清空时光标两边各最多删多少个字符。 */
        const val MAX_CLEAR = 5_000

        const val MAX_ENTRIES = 20
        const val MAX_CHARS = 10_000
    }
}

/** 密码框（文本或数字）：删除键的撤回不记，免得内存里留着密码。 */
fun EditorInfo.isPasswordField(): Boolean {
    val variation = inputType and InputType.TYPE_MASK_VARIATION
    return when (inputType and InputType.TYPE_MASK_CLASS) {
        InputType.TYPE_CLASS_TEXT -> variation == InputType.TYPE_TEXT_VARIATION_PASSWORD ||
            variation == InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD ||
            variation == InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD
        InputType.TYPE_CLASS_NUMBER -> variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD
        else -> false
    }
}
