package com.youki.dex.fragments

import android.app.ActivityOptions
import android.content.ActivityNotFoundException
import android.content.ComponentName
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.widget.Toast
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.PreferenceManager
import com.youki.dex.R
import com.youki.dex.livewallpaper.ui.gallery.GalleryActivity
import com.youki.dex.utils.AppUtils
import com.youki.dex.utils.DeviceUtils

class PreferencesFragment : PreferenceFragmentCompat() {

    companion object {
        // MaterialFiles — the external app that replaced the old internal file manager
        private const val MATERIAL_FILES_PACKAGE = "me.zhanghai.android.files"
        private const val MATERIAL_FILES_ACTIVITY = "me.zhanghai.android.files.filelist.FileListActivity"

        // windowing mode constants (matching what's in AppUtils)
        private const val WINDOWING_MODE_FREEFORM = 5
    }

    override fun onCreatePreferences(savedInstanceState: Bundle?, rootKey: String?) {
        setPreferencesFromResource(R.xml.preferences_main, rootKey)
        refreshBetaVisibility()

        // Live Wallpaper Gallery — نفس أنيميشن Pixel-style لبقية صفحات الإعدادات
        findPreference<Preference>("lw_open_gallery")?.setOnPreferenceClickListener {
            startActivity(Intent(requireContext(), GalleryActivity::class.java))
            requireActivity().overridePendingTransition(
                R.anim.fragment_open_enter,
                R.anim.fragment_open_exit
            )
            true
        }

        // File Manager — opens the external MaterialFiles app in a freeform window (not fullscreen)
        findPreference<Preference>("open_file_manager")?.setOnPreferenceClickListener {
            openMaterialFiles()
            true
        }
    }

    /**
     * Opens MaterialFiles in windowed (freeform) mode instead of fullscreen.
     *
     * The old problem: FLAG_ACTIVITY_NEW_TASK with no ActivityOptions used to
     * make the system open the app in fullscreen mode (the default mode for
     * any external app).
     *
     * The fix: we use AppUtils.makeActivityOptions the same way PerfectServer
     * uses it to launch every other app — this guarantees consistency
     */
    private fun openMaterialFiles() {
        val ctx = requireContext()
        val sp = PreferenceManager.getDefaultSharedPreferences(ctx)
        val dockHeight = sp.getInt("dock_height", 0)

        // We use the same "standard" launch mode PerfectServer uses for regular apps
        val launchMode = sp.getString("launch_mode", "standard") ?: "standard"
        val options = AppUtils.makeActivityOptions(ctx, launchMode, dockHeight, android.view.Display.DEFAULT_DISPLAY)

        val intent = Intent().apply {
            component = ComponentName(MATERIAL_FILES_PACKAGE, MATERIAL_FILES_ACTIVITY)
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        }

        try {
            ctx.startActivity(intent, options.toBundle())
        } catch (e: ActivityNotFoundException) {
            // FIX (hardcoded Arabic text): the Toast message used to always be fixed
            // Arabic text. Now it's read from strings.xml (materialfiles_not_installed),
            // so it's translated automatically based on the device/app language.
            Toast.makeText(
                ctx,
                ctx.getString(R.string.materialfiles_not_installed),
                Toast.LENGTH_SHORT
            ).show()
        }
    }

    override fun onResume() {
        super.onResume()
        refreshBetaVisibility()
    }

    private fun refreshBetaVisibility() {
        val dev = PreferenceManager.getDefaultSharedPreferences(requireContext())
            .getBoolean("developer_mode_enabled", false)
        findPreference<Preference>("beta_preview")?.isVisible = dev
    }
}
