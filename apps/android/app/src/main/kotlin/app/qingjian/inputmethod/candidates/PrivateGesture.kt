package app.qingjian.inputmethod.candidates

/** 遮挡保护只限制这次私密输入手势，不影响正常候选、收起或关闭操作。 */
class PrivateGesture {
    var allowed = false
        private set

    fun observe(down: Boolean, obscured: Boolean, cancelled: Boolean) {
        if (down) allowed = true
        if (obscured || cancelled) allowed = false
    }
}
