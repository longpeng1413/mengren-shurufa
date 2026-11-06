package app.qingjian.inputmethod.settings.page

import android.view.View
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.cloud.CloudConfig
import app.qingjian.inputmethod.cloud.SecretStore
import app.qingjian.inputmethod.cloud.MemberAccount
import app.qingjian.inputmethod.cloud.CloudServices
import android.content.Intent
import android.net.Uri
import android.app.AlertDialog
import app.qingjian.inputmethod.core.Native
import app.qingjian.inputmethod.settings.Settings
import kotlin.concurrent.thread
import org.json.JSONObject

/**
 * 云端大模型二级页：接口地址、模型、密钥（Keystore 加密存），测试连接与清除密钥；
 * 下面是用到它的几处（云联想、补充译词），没配好时只给一行说明。
 */
class CloudSettingsActivity : PageActivity() {
    override val title = R.string.section_cloud

    /** 上次「测试连接」的结果（给用户看的一句话）；测试中是「测试中…」。 */
    private var testResult: String? = null
    private var editingService = "legacy"

    override fun content(): List<View> {
        val cloud = CloudServices.load(this, editingService)
        return listOf(
            ui.group(null, memberRows()),
            ui.group(null, youdaoRows()),
            ui.group(getString(R.string.service_routes), routeRows()),
            ui.group(null, listOf(ui.picker(R.string.service_edit, CloudServices.names(this)[CloudServices.ids.indexOf(editingService)], CloudServices.names(this), CloudServices.ids.indexOf(editingService)) { editingService = CloudServices.ids[it]; testResult = null; rebuild() })),
            ui.group(null, listOf(ui.input(R.string.cloud_proxy, SecretStore.prefs(this).getString(CloudConfig.KEY_PROXY, "").orEmpty(), SecretStore.prefs(this).getString(CloudConfig.KEY_PROXY, "").orEmpty(), secret = false) {
                CloudConfig.set(this, CloudConfig.KEY_PROXY, it); rebuild()
            })),
            ui.group(getString(R.string.section_service), serviceRows(cloud), getString(R.string.cloud_note)),
            ui.group(null, actionRows(cloud)),
            ui.group(getString(R.string.section_cloud_uses), useRows(), getString(R.string.cloud_uses_note)),
        )
    }

    private fun serviceRows(cloud: CloudConfig): List<View> {
        return listOf(
        ui.input(R.string.cloud_proxy, SecretStore.prefs(this).getString(if (editingService == "legacy") CloudConfig.KEY_PROXY else "service_${editingService}_proxy", "").orEmpty(), SecretStore.prefs(this).getString(if (editingService == "legacy") CloudConfig.KEY_PROXY else "service_${editingService}_proxy", "").orEmpty(), secret = false) {
            CloudServices.set(this, editingService, "proxy", it); rebuild()
        },
        ui.input(R.string.cloud_base_url, cloud.baseUrl, cloud.baseUrl, secret = false) {
            CloudServices.set(this, editingService, CloudConfig.KEY_BASE_URL, it)
            testResult = null
            rebuild()
        },
        ui.input(R.string.cloud_model, cloud.model, cloud.model, secret = false) {
            CloudServices.set(this, editingService, CloudConfig.KEY_MODEL, it)
            testResult = null
            rebuild()
        },
        ui.input(R.string.cloud_key, cloud.keyHint ?: getString(R.string.cloud_key_empty), "", secret = true) {
            if (it.isNotEmpty()) CloudServices.saveKey(this, editingService, it)
            testResult = null
            rebuild()
        },
    )
    }

