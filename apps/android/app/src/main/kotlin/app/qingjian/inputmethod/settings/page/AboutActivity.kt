package app.qingjian.inputmethod.settings.page

import android.app.AlertDialog
import android.content.Intent
import android.content.SharedPreferences
import android.net.Uri
import android.os.Bundle
import android.provider.Settings as AndroidSettings
import app.qingjian.inputmethod.update.Updates
import app.qingjian.inputmethod.update.UpdateInstaller
import java.lang.ref.WeakReference
import android.view.Gravity
import android.view.View
import android.widget.ImageView
import android.widget.LinearLayout
import app.qingjian.inputmethod.R

/** 关于：品牌标、名字、版本号，以及「全部在本机」的说明与许可。 */
class AboutActivity : PageActivity() {
    override val title = R.string.page_about
    private var resumed = false
    private var permissionPending = false
    private val updateListener = SharedPreferences.OnSharedPreferenceChangeListener { _, _ ->
        runOnUiThread { if (resumed && !isDestroyed) { rebuild(); maybeInstall() } }
    }
    override fun onCreate(savedInstanceState: Bundle?) { super.onCreate(savedInstanceState); Updates.prefs(this).registerOnSharedPreferenceChangeListener(updateListener) }
    override fun onResume() {
        super.onResume(); resumed = true; UpdateInstaller.foreground = WeakReference(this)
        if (permissionPending) {
            permissionPending = false
            if (!packageManager.canRequestPackageInstalls()) { Updates.prefs(this).edit().putBoolean("install_requested", false).apply(); Updates.fail(this, "permission") }
        }
        rebuild(); maybeInstall()
    }
    override fun onPause() { resumed = false; UpdateInstaller.foreground = null; super.onPause() }
    override fun onDestroy() { Updates.prefs(this).unregisterOnSharedPreferenceChangeListener(updateListener); super.onDestroy() }
    private fun maybeInstall() {
        if (!resumed) return
        UpdateInstaller.confirmation?.let { UpdateInstaller.confirmation = null; startActivity(it); return }
        val store = Updates.prefs(this)
        if (store.getString("status", "") != "ready" || !store.getBoolean("install_requested", false)) return
        if (!packageManager.canRequestPackageInstalls()) {
            if (permissionPending) return
            permissionPending = true
            AlertDialog.Builder(this).setTitle(R.string.update_permission_title).setMessage(R.string.update_permission_note)
                .setPositiveButton(R.string.update_permission_open) { _, _ -> startActivity(Intent(AndroidSettings.ACTION_MANAGE_UNKNOWN_APP_SOURCES, Uri.parse("package:$packageName"))) }
                .setNegativeButton(android.R.string.cancel) { _, _ -> permissionPending = false; store.edit().putBoolean("install_requested", false).apply() }
                .setOnCancelListener { permissionPending = false; store.edit().putBoolean("install_requested", false).apply() }.show()
            return
        }
        store.edit().putBoolean("install_requested", false).apply()
        UpdateInstaller.install(this)
    }
    private fun updateRows(): List<View> {
        val store = Updates.prefs(this)
        val state = store.getString("status", "idle").orEmpty()
        val statusId = when (state) {
            "checking" -> R.string.update_checking
            "latest" -> R.string.update_latest
            "available" -> R.string.update_available
            "ready" -> R.string.update_ready
            "installing" -> R.string.update_installing
            "confirmation" -> R.string.update_confirmation
            "installed" -> R.string.update_installed
            "error_integrity" -> R.string.update_integrity
            "error_signature" -> R.string.update_signature
            "error_cancelled" -> R.string.update_cancelled
            "error_release_changed" -> R.string.update_changed
            "error_permission" -> R.string.update_permission_note
            "error_package" -> R.string.update_package_error
            else -> if (state.startsWith("error_")) R.string.update_network_error else R.string.update_idle
        }
        val status = if (state == "downloading") getString(R.string.update_progress, store.getInt("percent", 0)) else getString(statusId)
        val rows = mutableListOf<View>(
            ui.toggle(R.string.update_auto, R.string.update_auto_note, store.getBoolean("auto", true)) { store.edit().putBoolean("auto", it).apply(); if (it) Updates.check(this) },
            ui.action(R.string.update_check, status) { Updates.check(this) },
        )
        val offer = Updates.offer(this)
        if (offer != null) {
            val version = offer.optString("version")
            val size = offer.optJSONObject("asset")?.optLong("size") ?: 0
            val notes = offer.optJSONArray("notes")?.let { array -> List(array.length()) { array.optString(it) }.joinToString("\n") }.orEmpty()
            rows += ui.action(R.string.update_install, getString(R.string.update_offer, version, size / 1024 / 1024)) {
                if (state == "downloading") return@action
                if (Updates.pending(this) != null) { store.edit().putBoolean("install_requested", true).putString("status", "ready").apply(); maybeInstall() }
                else Updates.download(this, version)
            }
            if (notes.isNotBlank()) rows += ui.text(notes, 13f, ui.colors.secondary)
        }
        if (state == "downloading") rows += ui.action(R.string.update_cancel, null) { Updates.cancel(this) }
        rows += ui.input(R.string.update_proxy, store.getString("proxy", "").orEmpty(), store.getString("proxy", "").orEmpty(), secret = false) { store.edit().putString("proxy", it.trim()).apply() }
        return rows
    }

    override fun content(): List<View> {
        val version = packageManager.getPackageInfo(packageName, 0).versionName.orEmpty()
        return listOf(
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                gravity = Gravity.CENTER_HORIZONTAL
                setPadding(0, ui.dp(28), 0, ui.dp(28))
                addView(ImageView(context).apply { setImageResource(R.drawable.ic_brand) }, LinearLayout.LayoutParams(ui.dp(72), ui.dp(72)))
                addView(ui.text(getString(R.string.app_name), 22f, ui.colors.text, bold = true).apply {
                    gravity = Gravity.CENTER
                    setPadding(0, ui.dp(14), 0, 0)
                })
                addView(ui.text(getString(R.string.about_version, version), 13f, ui.colors.secondary).apply {
                    gravity = Gravity.CENTER
                    setPadding(0, ui.dp(4), 0, 0)
                })
            },
            ui.group(getString(R.string.update_section), updateRows(), getString(R.string.update_network_note)),
            ui.group(
                null,
                ui.row(R.string.about_local_title, R.string.about_local_note),
                ui.row(R.string.about_license_title, R.string.about_license_note),
            ),
        )
    }
}
