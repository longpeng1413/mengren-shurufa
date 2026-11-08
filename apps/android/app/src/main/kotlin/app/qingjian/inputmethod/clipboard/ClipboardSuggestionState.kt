package app.qingjian.inputmethod.clipboard

/** 当前复制建议的生命周期；消费、关闭或超过复制时间五分钟后不再重复显示。 */
class ClipboardSuggestionState {
    data class Item(val token: String, val timestamp: Long, val text: String, val sensitive: Boolean, val code: Boolean)
    private var item: Item? = null
    private var dismissed: String? = null

    fun offer(value: Item?, now: Long): Item? {
        item = value?.takeIf { fresh(it.timestamp, now) && it.token != dismissed }
        return current(now)
    }

    fun current(now: Long): Item? {
        if (item?.let { fresh(it.timestamp, now) } != true) item = null
        return item
    }

    fun dismiss() { dismissed = item?.token ?: dismissed; item = null }
    fun hide() { item = null }

    companion object {
        const val LIFETIME = 300_000L
        fun fresh(timestamp: Long, now: Long) = timestamp > 0 && timestamp <= now && now - timestamp < LIFETIME
    }
}
