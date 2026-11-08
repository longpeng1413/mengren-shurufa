package app.qingjian.inputmethod.clipboard

import org.junit.Assert.*
import org.junit.Test

class ClipboardSuggestionStateTest {
    private fun item(token: String = "copy-a", timestamp: Long = 1_000L) =
        ClipboardSuggestionState.Item(token, timestamp, "001234", false, true)

    @Test fun switchingFieldsKeepsFreshCopyButConsumptionDoesNotReofferIt() {
        val state = ClipboardSuggestionState()
        assertEquals("001234", state.offer(item(), 2_000)?.text)
        state.hide()
        assertNotNull(state.offer(item(), 3_000))
        state.dismiss()
        assertNull(state.offer(item(), 4_000))
        assertNotNull(state.offer(item("copy-b"), 4_000))
    }

    @Test fun expirationUsesCopyTimeAndRejectsUnknownOrFutureTimestamps() {
        val state = ClipboardSuggestionState()
        assertNotNull(state.offer(item(), 300_999))
        assertNull(state.current(301_000))
        assertNull(state.offer(item(), 301_001))
        assertNull(state.offer(item(timestamp = 0), 2_000))
        assertNull(state.offer(item(timestamp = 3_000), 2_000))
    }
}
