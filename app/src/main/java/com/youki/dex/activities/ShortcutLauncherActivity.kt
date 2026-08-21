package com.youki.dex.activities

import android.app.Activity
import android.app.AlertDialog
import android.content.ComponentName
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.provider.Settings
import androidx.preference.PreferenceManager
import com.youki.dex.R
import com.youki.dex.services.DOCK_SERVICE_ACTION
import com.youki.dex.services.NotificationService
import com.youki.dex.utils.DeviceUtils

/**
 * Trampoline activity for the home-screen shortcut.
 * Replicates DockTileService.onClick() logic exactly — copy/paste.
 */
class ShortcutLauncherActivity : Activity() {

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val prefs = PreferenceManager.getDefaultSharedPreferences(this)
        val isAccessibilityOn = DeviceUtils.isAccessibilityServiceEnabled(this)
        val isNotificationOn  = isNotificationListenerEnabled()
        val dexActive         = prefs.getBoolean("dex_mode_active", false)

        if (!dexActive) {
            // ── the exact same logic as DockTileService.onClick() ──

            if (!isNotificationOn) {
                AlertDialog.Builder(this)
                    .setTitle(getString(R.string.notification_access_required))
                    .setMessage(getString(R.string.notification_access_message))
                    .setPositiveButton(getString(R.string.ok)) { _, _ ->
                        startActivity(
                            Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS)
                                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                        )
                        finish()
                    }
                    .setNegativeButton(getString(R.string.skip)) { _, _ -> finish() }
                    .setOnCancelListener { finish() }
                    .show()
                return
            }

            if (!isAccessibilityOn) {
                val hasPermission = DeviceUtils.hasWriteSettingsPermission(this)
                if (hasPermission) {
                    DeviceUtils.enableService(this)
                    Handler(Looper.getMainLooper()).postDelayed({
                        if (DeviceUtils.isAccessibilityServiceEnabled(this)) {
                            startActivity(
                                Intent(this, LauncherActivity::class.java)
                                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                            )
                            finish()
                        } else {
                            AlertDialog.Builder(this)
                                .setTitle(getString(R.string.accessibility_service_required))
                                .setMessage(getString(R.string.accessibility_service_auto_failed_message))
                                .setPositiveButton(getString(R.string.ok)) { _, _ ->
                                    startActivity(
                                        Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)
                                            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                                    )
                                    finish()
                                }
                                .setNegativeButton(getString(R.string.cancel)) { _, _ -> finish() }
                                .setOnCancelListener { finish() }
                                .show()
                        }
                    }, 800)
                } else {
                    AlertDialog.Builder(this)
                        .setTitle(getString(R.string.accessibility_service_required))
                        .setMessage(getString(R.string.accessibility_service_message))
                        .setPositiveButton(getString(R.string.ok)) { _, _ ->
                            startActivity(
                                Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)
                                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
                            )
                            finish()
                        }
                        .setNegativeButton(getString(R.string.cancel)) { _, _ -> finish() }
                        .setOnCancelListener { finish() }
                        .show()
                }
            } else {
                // All permissions are present — launch directly
                startActivity(
                    Intent(this, LauncherActivity::class.java)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                )
                finish()
            }

        } else {
            // DEX is running — stop it (same logic as disable_self in DockTileService)
            sendBroadcast(
                Intent(DOCK_SERVICE_ACTION)
                    .setPackage(packageName)
                    .putExtra("action", "disable_self")
            )
            val homeIntent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME)
            val launchers  = packageManager.queryIntentActivities(homeIntent, 0)
                .filter { it.activityInfo.packageName != packageName }
            val target = launchers.firstOrNull()
            if (target != null) {
                startActivity(
                    Intent(Intent.ACTION_MAIN)
                        .addCategory(Intent.CATEGORY_HOME)
                        .setPackage(target.activityInfo.packageName)
                        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                )
            }
            finish()
        }
    }

    private fun isNotificationListenerEnabled(): Boolean {
        val flat = Settings.Secure.getString(
            contentResolver,
            "enabled_notification_listeners"
        ) ?: return false
        val cn = ComponentName(this, NotificationService::class.java)
        return flat.split(":").any {
            try { ComponentName.unflattenFromString(it) == cn } catch (e: Exception) { false }
        }
    }
}
