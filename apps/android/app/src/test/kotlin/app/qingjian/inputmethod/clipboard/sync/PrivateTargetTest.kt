package app.qingjian.inputmethod.clipboard.sync

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PrivateTargetTest {
    @Test fun only_the_armed_connection_matches() {
        val guard = PrivateTarget<Any> { 100L }
        val first = Any()
        val other = Any()
        guard.arm(first)
        assertTrue(guard.matches(first))
        assertFalse(guard.matches(other))
        assertFalse(guard.matches(null))
        guard.clear()
        assertFalse(guard.matches(first))
    }

    @Test fun expiry_and_rearming_never_authorize_the_previous_field() {
        var now = 0L
        val guard = PrivateTarget<Any> { now }
        val first = Any()
        val second = Any()
        guard.arm(first)
        now = 119_999L
        assertTrue(guard.waiting)
        now = 120_000L
        assertFalse(guard.waiting)
        guard.arm(second)
        assertTrue(guard.matches(second))
        assertFalse(guard.matches(first))
    }
}
