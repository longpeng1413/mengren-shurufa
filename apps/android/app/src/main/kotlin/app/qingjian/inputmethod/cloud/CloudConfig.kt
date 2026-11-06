package app.qingjian.inputmethod.cloud

import android.content.Context
import org.json.JSONObject

/**
 * 云端大模型的配置（OpenAI 兼容接口，快捷翻译、释义兜底与云联想用）：地址与模型是明文设置，密钥在 [SecretStore]。
 * [toJson] 是交给 Rust 的那份（含解好的密钥），只在调 JNI 时临时拼，不留存。
 */
class CloudConfig(val baseUrl: String, val model: String, private val apiKey: String?, val subscription: Boolean = false, private val youdao: JSONObject = JSONObject(), private val proxy: String = "") {
    private var question: CloudConfig? = null

    /** 当前服务配好即可使用，不回退到其他服务。 */
    val ready: Boolean get() = (baseUrl.isNotBlank() && model.isNotBlank() && !apiKey.isNullOrBlank()) || question?.ready == true || (youdao.optBoolean("enabled") && youdao.optString("app_key").isNotBlank() && youdao.optString("secret").isNotBlank())

    /** 设置页显示用：只露末尾 4 位。 */
    val keyHint: String? get() = if (subscription) null else apiKey?.takeIf { it.length > 4 }?.let { "····${it.takeLast(4)}" }

    /** [predict] 时 Rust 侧连云联想与整句补全一起开，否则只做显式的翻译。 */
    fun toJson(predict: Boolean = false): String = JSONObject()
        .put("base_url", baseUrl)
        .put("model", model)
        .put("api_key", apiKey.orEmpty())
        .put("predict", predict && !apiKey.isNullOrBlank())
        .put("subscription", subscription)
        .put("proxy", proxy)
        .put("question", question?.let { JSONObject(it.toJson(predict)) })
        .put("youdao_enabled", youdao.optBoolean("enabled"))
        .put("youdao_app_key", youdao.optString("app_key"))
        .put("youdao_secret", youdao.optString("secret"))
        .toString()

    companion object {
        /** 与 PC 的 `PredictConfig` 缺省一致。 */
        const val DEFAULT_BASE_URL = "https://api.deepseek.com"
        const val DEFAULT_MODEL = "deepseek-flash"

        const val KEY_BASE_URL = "base_url"
        const val KEY_MODEL = "model"
        const val KEY_PROXY = "proxy"

        fun member(context: Context): CloudConfig {
            val prefs = SecretStore.prefs(context)
            val account = MemberAccount.load(context)
            val token = account?.takeIf { it.optLong("expires_at") > System.currentTimeMillis() / 1000 }?.optString("access_token")
            return CloudConfig("https://api.openai.com/v1", prefs.getString(MemberAccount.MODEL, "").orEmpty(), token, true, proxy = prefs.getString(KEY_PROXY, "").orEmpty())
        }

        fun youdao(context: Context): CloudConfig {
            val prefs = SecretStore.prefs(context)
            return CloudConfig("", "", null, youdao = JSONObject().put("enabled", true)
                .put("app_key", prefs.getString("youdao_app_key", ""))
                .put("secret", prefs.getString("youdao_secret_cipher", null)?.let(SecretStore::open).orEmpty()))
        }

        fun load(context: Context, purpose: String = "predict"): CloudConfig {
            val prefs = SecretStore.prefs(context)
            val youdao = JSONObject().put("enabled", prefs.getBoolean("youdao_enabled", false))
                .put("app_key", prefs.getString("youdao_app_key", ""))
                .put("secret", prefs.getString("youdao_secret_cipher", null)?.let { SecretStore.open(it) }.orEmpty())
            val proxy = prefs.getString(KEY_PROXY, "").orEmpty()
            val route = prefs.getString("route_$purpose", null) ?: when {
                purpose == "translate" && youdao.optBoolean("enabled") -> "youdao"
                prefs.getBoolean(MemberAccount.ENABLED, false) -> "chatgpt"
                else -> "legacy"
            }
            val config = when (route) {
                "youdao" -> CloudConfig("", "", null, youdao = youdao.put("enabled", true))
                "chatgpt" -> {
                    val account = MemberAccount.load(context)
                    val token = account?.takeIf { it.optLong("expires_at") > System.currentTimeMillis() / 1000 }?.optString("access_token")
                    CloudConfig("https://api.openai.com/v1", prefs.getString(MemberAccount.MODEL, "").orEmpty(), token, true, proxy = proxy)
                }
                else -> CloudServices.load(context, route)
            }
            if (purpose == "predict") config.question = load(context, "question")
            return config
        }

        fun set(context: Context, key: String, value: String) {
            SecretStore.prefs(context).edit().putString(key, value.trim()).apply()
        }
    }
}
