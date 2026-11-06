package app.qingjian.inputmethod.clipboard.sync

import java.lang.ref.WeakReference

/** 单次自动填入授权绑定具体输入连接，不保存文字；超时、目标失效或取消后不可再用。 */
internal class PrivateTarget<T : Any>(private val clock: () -> Long) {
    private var target = WeakReference<T>(null)
    private var expires = 0L

    val waiting: Boolean get() {
        if (clock() >= expires) clear()
        return target.get() != null
    }

    fun arm(value: T) { target = WeakReference(value); expires = clock() + 120_000L }
    fun matches(value: T?) = waiting && value != null && target.get() === value
    fun clear() { target.clear(); expires = 0L }
}
