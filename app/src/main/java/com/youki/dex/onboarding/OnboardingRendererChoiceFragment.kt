package com.youki.dex.onboarding

import android.os.Bundle
import android.view.View
import android.widget.TextView
import androidx.fragment.app.Fragment
import com.google.android.material.button.MaterialButtonToggleGroup
import com.youki.dex.R
import com.youki.dex.utils.OnboardingPrefs

/**
 * Step 4 — "Renderer" choice: OpenGL ES vs. Vulkan vs. Auto for the live wallpaper engine.
 *
 * Deliberately simple per the project decision: plain descriptive text explaining the
 * tradeoff, plus the device's detected Vulkan API version shown as informational text —
 * NO live preview, NO in-app benchmark. An earlier design considered running both
 * renderers briefly for a side-by-side comparison; that was dropped because Vulkan
 * support quality varies too unpredictably device-to-device for either a fixed
 * version-number rule or a quick on-device benchmark to be trustworthy — the same device
 * running Vulkan 1.1 can render worse than another device also on 1.1, depending on driver
 * quality, so ANY automatic heuristic risks picking wrong. The user's own judgment (aided
 * by the plain description here, and revisitable later from Advanced Settings) is the
 * actual decision-maker.
 *
 * Writes straight to storage the moment it's chosen — there is nothing to "commit" when
 * leaving this step. Advanced Settings reuses the exact same
 * OnboardingPrefs.getRendererBackend / setRendererBackend pair, so a user can freely
 * change their mind after onboarding too.
 */
class OnboardingRendererChoiceFragment : Fragment(R.layout.fragment_onboarding_renderer_choice) {

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)
        val ctx = requireContext()

        val vulkanInfoTv = view.findViewById<TextView>(R.id.onboarding_renderer_vulkan_info_tv)
        val backendToggle =
            view.findViewById<MaterialButtonToggleGroup>(R.id.onboarding_renderer_toggle_group)

        // ── Vulkan capability text ──────────────────────────────────────────
        // NativeBridge.nativeQueryVulkanCapabilities() (see jni_bridge.rs) creates a
        // throwaway VkInstance purely to read the supported API version + device name,
        // then immediately destroys it — cheap enough to call synchronously here on the
        // main thread (same "instance-only query, no surface, no swapchain" design as the
        // Advanced Settings screen re-uses below). If the native library hasn't loaded yet
        // for any reason, fall back to a neutral message rather than crashing onboarding
        // over a purely informational string.
        vulkanInfoTv.text = try {
            // NOTE: must match the exact package the JNI symbols in
            // jni_bridge.rs are named after (Java_com_youki_dex_livewallpaper_
            // NativeBridge_*) — see that file's top-of-file naming note. If
            // the actual PR places NativeBridge in a different package/class,
            // this call site AND every Java_* symbol name in jni_bridge.rs
            // must be updated together, or JNI resolution silently fails at
            // runtime (UnsatisfiedLinkError, not a compile error).
            com.youki.dex.livewallpaper.NativeBridge.nativeQueryVulkanCapabilities()
        } catch (e: UnsatisfiedLinkError) {
            getString(R.string.onboarding_renderer_vulkan_info_unavailable)
        }

        // ── Restore whatever was already chosen (e.g. the user goes Back and revisits
        // this step) rather than always resetting to defaults.
        val current = OnboardingPrefs.getRendererBackend(ctx)
        backendToggle.check(
            when (current) {
                OnboardingPrefs.RendererBackend.GLES -> R.id.onboarding_renderer_gles_btn
                OnboardingPrefs.RendererBackend.VULKAN -> R.id.onboarding_renderer_vulkan_btn
                OnboardingPrefs.RendererBackend.AUTO -> R.id.onboarding_renderer_auto_btn
            }
        )

        backendToggle.addOnButtonCheckedListener { _, checkedId, isChecked ->
            if (!isChecked) return@addOnButtonCheckedListener
            val chosen = when (checkedId) {
                R.id.onboarding_renderer_gles_btn -> OnboardingPrefs.RendererBackend.GLES
                R.id.onboarding_renderer_vulkan_btn -> OnboardingPrefs.RendererBackend.VULKAN
                else -> OnboardingPrefs.RendererBackend.AUTO
            }
            // Written immediately, same "a choice is meaningful the moment it's made"
            // reasoning as every other onboarding step in this wizard — there is no
            // separate "confirm"/"next" step that actually performs the save.
            OnboardingPrefs.setRendererBackend(ctx, chosen)
        }
    }
}
