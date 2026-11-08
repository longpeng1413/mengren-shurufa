//! 覆盖角标时保留双拼和保留手势，拒绝控制字符。
package app.qingjian.inputmethod.keyboard.layout

import org.junit.Assert.*
import org.junit.Test

class FlickSymbolsTest {
    @Test fun overridesPreserveKeyboardBehavior() {
        val original = Key.Letter('a', "!", "zh", "ang", 1.5f)
        val shifted = Key.Letter(';', "", weight = 1.5f, flickShift = true)
        val digit = Key.Digit('2', "ABC")
        val rows = FlickSymbols.apply(listOf(listOf(original, shifted, digit)), mapOf('a' to "💡", ';' to "?"))[0]
        val letter = rows[0] as Key.Letter
        assertEquals("💡", letter.hint)
        assertEquals("💡", letter.flick)
        assertEquals("zh", letter.initial)
        assertEquals("ang", letter.finals)
        assertEquals(1.5f, letter.weight)
        assertSame(shifted, rows[1])
        assertSame(digit, rows[2])
        assertEquals("!", original.hint)
    }
    @Test fun missingAndInvalidOverridesKeepDefaults() {
        val key = Key.Letter('a', "!")
        assertSame(key, FlickSymbols.apply(listOf(listOf(key)), emptyMap())[0][0])
        assertSame(key, FlickSymbols.apply(listOf(listOf(key)), mapOf('a' to "\n"))[0][0])
    }
    @Test fun unicodeAndLengthValidation() {
        assertTrue(FlickSymbols.valid("💡".repeat(8)))
        assertFalse(FlickSymbols.valid("💡".repeat(9)))
        assertTrue(FlickSymbols.valid("()"))
        assertFalse(FlickSymbols.valid(""))
        assertFalse(FlickSymbols.valid("a b"))
        assertFalse(FlickSymbols.valid("\u0000"))
    }
}
