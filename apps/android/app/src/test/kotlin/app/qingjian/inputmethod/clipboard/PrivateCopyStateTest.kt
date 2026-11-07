package app.qingjian.inputmethod.clipboard

import org.junit.Assert.*
import org.junit.Test

class PrivateCopyStateTest {
    @Test fun not_ready_does_not_consume_copy_but_accepted_copy_is_not_replayed() {
        val state = PrivateCopyState(0)
        assertTrue(state.shouldAttempt(1_000, 1_010))
        assertTrue(state.shouldAttempt(1_000, 1_020))
        state.accepted = 1_000
        assertFalse(state.shouldAttempt(1_000, 1_030))
        assertFalse(PrivateCopyState(state.accepted).shouldAttempt(1_000, 1_040))
        assertTrue(state.shouldAttempt(2_000, 2_000))
    }

    @Test fun old_or_future_clipboard_is_not_automatically_sent_after_restart() {
        val state = PrivateCopyState(0)
        assertFalse(state.shouldAttempt(1_000, 121_001))
        assertFalse(state.shouldAttempt(2_000, 1_000))
        assertTrue(state.shouldAttempt(1_000, 121_000))
    }
}
