//! 上滑符号逐键编辑；留空恢复当前语言的默认符号。
package app.qingjian.inputmethod.settings.page

import android.app.AlertDialog
import android.text.InputType
import android.view.View
import android.widget.EditText
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.keyboard.layout.FlickSymbols
import app.qingjian.inputmethod.keyboard.layout.Key
import app.qingjian.inputmethod.keyboard.layout.KeyboardLayout

class FlickSymbolsActivity : PageActivity() {
    override val title = R.string.flick_symbols_customize

    override fun content(): List<View> {
        val defaults = KeyboardLayout.qwerty(chinese = true).flatten().filterIsInstance<Key.Letter>().associate { it.char to it.hint }
        return listOf(ui.group(null,
            FlickSymbols.LETTERS.map { char ->
                val saved = prefs.getString("${FlickSymbols.PREFIX}$char", null)
                val value = saved ?: getString(R.string.flick_symbol_default, defaults[char].orEmpty())
                ui.action(R.string.flick_symbol_edit, "${char.uppercaseChar()}  →  $value") { edit(char, saved.orEmpty()) }
            }, footer = getString(R.string.flick_symbols_customize_note)),
            ui.group(null, ui.action(R.string.flick_symbols_reset, null) {
                AlertDialog.Builder(this).setTitle(R.string.flick_symbols_reset)
                    .setMessage(R.string.flick_symbols_reset_note)
                    .setPositiveButton(android.R.string.ok) { _, _ ->
                        prefs.edit().apply { FlickSymbols.LETTERS.forEach { remove("${FlickSymbols.PREFIX}$it") } }.apply()
                        rebuild()
                    }.setNegativeButton(android.R.string.cancel, null).show()
            }))
    }

    private fun edit(char: Char, current: String) {
        val field = EditText(this).apply {
            inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_NO_SUGGESTIONS
            setSingleLine()
            setText(current)
            setSelection(current.length)
        }
        val dialog = AlertDialog.Builder(this)
            .setTitle(getString(R.string.flick_symbol_title, char.uppercaseChar().toString()))
            .setMessage(R.string.flick_symbol_edit_note).setView(field)
            .setPositiveButton(android.R.string.ok, null)
            .setNegativeButton(android.R.string.cancel, null).create()
        dialog.setOnShowListener {
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
                val value = field.text.toString().trim()
                if (value.isNotEmpty() && !FlickSymbols.valid(value)) {
                    field.error = getString(R.string.flick_symbol_invalid)
                } else {
                    prefs.edit().apply {
                        if (value.isEmpty()) remove("${FlickSymbols.PREFIX}$char")
                        else putString("${FlickSymbols.PREFIX}$char", value)
                    }.apply()
                    dialog.dismiss()
                    rebuild()
                }
            }
        }
        dialog.show()
    }
}
