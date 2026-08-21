//! Rendering engine core: a common `RendererBackend` trait implemented by
//! both the OpenGL ES backend (`gl_backend`) and the Vulkan backend
//! (`vk_backend`), plus the transform-matrix math shared by both
//! (equivalent to the old TransformMatrix.kt).
//!
//! Design note (per project decision): the two backends are never asked to
//! run side-by-side for a live preview — the onboarding/Advanced Settings UI
//! only needs `query_capabilities()` (a cheap, renderer-less Vulkan instance
//! query) plus a plain descriptive text choice. `init()` is only ever called
//! once, for whichever backend the user picked (or "Auto" resolved to).

mod gl_backend;
mod matrix;
mod vk_backend;

pub use gl_backend::GlRenderer;
// NOTE: TransformMatrix is not re-exported here since nothing outside
// renderer/ currently references it directly — gl_backend.rs imports it via
// `crate::renderer::matrix::TransformMatrix` instead. Caught as an unused-
// import warning by the first real build; re-add if an external caller
// needs `renderer::TransformMatrix` directly in the future.
// pub use matrix::TransformMatrix;
pub use vk_backend::VkRenderer;

use ndk::native_window::NativeWindow;

/// Mirrors the WallpaperConfig fields the old WallpaperGLRenderer.kt consumed
/// via updateConfig() — crop/fit mode, pan/zoom, brightness/contrast. Kotlin
/// continues to own reading this from Room (WallpaperConfigRepository) and
/// simply passes the resolved struct across JNI; Rust never touches the DB.
/// Mirrors the subset of WallpaperConfig fields the old WallpaperGLRenderer.kt
/// + TransformMatrix.kt actually consumed (cross-checked field-by-field
/// against WallpaperConfig.kt — do not add fields here that aren't read by
/// drawVideoFrame()/drawBackgroundQuad()/TransformMatrix.build() in the
/// original, and do not omit any that are). Kotlin continues to own reading
/// this from Room (WallpaperConfigRepository) and simply passes the resolved
/// struct across JNI; Rust never touches the DB.
///
/// NOTE: an earlier draft of this struct incorrectly invented `pan_x`/
/// `pan_y`/`zoom` fields that do not exist anywhere in WallpaperConfig.kt or
/// TransformMatrix.kt, and was missing `background_color` (used to clear +
/// draw the background quad, see WallpaperGLRenderer.kt line ~170) and
/// `saturation` (bound to the uSaturation uniform, line ~221). Both were
/// caught by re-reading the original files line-by-line and are fixed here.
#[derive(Clone, Debug)]
pub struct FrameConfig {
    pub scale_mode: ScaleMode,
    /// "#AARRGGBB" hex string, parsed the same way WallpaperGLRenderer's
    /// private parseColor() did (see gl_backend::parse_color) — kept as the
    /// raw string across the JNI boundary and parsed on the Rust side, per
    /// the project's "Kotlin = UI only, all logic incl. parsing = Rust" rule.
    /// Parsed once per config update (not per-frame) by the JNI bridge layer
    /// before being cached in the backend — see jni_bridge::renderer.
    pub background_color: String,
    pub brightness: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub color_correction_enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    /// No aspect correction — video fills [-1,1]x[-1,1] entirely as-is.
    Stretch,
    /// Smaller dimension determines scale — full video visible, empty space
    /// on the edges.
    Fit,
    /// Larger dimension determines scale — fully covers the screen, crops
    /// the excess. (Named `Cover` here to match WallpaperConfig.ScaleMode.COVER
    /// exactly; do not rename to "Crop" — that was an error in an earlier
    /// draft of this file and does not match the Kotlin enum it mirrors.)
    Cover,
    /// Same base scaling as Cover, plus the user's free-transform (scale/
    /// rotate/translate/flip, per-orientation) applied on top. See
    /// matrix::FreeTransform and matrix::TransformMatrix::build.
    Free,
}

/// Which GPU API is currently driving the live wallpaper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Gles,
    Vulkan,
}

