package app.qingjian.inputmethod.cloud

import android.content.Context
import app.qingjian.inputmethod.core.Native
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit

/** ChatGPT 账户只存 Keystore 加密密文；续期在后台进行。 */
object MemberAccount {
    private const val ACCOUNT = "member_account_cipher"
    const val ENABLED = "member_enabled"
    const val MODEL = "member_model"
    private val executor = Executors.newSingleThreadScheduledExecutor { task -> Thread(task, "member-refresh").apply { isDaemon = true } }
    private var started = false

    fun load(context: Context): JSONObject? = runCatching {
        val sealed = SecretStore.prefs(context).getString(ACCOUNT, null) ?: return null
        JSONObject(SecretStore.open(sealed) ?: return null)
    }.getOrNull()

    @Synchronized fun save(context: Context, account: JSONObject, model: String) {
        SecretStore.prefs(context).edit().putString(ACCOUNT, SecretStore.seal(account.toString()))
            .putString(MODEL, model).putBoolean(ENABLED, true).apply()
    }

    @Synchronized fun clear(context: Context) {
        SecretStore.prefs(context).edit().remove(ACCOUNT).apply()
    }

    fun host(context: Context): String {
        val prefs = SecretStore.prefs(context)
        return prefs.getString("member_host_id", null) ?: "urn:uuid:${UUID.randomUUID()}".also {
            prefs.edit().putString("member_host_id", it).apply()
        }
    }

    @Synchronized fun start(context: Context) {
        if (started) return
        started = true
        val app = context.applicationContext
        executor.scheduleWithFixedDelay({
            runCatching {
                val current = load(app) ?: return@scheduleWithFixedDelay
                if (current.optLong("expires_at") > System.currentTimeMillis() / 1000 + 180) return@scheduleWithFixedDelay
                val result = JSONObject(Native.memberRefresh(current.toString(), SecretStore.prefs(app).getString(CloudConfig.KEY_PROXY, "").orEmpty()) ?: return@scheduleWithFixedDelay)
                if (result.has("error")) return@scheduleWithFixedDelay
                synchronized(this) {
                    if (load(app)?.optString("refresh_token") == current.optString("refresh_token")) {
                        SecretStore.prefs(app).edit().putString(ACCOUNT, SecretStore.seal(result.toString())).apply()
                    }
                }
            }
        }, 0, 60, TimeUnit.SECONDS)
    }
}
