package app.qingjian.inputmethod.panel

import app.qingjian.inputmethod.settings.ToolbarTool

/** 一次拖动只调整已选工具顺序；取消手势恢复拖动前的顺序，不丢掉其他编辑。 */
class ToolbarOrder(initial: List<ToolbarTool>) {
    var items = initial.toList()
        private set
    var dragged: ToolbarTool? = null
        private set
    private var beforeDrag = items

    fun replace(value: List<ToolbarTool>) { finish(); items = value.distinct() }

    fun begin(index: Int): Boolean {
        val tool = items.getOrNull(index) ?: return false
        beforeDrag = items.toList()
        dragged = tool
        return true
    }

    fun move(index: Int) {
        val tool = dragged ?: return
        val from = items.indexOf(tool)
        if (from < 0 || index !in items.indices || from == index) return
        items = items.toMutableList().apply { removeAt(from); add(index, tool) }
    }

    fun finish() { dragged = null }
    fun cancel() { if (dragged != null) items = beforeDrag; finish() }
}