    private fun memberRows(): List<View> {
        val rows = mutableListOf<View>(ui.action(R.string.member_login, testResult) { memberLogin() })
            rows += ui.toggle(R.string.member_use, R.string.member_note, SecretStore.prefs(this).getBoolean(MemberAccount.ENABLED, false)) {
                SecretStore.prefs(this).edit().putBoolean(MemberAccount.ENABLED, it).apply()
                rebuild()
            }
        if (MemberAccount.load(this) != null) rows += ui.action(R.string.member_logout, null) { MemberAccount.clear(this); rebuild() }
        if (MemberAccount.load(this) != null) rows += ui.input(R.string.member_model, SecretStore.prefs(this).getString(MemberAccount.MODEL, "").orEmpty(), SecretStore.prefs(this).getString(MemberAccount.MODEL, "").orEmpty(), secret = false) {
            SecretStore.prefs(this).edit().putString(MemberAccount.MODEL, it.trim()).apply(); rebuild()
        }
        if (MemberAccount.load(this) != null) rows += ui.action(R.string.cloud_models, null) { chooseModels(CloudConfig.member(this), null) }
        return rows
    }

    private fun routeRows(): List<View> {
        val store = SecretStore.prefs(this)
        return listOf("predict" to R.string.route_predict, "question" to R.string.route_question, "translate" to R.string.route_translate, "gloss" to R.string.route_gloss).map { (purpose, title) ->
            val ids = CloudServices.ids + "chatgpt" + if (purpose == "translate") listOf("youdao") else emptyList()
            val names = CloudServices.names(this) + getString(R.string.member_service) + if (purpose == "translate") listOf(getString(R.string.youdao_service)) else emptyList()
            val fallback = when {
                purpose == "translate" && store.getBoolean("youdao_enabled", false) -> "youdao"
                store.getBoolean(MemberAccount.ENABLED, false) -> "chatgpt"
                else -> "legacy"
            }
            val index = ids.indexOf(store.getString("route_$purpose", fallback)).coerceAtLeast(0)
            ui.picker(title, names[index], names, index) {
                store.edit().putString("route_$purpose", ids[it]).apply()
                if (purpose == "translate") store.edit().putBoolean("youdao_enabled", ids[it] == "youdao").apply()
                rebuild()
            }
        }
    }

    private fun youdaoRows(): List<View> {
        val store = SecretStore.prefs(this)
        return listOf(
            ui.toggle(R.string.youdao_use, R.string.youdao_note, store.getBoolean("youdao_enabled", false)) { val edit = store.edit().putBoolean("youdao_enabled", it)
                if (it) edit.putString("route_translate", "youdao")
                else if (store.getString("route_translate", null) == "youdao") edit.remove("route_translate")
                edit.apply(); rebuild() },
            ui.input(R.string.youdao_app_key, store.getString("youdao_app_key", "").orEmpty(), store.getString("youdao_app_key", "").orEmpty(), secret = false) {
                store.edit().putString("youdao_app_key", it.trim()).apply()
                if (it.isNotBlank() && store.contains("youdao_secret_cipher") && !store.contains("route_translate")) store.edit().putBoolean("youdao_enabled", true).apply()
                testResult = null; rebuild()
            },
            ui.input(R.string.youdao_secret, if (store.contains("youdao_secret_cipher")) getString(R.string.youdao_secret_set) else getString(R.string.cloud_key_empty), "", secret = true) {
                if (it.isNotBlank()) {
                    store.edit().putString("youdao_secret_cipher", SecretStore.seal(it.trim())).apply()
                    if (!store.getString("youdao_app_key", "").isNullOrBlank() && !store.contains("route_translate")) store.edit().putBoolean("youdao_enabled", true).apply()
                }
                testResult = null
                rebuild()
            },
            ui.action(R.string.youdao_test, testResult) { testConnection(CloudConfig.youdao(this)) },
        )
    }

