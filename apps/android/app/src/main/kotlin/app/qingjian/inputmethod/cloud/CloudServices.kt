package app.qingjian.inputmethod.cloud

import android.content.Context
import app.qingjian.inputmethod.R

/** 各服务独立保存连接参数与加密密钥，旧版设置保留在 legacy 槽位。 */
object CloudServices {
    val ids = listOf("legacy", "deepseek", "openai", "ark", "custom")
    fun names(context: Context) = listOf(context.getString(R.string.service_legacy), "DeepSeek", "OpenAI API", context.getString(R.string.service_ark), context.getString(R.string.service_custom))

    fun load(context: Context, id: String): CloudConfig {
        val prefs = SecretStore.prefs(context)
        if (id == "legacy") return CloudConfig(
            prefs.getString(CloudConfig.KEY_BASE_URL, CloudConfig.DEFAULT_BASE_URL).orEmpty(),
            prefs.getString(CloudConfig.KEY_MODEL, CloudConfig.DEFAULT_MODEL).orEmpty(), SecretStore.load(context),
            proxy = prefs.getString(CloudConfig.KEY_PROXY, "").orEmpty(),
        )
        val base = when (id) {
            "deepseek" -> "https://api.deepseek.com"
            "openai" -> "https://api.openai.com/v1"
            "ark" -> "https://ark.cn-beijing.volces.com/api/v3"
            else -> ""
        }
        return CloudConfig(prefs.getString("service_${id}_base_url", base).orEmpty(),
            prefs.getString("service_${id}_model", "").orEmpty(),
            prefs.getString("service_${id}_secret", null)?.let(SecretStore::open),
            proxy = prefs.getString("service_${id}_proxy", "").orEmpty())
    }

    fun set(context: Context, id: String, field: String, value: String) {
        if (id == "legacy") { CloudConfig.set(context, field, value); return }
        SecretStore.prefs(context).edit().putString("service_${id}_$field", value.trim()).apply()
    }

    fun saveKey(context: Context, id: String, value: String) {
        if (id == "legacy") SecretStore.save(context, value)
        else SecretStore.prefs(context).edit().putString("service_${id}_secret", SecretStore.seal(value.trim())).apply()
    }

    fun clearKey(context: Context, id: String) {
        if (id == "legacy") SecretStore.clear(context)
        else SecretStore.prefs(context).edit().remove("service_${id}_secret").apply()
    }
}
