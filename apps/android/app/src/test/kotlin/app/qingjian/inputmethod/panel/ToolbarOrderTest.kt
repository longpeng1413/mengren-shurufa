package app.qingjian.inputmethod.panel

import app.qingjian.inputmethod.settings.ToolbarTool
import org.junit.Assert.*
import org.junit.Test

class ToolbarOrderTest {
    @Test fun dragAcrossRowsKeepsEverySelectedButtonExactlyOnce() {
        val original = ToolbarTool.entries.take(10)
        val state = ToolbarOrder(original)
        assertTrue(state.begin(1))
        state.move(6); state.move(8); state.move(8)
        state.finish()
        assertEquals(original[1], state.items[8])
        assertEquals(original.toSet(),state.items.toSet())
        assertEquals(original.size,state.items.size)
    }

    @Test fun cancelRestoresPreviousEditsAndOutsideIndicesDoNotRemoveButtons() {
        val state = ToolbarOrder(ToolbarTool.DEFAULT)
        val edited = ToolbarTool.DEFAULT.reversed()
        state.replace(edited)
        assertTrue(state.begin(0))
        state.move(4);state.move(999);state.move(-1)
        state.cancel()
        assertEquals(edited,state.items)
        assertFalse(state.begin(999))
        assertNull(state.dragged)
    }
}
