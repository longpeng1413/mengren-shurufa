package app.qingjian.inputmethod.clipboard

/** 只记成功受理的复制时间戳，不持有秘密；未受理允许再次捕获，旧复制不会重新发送。 */
internal class PrivateCopyState(var accepted: Long) {
    fun shouldAttempt(timestamp: Long, now: Long): Boolean =
        timestamp != accepted && now - timestamp in 0L..120_000L
}