    private fun memberLogin() {
        if (testResult == getString(R.string.member_waiting)) return
        val start = runCatching { JSONObject(Native.memberStart(MemberAccount.host(this), SecretStore.prefs(this).getString(CloudConfig.KEY_PROXY, "").orEmpty()) ?: "{}") }.getOrNull()
        val url = start?.optString("url").orEmpty()
        if (url.isEmpty()) { testResult = getString(R.string.member_failed); rebuild(); return }
        testResult = getString(R.string.member_waiting)
        rebuild()
        runCatching { startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url))) }.onFailure {
            testResult = getString(R.string.member_failed); rebuild(); return
        }
        val app = applicationContext
        thread(name = "member-login") {
            val result = runCatching { JSONObject(Native.memberFinish() ?: "{}") }.getOrNull()
            val account = result?.optJSONObject("account")
            if (account != null) MemberAccount.save(app, account, result.optString("model"))
            runOnUiThread {
                if (!isDestroyed) { testResult = getString(if (account != null) R.string.member_ok else R.string.member_failed); rebuild() }
            }
        }
    }

    private fun actionRows(cloud: CloudConfig): List<View> {
        val rows = mutableListOf<View>(ui.action(R.string.cloud_test, testResult) { testConnection(cloud) })
        rows += ui.action(R.string.cloud_models, null) { chooseModels(cloud, editingService) }
        if (cloud.keyHint != null) {
            rows += ui.action(R.string.cloud_clear_key, null, color = ui.colors.danger) {
                CloudServices.clearKey(this, editingService)
                testResult = null
                rebuild()
            }
        }
        return rows
    }

    private fun chooseModels(cloud: CloudConfig, service: String?) {
        testResult = getString(R.string.cloud_testing); rebuild()
        val json = cloud.toJson()
        thread(name = "service-models") {
            val result = runCatching { JSONObject(Native.cloudModels(json) ?: "{}") }.getOrNull()
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                val array = result?.optJSONArray("models")
                val models = if (array != null) List(array.length()) { array.getString(it) } else emptyList()
                testResult = result?.optString("error")?.takeIf { it.isNotEmpty() }
                if (models.isEmpty() && testResult == null) testResult = getString(R.string.cloud_models_empty)
                if (models.isNotEmpty()) AlertDialog.Builder(this).setTitle(R.string.cloud_model).setItems(models.toTypedArray()) { _, index ->
                    if (service == null) SecretStore.prefs(this).edit().putString(MemberAccount.MODEL, models[index]).apply()
                    else CloudServices.set(this, service, CloudConfig.KEY_MODEL, models[index])
                    rebuild()
                }.show()
                rebuild()
            }
        }
    }

    /** 没配好云端时开关摆着也不生效，换成说明。 */
    private fun useRows(): List<View> {
        val settings = Settings.load(this)
        val rows = mutableListOf<View>(
            ui.toggle(R.string.cloud_predict_title, R.string.cloud_predict_note, settings.cloudPredict) {
                prefs.edit().putBoolean(Settings.KEY_CLOUD_PREDICT, it).apply()
                rebuild()
            },
        )
        if (settings.cloudPredict) {
            rows += ui.toggle(R.string.cloud_wifi_only_title, R.string.cloud_wifi_only_note, settings.cloudWifiOnly) {
                prefs.edit().putBoolean(Settings.KEY_CLOUD_WIFI_ONLY, it).apply()
            }
        }
        rows += ui.toggle(R.string.gloss_fill_title, R.string.gloss_fill_note, settings.glossFill) {
            prefs.edit().putBoolean(Settings.KEY_GLOSS_FILL, it).apply()
        }
        return rows
    }

    /** 测试连接要等网络，放后台线程；回来再重画这一页。 */
    private fun testConnection(cloud: CloudConfig) {
        if (testResult == getString(R.string.cloud_testing)) return
        if (!cloud.ready) {
            testResult = getString(R.string.translate_no_cloud)
            rebuild()
            return
        }
        testResult = getString(R.string.cloud_testing)
        rebuild()
        val settings = cloud.toJson()
        thread(name = "qingjian-cloud-test") {
            val outcome = runCatching { JSONObject(Native.cloudTest(settings) ?: "{}") }.getOrNull()
            val message = outcome?.optString("message")?.takeIf { it.isNotEmpty() } ?: getString(R.string.cloud_test_failed)
            runOnUiThread {
                if (isDestroyed) return@runOnUiThread
                testResult = message
                rebuild()
            }
        }
    }
}
