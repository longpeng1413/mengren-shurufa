package app.qingjian.inputmethod.candidates

import org.junit.Assert.*
import org.junit.Test

class PrivateGestureTest {
    @Test fun obscuredDownCannotBecomeAuthorizedAtUpButNextCleanGestureWorks() {
        val gesture = PrivateGesture()
        gesture.observe(true, true, false)
        gesture.observe(false, false, false)
        assertFalse(gesture.allowed)
        gesture.observe(true, false, false)
        gesture.observe(false, false, false)
        assertTrue(gesture.allowed)
    }

    @Test fun laterObscuringOrMultiplePointersRevokesOnlyCurrentGesture() {
        val gesture = PrivateGesture()
        gesture.observe(true, false, false)
        gesture.observe(false, true, false)
        assertFalse(gesture.allowed)
        gesture.observe(true, false, false)
        gesture.observe(false, false, true)
        assertFalse(gesture.allowed)
    }
}
