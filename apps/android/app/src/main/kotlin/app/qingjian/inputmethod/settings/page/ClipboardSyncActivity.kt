package app.qingjian.inputmethod.settings.page

import android.app.AlertDialog
import android.os.Bundle
import android.text.InputType
import android.view.View
import android.widget.EditText
import app.qingjian.inputmethod.R
import app.qingjian.inputmethod.clipboard.sync.ClipboardSync
import app.qingjian.inputmethod.core.Native
import org.json.JSONObject
import app.qingjian.inputmethod.cloud.SecretStore

/** 局域网同步页：总开关、设备名、已配对和附近的设备、配对。服务只在开关打开时运行。 */
class ClipboardSyncActivity : PageActivity(), ClipboardSync.Listener {
    override val title = R.string.page_clipboard_sync

    private var showingCode: String? = null
    private var status: String? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        ClipboardSync.apply(this)
        super.onCreate(savedInstanceState)
    }

    override fun onStart() {
        super.onStart()
        ClipboardSync.addListener(this)
        ClipboardSync.apply(this)
        runCatching { Native.clipboardSyncWatch(true) }
        rebuild()
    }

    override fun onStop() {
        ClipboardSync.removeListener(this)
        runCatching { Native.clipboardSyncWatch(false) }
        if (showingCode != null) {
            runCatching { Native.clipboardSyncCancelPairing() }
            showingCode = null
        }
        super.onStop()
    }

    override fun onSyncEvent(type: String, event: JSONObject) {
        when (type) {
            "nearby", "private_started", "private_delivery" -> rebuild()
            "paired" -> {
                showingCode = null
                status = getString(R.string.sync_paired_ok, event.optString("name"))
                rebuild()
            }
            "pair_failed" -> {
                showingCode = null
                status = getString(R.string.sync_pair_failed)
                rebuild()
            }
        }
    }

    override fun content(): List<View> {
        val on = ClipboardSync.enabled(this)
        val views = mutableListOf<View>(
            ui.group(
                null,
                ui.toggle(R.string.sync_enabled_title, R.string.sync_enabled_note, on) {
                    prefs.edit().putBoolean(ClipboardSync.KEY_ENABLED, it).apply()
                    ClipboardSync.apply(this)
                    if (it) runCatching { Native.clipboardSyncWatch(true) }
                    rebuild()
                },
                ui.toggle(R.string.sync_neon_title, R.string.sync_neon_note, ClipboardSync.neonEnabled(this)) {
                    prefs.edit().putBoolean(ClipboardSync.KEY_NEON, it).apply()
                },
            ),
        )
        if (!on) return views
        views += ui.group(null, ui.toggle(R.string.private_receive_enabled, R.string.private_receive_note,
            app.qingjian.inputmethod.clipboard.sync.PrivateReceive.enabled(this)) {
            prefs.edit().putBoolean(app.qingjian.inputmethod.clipboard.sync.PrivateReceive.KEY_ENABLED, it).apply()
            app.qingjian.inputmethod.clipboard.sync.PrivateReceive.update(this)
        })
        val privateModes = arrayOf(getString(R.string.private_auto), getString(R.string.private_manual), getString(R.string.private_off))
        views += ui.group(null, ui.toggle(R.string.private_auto_fill, R.string.private_auto_fill_note,
            app.qingjian.inputmethod.clipboard.sync.PrivateReceive.autoFillEnabled(this)) {
            prefs.edit().putBoolean(app.qingjian.inputmethod.clipboard.sync.PrivateReceive.KEY_AUTO_FILL, it).apply()
        })
        val privateIndex = app.qingjian.inputmethod.clipboard.sync.PrivateSend.MODES.indexOf(app.qingjian.inputmethod.clipboard.sync.PrivateSend.mode(this)).coerceAtLeast(0)
        views += ui.group(null,
            ui.row(getString(R.string.private_title), privateModes[privateIndex]).apply {
                setOnClickListener { AlertDialog.Builder(this@ClipboardSyncActivity).setTitle(R.string.private_title)
                    .setSingleChoiceItems(privateModes, privateIndex) { dialog, index ->
                        prefs.edit().putString(app.qingjian.inputmethod.clipboard.sync.PrivateSend.KEY_MODE, app.qingjian.inputmethod.clipboard.sync.PrivateSend.MODES[index]).apply()
                        dialog.dismiss(); rebuild()
                    }.setNegativeButton(android.R.string.cancel, null).show() }
            },
            ui.row(getString(R.string.private_send), getString(R.string.private_note)).apply {
                setOnClickListener { app.qingjian.inputmethod.clipboard.sync.PrivateSend.manual(this@ClipboardSyncActivity) }
            })
        // 配对结果放在最上面：放到页面最后要滚动才看得见，以为没配上
        status?.let { views += ui.footnote(it) }
        val name = ClipboardSync.deviceName(this)
        views += ui.group(
            R.string.sync_this_device,
            ui.input(R.string.sync_device_name, name, prefs.getString(ClipboardSync.KEY_DEVICE_NAME, "").orEmpty(), false) {
                prefs.edit().putString(ClipboardSync.KEY_DEVICE_NAME, it.trim()).apply()
                ClipboardSync.restart(this)
                runCatching { Native.clipboardSyncWatch(true) }
                rebuild()
            },
        )
        val devices = runCatching { Native.clipboardSyncDevices() }.getOrNull()?.let(::JSONObject)
        views += relaySettings(devices)
        val peers = devices?.optJSONArray("peers")
        val nearby = devices?.optJSONArray("nearby")
        val paired = HashSet<String>()
        if (peers != null && peers.length() > 0) {
            val rows = mutableListOf<View>()
            for (i in 0 until peers.length()) {
                val peer = peers.getJSONObject(i)
                val id = peer.getString("id")
                paired += id
                val delivery = ClipboardSync.privateDelivery(id)?.let {
                    getString(if (it) R.string.private_delivery_received else R.string.private_delivery_unconfirmed)
                }
                val state = listOfNotNull(peerState(peer), delivery).joinToString(" · ")
                rows += ui.row(peer.getString("name"), state).apply { setOnClickListener { confirmUnpair(id, peer.getString("name")) } }
            }
            views += ui.group(getString(R.string.sync_paired), rows)
        }
        val rows = mutableListOf<View>()
        if (nearby != null) {
            for (i in 0 until nearby.length()) {
                val device = nearby.getJSONObject(i)
                if (device.getString("id") in paired) continue
                val address = device.getString("address")
                rows += ui.row(device.getString("name"), device.optString("model")).apply {
                    setOnClickListener { askCode(address) }
                }
            }
        }
        views += ui.group(getString(R.string.sync_nearby), rows, if (rows.isEmpty()) getString(R.string.sync_none_nearby) else null)
        val relays = devices?.optJSONArray("relay_peers")
        if (relays != null && prefs.getBoolean(ClipboardSync.KEY_RELAY_ENABLED, false)) {
            val remote = mutableListOf<View>()
            for (i in 0 until relays.length()) {
                val id = relays.getString(i)
                if (id in paired) continue
                remote += ui.row(getString(R.string.sync_relay_device, id.takeLast(8)), null).apply { setOnClickListener { askCode("relay:$id") } }
            }
            views += ui.group(getString(R.string.sync_relay_devices), remote)
        }

        val code = showingCode
        views += ui.group(
            getString(R.string.sync_pairing),
            listOf(
                ui.action(R.string.sync_show_code, code) {
                    showingCode = runCatching { Native.clipboardSyncBeginPairing() }.getOrNull()
                    status = null
                    rebuild()
                },
                ui.action(R.string.sync_enter_address, null) { askAddress() },
            ),
            if (code != null) getString(R.string.sync_code_hint) else null,
        )
        return views
    }

    private fun relaySettings(devices: JSONObject?): View {
        val tokenSaved = prefs.contains(ClipboardSync.KEY_RELAY_TOKEN)
        return ui.group(R.string.sync_relay_title,
            ui.toggle(R.string.sync_relay_enabled, R.string.sync_relay_note, prefs.getBoolean(ClipboardSync.KEY_RELAY_ENABLED, false)) {
                prefs.edit().putBoolean(ClipboardSync.KEY_RELAY_ENABLED, it).apply()
                ClipboardSync.restart(this); rebuild()
            },
            ui.input(R.string.sync_relay_url, getString(R.string.sync_relay_url_hint), prefs.getString(ClipboardSync.KEY_RELAY_URL, "").orEmpty(), false) {
                prefs.edit().putString(ClipboardSync.KEY_RELAY_URL, it.trim()).apply()
                ClipboardSync.restart(this); rebuild()
            },
            ui.input(R.string.sync_relay_token, getString(if (tokenSaved) R.string.sync_relay_keep else R.string.sync_relay_token_hint), "", true) {
                val token = it.trim()
                if (token.isNotEmpty()) {
                    if (token.length !in 24..256 || token.any { char -> char.code !in 33..126 }) { status = getString(R.string.sync_relay_bad_token) }
                    else runCatching { SecretStore.seal(token) }.onSuccess { sealed ->
                        prefs.edit().putString(ClipboardSync.KEY_RELAY_TOKEN, sealed).apply()
                        ClipboardSync.restart(this)
                    }.onFailure { status = getString(R.string.sync_relay_bad_token) }
                }
                rebuild()
            },
            ui.action(R.string.sync_relay_clear_token, null) {
                prefs.edit().remove(ClipboardSync.KEY_RELAY_TOKEN).apply()
                ClipboardSync.restart(this); rebuild()
            },
            ui.row(getString(R.string.sync_relay_status), getString(if (devices?.optBoolean("relay_connected") == true) R.string.sync_relay_connected else R.string.sync_relay_disconnected)),
        )
    }

    private fun peerState(peer: JSONObject): String = when (peer.optString("status")) {
        "connected" -> getString(R.string.sync_connected) + if (peer.optBoolean("relay")) " · " + getString(R.string.sync_relay_title) else ""
        "unreachable" -> {
            val seen = peer.optLong("last_seen", 0L)
            if (seen > 0L) {
                val ago = if (System.currentTimeMillis() - seen < 60_000L) {
                    getString(R.string.sync_just_now)
                } else {
                    android.text.format.DateUtils.getRelativeTimeSpanString(seen)
                }
                getString(R.string.sync_unreachable_since, ago)
            } else {
                getString(R.string.sync_unreachable)
            }
        }
        else -> getString(R.string.sync_connecting)
    }

    private fun field(hint: Int, type: Int, value: String = "") = EditText(this).apply {
        setHint(hint)
        inputType = type
        setText(value)
        setSingleLine()
    }

    private fun askAddress() {
        val input = field(R.string.sync_address, InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI)
        AlertDialog.Builder(this)
            .setTitle(R.string.sync_enter_address)
            .setView(input)
            .setPositiveButton(android.R.string.ok) { _, _ -> askCode(input.text.toString()) }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun askCode(address: String) {
        val input = field(R.string.sync_code, InputType.TYPE_CLASS_NUMBER)
        AlertDialog.Builder(this)
            .setTitle(R.string.sync_pair)
            .setView(input)
            .setPositiveButton(android.R.string.ok) { _, _ ->
                val accepted = runCatching { Native.clipboardSyncJoin(address, input.text.toString()) }.getOrDefault(false)
                status = if (accepted) null else getString(R.string.sync_bad_address)
                rebuild()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun confirmUnpair(id: String, name: String) {
        AlertDialog.Builder(this)
            .setTitle(getString(R.string.sync_unpair_title, name))
            .setPositiveButton(R.string.sync_unpair) { _, _ ->
                runCatching { Native.clipboardSyncUnpair(id) }
                rebuild()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }
}
