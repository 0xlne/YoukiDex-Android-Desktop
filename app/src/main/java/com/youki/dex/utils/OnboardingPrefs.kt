package com.youki.dex.utils

import android.content.Context
import androidx.preference.PreferenceManager

/**
 * Storage for first-run onboarding state (see SplashActivity, OnboardingActivity) — whether
 * onboarding has completed. Uses the default SharedPreferences (same store every other
 * preference in this app uses) rather than a separate file, since these are genuinely just
 * more app preferences, not sensitive or high-volume data that would need its own store.
 */
object OnboardingPrefs {
    /**
     * A user-facing display name the app itself remembers, separate from the OS-level
     * Android user account name (DeviceUtils.getUserName() / UserManager.userName is
     * read-only for a regular app — renaming the actual OS user account requires
     * MANAGE_USERS, a system-level permission this app doesn't and shouldn't have).
     * Falls back to DeviceUtils.getUserName() when nothing's been explicitly set here,
     * so a fresh install still shows something sensible before onboarding ever runs.
     */
    fun getDisplayName(context: Context): String? {
        val stored = PreferenceManager.getDefaultSharedPreferences(context).getString(KEY_DISPLAY_NAME, null)
        return stored ?: DeviceUtils.getUserName(context)
    }

    fun setDisplayName(context: Context, name: String) {
        PreferenceManager.getDefaultSharedPreferences(context).edit()
            .putString(KEY_DISPLAY_NAME, name).apply()
    }

    private const val KEY_DISPLAY_NAME = "app_display_name"
    private const val KEY_ONBOARDING_COMPLETE = "onboarding_complete"

    fun isOnboardingComplete(context: Context): Boolean =
        PreferenceManager.getDefaultSharedPreferences(context).getBoolean(KEY_ONBOARDING_COMPLETE, false)

    fun setOnboardingComplete(context: Context, complete: Boolean) {
        PreferenceManager.getDefaultSharedPreferences(context).edit()
            .putBoolean(KEY_ONBOARDING_COMPLETE, complete).apply()
    }

    /**
     * Which rendering backend (OpenGL ES vs Vulkan) the live wallpaper engine uses.
     * See OnboardingRendererChoiceFragment (Step 5) and the equivalent Advanced Settings
     * screen — both write here. Per the project decision, this choice is made directly by
     * the user (a plain textual explanation, no live preview) rather than by any
     * benchmark/heuristic the app runs itself, since Vulkan support quality varies too
     * much device-to-device for a simple version check to be trustworthy.
     *
     * Stored as the plain String name of RendererBackend (AUTO/GLES/VULKAN) rather than an
     * ordinal Int, so the stored value stays meaningful/debuggable if the enum is ever
     * reordered — matches how KEY_SPLASH_VIDEO_URI stores a URI string rather than some
     * more compact encoding, for the same "don't over-optimize a rarely-read preference"
     * reasoning.
     */
    enum class RendererBackend { AUTO, GLES, VULKAN }

    fun getRendererBackend(context: Context): RendererBackend {
        val stored = PreferenceManager.getDefaultSharedPreferences(context)
            .getString(KEY_RENDERER_BACKEND, null)
        return stored?.let {
            try { RendererBackend.valueOf(it) } catch (e: IllegalArgumentException) { RendererBackend.AUTO }
        } ?: RendererBackend.AUTO
    }

    fun setRendererBackend(context: Context, backend: RendererBackend) {
        PreferenceManager.getDefaultSharedPreferences(context).edit()
            .putString(KEY_RENDERER_BACKEND, backend.name).apply()
    }

    private const val KEY_RENDERER_BACKEND = "renderer_backend"
}
