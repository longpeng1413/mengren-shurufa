//! 字母键上滑符号覆盖；仅替换角标，不改变双拼声韵母、宽度或 Shift 手势。
package app.qingjian.inputmethod.keyboard.layout

object FlickSymbols {
    const val PREFIX = "flick_symbol_"
    const val LETTERS = "qwertyuiopasdfghjklzxcvbnm"

    fun valid(value: String): Boolean = value.isNotEmpty() &&
        value.codePointCount(0, value.length) <= 8 &&
        value.codePoints().allMatch { !Character.isISOControl(it) && !Character.isWhitespace(it) }

    fun apply(rows: List<List<Key>>, overrides: Map<Char, String>): List<List<Key>> = rows.map { row ->
        row.map { key ->
            val symbol = if (key is Key.Letter) overrides[key.char] else null
            if (key is Key.Letter && !key.flickShift && symbol != null && valid(symbol)) {
                Key.Letter(key.char, symbol, key.initial, key.finals, key.weight, key.flickShift)
            } else key
        }
    }
}
