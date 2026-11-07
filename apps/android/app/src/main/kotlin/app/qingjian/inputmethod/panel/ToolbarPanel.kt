package app.qingjian.inputmethod.panel

import android.annotation.SuppressLint
import android.content.Context
import android.graphics.Canvas
import android.graphics.RectF
import android.view.MotionEvent
import android.view.ViewConfiguration
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.keyboard.Haptics
import app.qingjian.inputmethod.panel.base.PanelIcons.Icon
import app.qingjian.inputmethod.panel.base.PanelView
import app.qingjian.inputmethod.panel.base.Tabs
import app.qingjian.inputmethod.settings.Settings
import app.qingjian.inputmethod.settings.ToolbarTool
import app.qingjian.inputmethod.theme.Palette

/**
 * 定制工具栏（#777，功能面板「常用功能」里进），在键盘区原地编辑：上面一排是工具栏上的、按顺序，右上角「−」点了移除；
 * 下面是全部工具，放了的高亮，点没放的加到末尾、点放了的移除。左上角 ✓ 完成才写进设置并回到键盘，
 * 系统返回键放弃这次改动；右上角「恢复默认」先换成 [ToolbarTool.DEFAULT]，同样要点 ✓ 才算。
 */
@SuppressLint("ViewConstructor")
class ToolbarPanel(
    context: Context,
    palette: Palette,
    actions: PanelActions,
    headerHeight: Float,
    private val saved: List<ToolbarTool>,
) : PanelView(context, palette, actions, headerHeight) {
    private val tabs = Tabs(ink, scrollable(true) { _, _ -> false }, Tabs.Style.UNDERLINE)
        .apply { labels = listOf("定制工具栏") }

    private val card = RectF()
    private val scroll = scrollable(false) { x, y -> card.contains(x, y) }
    private val columns = 5

    /** 正在编辑的工具栏，点 ✓ 才存。 */
    private val order = ToolbarOrder(saved)
    private var placed: List<ToolbarTool>
        get() = order.items
        set(value) { order.replace(value) }
    private var dragPointer = -1
    private var dragX = 0f
    private var dragY = 0f
    private val dragSlop = ViewConfiguration.get(context).scaledTouchSlop
    private val dragScroll = object : Runnable {
        override fun run() {
            if (order.dragged == null) return
            val edge = ink.dp(30f)
            val delta = when {
                dragY < card.top + edge -> -ink.dp(5f)
                dragY > card.bottom - edge -> ink.dp(5f)
                else -> 0f
            }
            val lastOffset = (sectionTitle + rows(placed.size)*cellHeight - card.height()).coerceAtLeast(0f)
            val next = (scroll.offset + delta).coerceIn(0f, lastOffset)
            scroll.drag(next - scroll.offset)
            moveDragged()
            invalidate()
            postDelayed(this,16L)
        }
    }

    private val sectionTitle get() = ink.dp(34f)
    private val cellHeight get() = ink.dp(68f)

    private fun rows(count: Int) = maxOf(1, (count + columns - 1) / columns)

    /** 下面「全部工具」那一节在内容里的起点（相对卡片顶，未滚动）。 */
    private val allTop get() = sectionTitle + rows(placed.size) * cellHeight

    private val contentHeight get() = allTop + sectionTitle + rows(ToolbarTool.entries.size) * cellHeight + ink.dp(8f)

    private fun cellWidth() = card.width() / columns

    private enum class Badge { REMOVE, ADD, PLACED }

    override fun onDraw(canvas: Canvas) {
        drawHeader(canvas, tabs, leftIcon = Icon.CHECK)
        ink.text(canvas, RESET, 14f, palette.brand, width - ink.dp(RESET_WIDTH_DP) / 2, headerHeight / 2)
        card.set(ink.dp(10f), headerHeight, width - ink.dp(10f), height - ink.dp(8f))
        ink.round(canvas, card, 16f, palette.surface)
        scroll.max = contentHeight - card.height()
        canvas.save()
        canvas.clipRect(card)
        val top = card.top - scroll.offset
        drawTitle(canvas, context.getString(R.string.toolbar_drag_order), top)
        if (placed.isEmpty()) {
            ink.text(canvas, "中间空着，从下面加", 13f, palette.gloss, card.centerX(), top + sectionTitle + cellHeight / 2)
        }
        placed.forEachIndexed { i, tool -> drawCell(canvas, tool, i, top + sectionTitle, Badge.REMOVE) }
        drawTitle(canvas, "全部工具（点一下加到末尾）", top + allTop)
        ToolbarTool.entries.forEachIndexed { i, tool ->
            drawCell(canvas, tool, i, top + allTop + sectionTitle, if (tool in placed) Badge.PLACED else Badge.ADD)
        }
        canvas.restore()
        order.dragged?.let { tool ->
            val rect = RectF(dragX - ink.dp(24f), dragY - ink.dp(24f), dragX + ink.dp(24f), dragY + ink.dp(24f))
            ink.round(canvas, rect, 12f, palette.surface)
            ink.icon(canvas, tool.icon, dragX, dragY, 30f, palette.brand)
        }
    }

    private fun drawTitle(canvas: Canvas, title: String, top: Float) {
        ink.textStart(canvas, title, 12f, palette.gloss, card.left + ink.dp(16f), top + sectionTitle / 2 + ink.dp(4f), card.right - ink.dp(16f))
    }

    private fun drawCell(canvas: Canvas, tool: ToolbarTool, index: Int, top: Float, badge: Badge) {
        val cx = card.left + cellWidth() * (index % columns + 0.5f)
        val cy = top + cellHeight * (index / columns + 0.5f)
        val highlighted = badge == Badge.PLACED
        ink.icon(canvas, tool.icon, cx, cy - ink.dp(10f), 26f, if (highlighted) palette.brand else palette.text)
        val label = when (tool) {
            ToolbarTool.PRIVATE_SEND -> context.getString(app.qingjian.inputmethod.R.string.private_send)
            ToolbarTool.PRIVATE_INPUT -> context.getString(app.qingjian.inputmethod.R.string.private_input)
            ToolbarTool.PHRASES -> context.getString(app.qingjian.inputmethod.R.string.custom_phrases)
            else -> tool.label
        }
        ink.text(canvas, label, 12f, if (highlighted) palette.brand else palette.gloss, cx, cy + ink.dp(18f), maxWidth = cellWidth() - ink.dp(4f))
        val bx = cx + ink.dp(17f)
        val by = cy - ink.dp(24f)
        ink.fill.color = if (highlighted) palette.brand else palette.functionalKey
        canvas.drawCircle(bx, by, ink.dp(7.5f), ink.fill)
        val mark = when (badge) {
            Badge.REMOVE -> Icon.MINUS
            Badge.ADD -> Icon.PLUS
            Badge.PLACED -> Icon.CHECK
        }
        ink.icon(canvas, mark, bx, by, 13f, if (highlighted) palette.onBrand else palette.text)
    }

    /** 左上角是 ✓：存下并回到键盘。 */
    override fun onBack() {
        finishDrag(false)
        actions.closePanel()
        if (placed != saved) actions.setChoice(Settings.KEY_TOOLBAR, ToolbarTool.format(placed))
    }

    override fun onHold(x: Float, y: Float, rawX: Float) {
        val index = placedIndex(x,y) ?: return
        if (!order.begin(index)) return
        dragX = x; dragY = y
        Haptics.tap(context)
        parent?.requestDisallowInterceptTouchEvent(true)
        removeCallbacks(dragScroll)
        post(dragScroll)
        invalidate()
    }

    private fun placedIndex(x: Float, y: Float): Int? {
        if (!card.contains(x,y)) return null
        val contentY = y - card.top + scroll.offset - sectionTitle
        if (contentY < 0 || contentY >= rows(placed.size)*cellHeight) return null
        val column = ((x-card.left)/cellWidth()).toInt().coerceIn(0,columns-1)
        return ((contentY/cellHeight).toInt()*columns+column).takeIf { it in placed.indices }
    }

    private fun moveDragged() {
        if (placed.isEmpty() || dragX < card.left-dragSlop || dragX > card.right+dragSlop) return
        val contentY = (dragY-card.top+scroll.offset-sectionTitle).coerceAtLeast(0f)
        val column = ((dragX-card.left)/cellWidth()).toInt().coerceIn(0,columns-1)
        val index = ((contentY/cellHeight).toInt()*columns+column).coerceIn(0,placed.lastIndex)
        order.move(index)
    }

    private fun finishDrag(cancelled: Boolean) {
        removeCallbacks(dragScroll)
        if (cancelled) order.cancel() else order.finish()
        dragPointer = -1
        parent?.requestDisallowInterceptTouchEvent(false)
        invalidate()
    }

    @SuppressLint("ClickableViewAccessibility")
    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) dragPointer = event.getPointerId(0)
        if (order.dragged == null) return super.onTouchEvent(event)
        val pointer = event.findPointerIndex(dragPointer)
        if (pointer < 0 || event.pointerCount != 1) { finishDrag(true); return true }
        dragX = event.getX(pointer); dragY = event.getY(pointer)
        when (event.actionMasked) {
            MotionEvent.ACTION_MOVE -> { moveDragged(); invalidate() }
            MotionEvent.ACTION_UP -> { moveDragged(); finishDrag(false) }
            MotionEvent.ACTION_CANCEL -> finishDrag(true)
        }
        return true
    }

    override fun onDetachedFromWindow() {
        finishDrag(true)
        super.onDetachedFromWindow()
    }

    override fun onTap(x: Float, y: Float) {
        if (y < headerHeight) {
            if (x > width - ink.dp(RESET_WIDTH_DP)) placed = ToolbarTool.DEFAULT
            return
        }
        if (!card.contains(x, y)) return
        val inContent = y - card.top + scroll.offset
        val column = ((x - card.left) / cellWidth()).toInt().coerceIn(0, columns - 1)
        if (inContent >= allTop + sectionTitle) {
            val row = ((inContent - allTop - sectionTitle) / cellHeight).toInt()
            val tool = ToolbarTool.entries.getOrNull(row * columns + column) ?: return
            placed = if (tool in placed) placed - tool else placed + tool
        } else if (inContent in sectionTitle..allTop) {
            val row = ((inContent - sectionTitle) / cellHeight).toInt()
            placed.getOrNull(row * columns + column)?.let { placed = placed - it }
        }
    }

    private companion object {
        const val RESET = "恢复默认"
        const val RESET_WIDTH_DP = 100f
    }
}
