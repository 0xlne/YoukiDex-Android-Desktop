package com.youki.dex.fragments

import android.content.Intent
import android.os.Bundle
import android.provider.Settings
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import com.youki.dex.R
import com.youki.dex.utils.AppUtils

/**
 * Permissions management screen.
 * Shows the dedicated permissions XML which includes a working
 * "Notification Listener" entry that opens the correct system settings page.
 */
class PermissionsPreferences : PreferenceFragmentCompat() {
    override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
        setPreferencesFromResource(R.xml.preferences_advanced, rootKey)
    }

    override fun onPreferenceTreeClick(preference: Preference): Boolean {
        // FIX: Notification permission — open the Notification Listener page directly
        if (preference.key == "notification_listener_permission") {
            AppUtils.openSystemSettings(requireContext(), Intent(Settings.ACTION_NOTIFICATION_LISTENER_SETTINGS))
            return true
        }
        return super.onPreferenceTreeClick(preference)
    }
}