/// User's Advanced Settings choice — "Auto" is resolved to a concrete
/// Backend once, at renderer init time, not re-evaluated per frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendPreference {
    Auto,
    ForceGles,
    ForceVulkan,
}

/// Common surface lifecycle every backend must implement. Mirrors the three
/// GLSurfaceView.Renderer callbacks the Kotlin engine drove manually from
/// WallpaperGLEngine.kt's EGL thread — the manual-EGL-thread approach itself
/// carries over conceptually, just now built on `ndk::native_window` instead
/// of `SurfaceHolder`.
pub trait RendererBackend {
    /// Called once when the ANativeWindow becomes available.
    fn on_surface_created(&mut self, window: &NativeWindow) -> Result<(), String>;
    /// Called whenever the surface size changes.
    fn on_surface_changed(&mut self, width: i32, height: i32);
    /// Called once per frame. `has_new_frame` mirrors
    /// SurfaceTexture.OnFrameAvailableListener firing since the last draw.
    fn on_draw_frame(&mut self, config: &FrameConfig, has_new_frame: bool);
    /// Releases all GPU resources (EGL/Vulkan context, textures, buffers).
    fn on_surface_destroyed(&mut self);
    /// Returns an ANativeWindow-compatible Surface source that ExoPlayer's
    /// setVideoSurface() can target — for GL this is the SurfaceTexture-backed
    /// external OES texture surface; for Vulkan this is the AHardwareBuffer
    /// bridge surface (see handoff doc's "SurfaceTexture → Vulkan" note).
    /// Returned as a raw JNI global ref handle for Kotlin to wrap as a
    /// android.view.Surface and hand to VideoEngine.prepare().
    fn video_input_surface_handle(&self) -> i64;
}

/// Vulkan capability info surfaced to the onboarding step / Advanced
/// Settings screen as plain descriptive text — no rendering, no surface,
/// just an instance-level query (cheap, no GPU context held afterwards).
#[derive(Clone, Debug)]
pub struct VulkanCapabilities {
    pub supported: bool,
    pub api_version_major: u32,
    pub api_version_minor: u32,
    pub api_version_patch: u32,
    pub device_name: String,
}

/// Cheap, renderer-less query used by:
///   - OnboardingRendererChoiceFragment (plain text: "Your device supports
///     Vulkan 1.3")
///   - Advanced Settings (same text, re-checked on demand)
///
/// Creates a throwaway VkInstance (no VkDevice, no surface, no swapchain),
/// reads the highest supported API version + the first physical device's
/// name, then immediately drops the instance. Safe to call at any time,
/// including while the live wallpaper's own renderer is active elsewhere,
/// since it never touches a Surface/ANativeWindow.
pub fn query_vulkan_capabilities() -> VulkanCapabilities {
    vk_backend::query_capabilities()
}

/// Resolves "Auto" to a concrete backend. Per project decision this is a
/// simple, transparent default (not a hidden heuristic/benchmark) — Auto
/// currently means "prefer Vulkan if the device reports it at all", since
/// the onboarding step's plain-text description already sets the user's
/// expectation that Vulkan support varies. This can be revisited without
/// changing the public FFI shape.
pub fn resolve_preference(pref: BackendPreference) -> Backend {
    match pref {
        BackendPreference::ForceGles => Backend::Gles,
        BackendPreference::ForceVulkan => Backend::Vulkan,
        BackendPreference::Auto => {
            if query_vulkan_capabilities().supported {
                Backend::Vulkan
            } else {
                Backend::Gles
            }
        }
    }
}

/// Constructs the concrete backend implementation. Called once per live
/// wallpaper engine lifecycle (see jni_bridge::renderer for the JNI-facing
/// wrapper that owns this behind a stable pointer handle).
pub fn create_backend(backend: Backend) -> Box<dyn RendererBackend> {
    match backend {
        Backend::Gles => Box::new(GlRenderer::new()),
        Backend::Vulkan => Box::new(VkRenderer::new()),
    }
}
