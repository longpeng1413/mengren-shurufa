package app.qingjian.inputmethod.voice

import android.content.Context
import app.qingjian.inputmethod.cloud.SecretStore
import org.json.JSONObject
import java.io.File

/**
 * 「语音识别」设置页的配置，与云端大模型（翻译、联想）那份分开存（`voice` 那份 prefs）：识别服务、计费方式、密钥。
 * 密钥与云端那份一样经 [SecretStore] 加密，只存密文；[toJson] 是交给 Rust 的那份（含解好的密钥），只在调 JNI 时临时拼。
 * 本机离线识别不要密钥，交给 Rust 的是模型目录与原生库目录（模型下没下好由 Rust 判断）。
 */
class VoiceConfig(
    val provider: String,
    val resourceId: String,
    private val apiKey: String?,
    private val modelRoot: String,
    private val libraryDir: String,
) {
    /** 豆包选了服务且填了密钥、离线选上了，才算配好。 */
    val ready get() = provider == PROVIDER_OFFLINE || (provider == PROVIDER_VOLCENGINE && !apiKey.isNullOrBlank())

    /** 设置页显示用：只露末尾 4 位。 */
    val keyHint: String? get() = apiKey?.takeIf { it.length > 4 }?.let { "····${it.takeLast(4)}" }

    fun toJson(): String = JSONObject()
        .put("provider", provider)
        .put("api_key", if (provider == PROVIDER_VOLCENGINE) apiKey.orEmpty() else "")
        .put("resource_id", resourceId)
        .put("model_root", modelRoot)
        .put("library_dir", libraryDir)
        .toString()

    companion object {
        const val PROVIDER_NONE = "none"
        const val PROVIDER_VOLCENGINE = "volcengine"
        const val PROVIDER_OFFLINE = "offline"

        /** 豆包流式语音识别模型 2.0 的两种计费方式（与 Rust 侧 `VolcengineConfig` 的常数一致）。 */
        const val RESOURCE_HOURLY = "volc.seedasr.sauc.duration"
        const val RESOURCE_CONCURRENT = "volc.seedasr.sauc.concurrent"
        const val RESOURCE_FILE2 = "volc.seedasr.auc"

        const val KEY_PROVIDER = "provider"
        const val KEY_RESOURCE = "resource_id"
        private const val KEY_CIPHER = "api_key_cipher"

        /** 离线模型的根目录（App 私有，卸载时随 App 删除）；模型按版本放在它下面的文件夹里。 */
        fun modelRoot(context: Context) = File(context.filesDir, "voice-model").absolutePath

        fun load(context: Context): VoiceConfig {
            val prefs = prefs(context)
            return VoiceConfig(
                prefs.getString(KEY_PROVIDER, null) ?: PROVIDER_NONE,
                prefs.getString(KEY_RESOURCE, null) ?: RESOURCE_HOURLY,
                prefs.getString(KEY_CIPHER, null)?.let(SecretStore::open),
                modelRoot(context),
                context.applicationInfo.nativeLibraryDir.orEmpty(),
            )
        }

        fun set(context: Context, key: String, value: String) {
            prefs(context).edit().putString(key, value).apply()
        }

        fun saveKey(context: Context, secret: String) {
            prefs(context).edit().putString(KEY_CIPHER, SecretStore.seal(secret.trim())).apply()
        }

        fun clearKey(context: Context) {
            prefs(context).edit().remove(KEY_CIPHER).apply()
        }

        fun prefs(context: Context) = context.getSharedPreferences("voice", Context.MODE_PRIVATE)
    }
}
