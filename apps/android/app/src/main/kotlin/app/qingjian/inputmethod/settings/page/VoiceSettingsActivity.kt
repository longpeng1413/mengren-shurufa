package app.qingjian.inputmethod.settings.page

import android.os.Handler
import android.os.Looper
import android.view.View
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.voice.VoiceConfig
import app.qingjian.inputmethod.voice.VoiceModel
import kotlin.concurrent.thread
import org.json.JSONObject

/**
 * 语音识别二级页：识别服务（豆包 / 本机离线）。豆包是计费方式、API Key（Keystore 加密存）、测试连接与清除密钥；
 * 本机离线是模型状态、下载 / 取消、测试识别与删除模型。与键盘设置里的「云端大模型」分开存。
 */
class VoiceSettingsActivity : PageActivity() {
    override val title = R.string.page_voice

    /** 上次「测试连接 / 测试识别」的结果；测试中是「测试中…」。 */
    private var testResult: String? = null

    private val main = Handler(Looper.getMainLooper())

    /** 下载中每半秒刷新进度；不在下载就停。 */
    private val refresh = object : Runnable {
        override fun run() {
            if (isDestroyed) return
            rebuild()
            if (VoiceModel.status(this@VoiceSettingsActivity).downloading) main.postDelayed(this, 500)
        }
    }

    override fun onResume() {
        super.onResume()
        main.removeCallbacks(refresh)
        main.post(refresh)
    }

    override fun onPause() {
        super.onPause()
        main.removeCallbacks(refresh)
    }

    override fun content(): List<View> {
        val config = VoiceConfig.load(this)
        val providers = listOf(
            VoiceConfig.PROVIDER_NONE to getString(R.string.voice_provider_none),
            VoiceConfig.PROVIDER_OFFLINE to getString(R.string.voice_provider_offline),
            VoiceConfig.PROVIDER_VOLCENGINE to getString(R.string.voice_provider_volcengine),
        )
        val provider = providers.indexOfFirst { it.first == config.provider }.coerceAtLeast(0)
        val rows = mutableListOf<View>(
            ui.picker(R.string.voice_provider, providers[provider].second, providers.map { it.second }, provider) {
                VoiceConfig.set(this, VoiceConfig.KEY_PROVIDER, providers[it].first)
                testResult = null
                rebuild()
            },
        )
        val note = when (config.provider) {
            VoiceConfig.PROVIDER_VOLCENGINE -> {
                rows += volcengineRows(config)
                R.string.voice_note
            }
            VoiceConfig.PROVIDER_OFFLINE -> {
                rows += offlineRows(config)
                R.string.voice_offline_note
            }
            else -> R.string.voice_none_note
        }
        return listOf(ui.group(null, rows, getString(note)))
    }

    private fun volcengineRows(config: VoiceConfig): List<View> {
        val resources = listOf(
            VoiceConfig.RESOURCE_HOURLY to getString(R.string.voice_model_stream2),
            VoiceConfig.RESOURCE_FILE2 to getString(R.string.voice_model_file2),
        )
        val resource = resources.indexOfFirst { it.first == config.resourceId }.coerceAtLeast(0)
        val rows = mutableListOf<View>(
            ui.picker(R.string.voice_resource, resources[resource].second, resources.map { it.second }, resource) {
                VoiceConfig.set(this, VoiceConfig.KEY_RESOURCE, resources[it].first)
                testResult = null
                rebuild()
            },
            ui.input(R.string.voice_key, config.keyHint ?: getString(R.string.cloud_key_empty), "", secret = true) {
                if (it.isNotEmpty()) VoiceConfig.saveKey(this, it)
                testResult = null
                rebuild()
            },
            ui.action(R.string.cloud_test, testResult) { test(config) },
        )
        if (config.keyHint != null) {
            rows += ui.action(R.string.cloud_clear_key, null, ui.colors.danger) {
                VoiceConfig.clearKey(this)
                testResult = null
                rebuild()
            }
        }
        return rows
    }

    private fun offlineRows(config: VoiceConfig): List<View> {
        val status = VoiceModel.status(this)
        val state = when {
            status.downloading -> getString(
                R.string.voice_offline_downloading,
                percent(status.received, status.total), megabytes(status.received), megabytes(status.total),
            )
            status.installed -> getString(R.string.voice_offline_installed, megabytes(status.bytes))
            status.bytes > 0 -> getString(R.string.voice_offline_partial, megabytes(status.bytes), megabytes(status.total))
            else -> getString(R.string.voice_offline_missing, megabytes(status.total))
        }
        val note = listOfNotNull(VoiceModel.lastError, state).joinToString("\n")
        val rows = mutableListOf<View>(ui.row(getString(R.string.voice_offline_model), note))
        when {
            status.downloading -> rows += ui.action(R.string.voice_offline_cancel, null, ui.colors.danger) {
                VoiceModel.cancel()
                rebuild()
            }
            !status.installed -> rows += ui.action(R.string.voice_offline_download, null) {
                VoiceModel.download(this) { runOnUiThread { if (!isDestroyed) rebuild() } }
                main.removeCallbacks(refresh)
                main.postDelayed(refresh, 300)
            }
            else -> rows += ui.action(R.string.voice_offline_test, testResult) { test(config) }
        }
        if (!status.downloading && status.bytes > 0) {
            rows += ui.action(R.string.voice_offline_delete, null, ui.colors.danger) {
                VoiceModel.delete(this)?.let { toast(it) }
                testResult = null
                rebuild()
            }
        }
        return rows
    }

    private fun toast(message: String) = android.widget.Toast.makeText(this, message, android.widget.Toast.LENGTH_SHORT).show()

    /** 测试要等网络或加载模型，放后台线程；回来再重画这一页。 */
    private fun test(config: VoiceConfig) {
        if (testResult == getString(R.string.cloud_testing)) return
        if (!config.ready) {
            testResult = getString(R.string.voice_unavailable)
            rebuild()
            return
        }
        testResult = getString(R.string.cloud_testing)
        rebuild()
        val settings = config.toJson()
        thread(name = "qingjian-voice-test") {
            val outcome = runCatching { JSONObject(Native.voiceTest(settings) ?: "{}") }.getOrNull()
            val message = outcome?.optString("message")?.takeIf { it.isNotEmpty() } ?: "测试失败"
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                testResult = message
                rebuild()
            }
        }
    }

    private fun megabytes(bytes: Long) = "${(bytes + 524_288) / 1_048_576} MB"

    private fun percent(done: Long, total: Long) = if (total <= 0) 0 else (done * 100 / total).toInt()
}
