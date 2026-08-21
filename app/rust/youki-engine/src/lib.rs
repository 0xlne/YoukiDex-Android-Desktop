//! youki_engine — native Rust core for YoukiDEX.
//!
//! Scope (see project handoff doc "YoukiDEX-RustPort-Handoff.md" for full
//! rationale on what belongs here vs what must stay in Kotlin):
//!
//!   - `renderer`   : live wallpaper rendering (OpenGL ES + Vulkan backends,
//!                    dynamic switch). Replaces livewallpaper/gl/ + service/.
//!   - `icons`      : icon loading/compositing/caching as a GPU texture atlas.
//!                    Replaces the Canvas/Bitmap parts of AppAdapter,
//!                    DockAppAdapter, IconPackUtils, ColorUtils, Utils.
//!   - `root`       : process-spawn + output-parsing logic for `su` shell
//!                    execution. Replaces the compute/parsing parts of
//!                    RootManager.kt (NOT Shizuku — that stays Kotlin, it's
//!                    Binder-bound).
//!   - `archive`    : ZIP compress/extract. Replaces FileManagerFragment.kt's
//!                    zip logic.
//!   - `textparse`  : regex/parsing helpers shared by MultiUserManager.kt and
//!                    AppUtils.kt equivalents.
//!
//! Everything in this crate is UI-agnostic: no Android View/Activity/Service
//! types cross the JNI boundary. Kotlin owns all framework lifecycle and
//! Binder-bound APIs (AccessibilityService, NotificationListenerService,
//! Shizuku, ContentResolver) and calls into this crate only for computation
//! or GPU work.

// ── Module declarations ──────────────────────────────────────────────────
// Only `renderer` and `root` exist as actual files as of this writing.
// The other four are commented out rather than left as broken `mod`
// declarations pointing at nonexistent files — an uncommented `mod archive;`
// with no archive.rs would fail `cargo check` immediately with a hard
// "file not found for module" error, which is a real, current problem, not
// a hypothetical future one. Uncomment each as its file is added.
//
mod archive;    // ZIP compress/extract — implemented (see archive.rs).
mod desktop_grid; // Launcher desktop cell-occupancy grid + drag-drop push
                  // resolution — ported from DesktopGridPrefs.kt's
                  // CellRect/DesktopOccupancyGrid/PushDirection and
                  // LauncherActivity.kt's pixelToCell/cellToPixel/resolveDrop.
// mod icons;       // NOT PORTED: after reading IconPackUtils.kt/ColorUtils.kt/
//                  // Utils.kt in full, these turned out to be almost entirely
//                  // Android Framework API calls (PackageManager, Resources,
//                  // Canvas.drawCircle/drawBitmap with Xfermode — hardware-
//                  // accelerated Android drawing calls, not manual pixel
//                  // loops) with no substantial pure-compute logic left over
//                  // once the framework calls are excluded. Porting would
//                  // mean reimplementing Android's own accelerated Canvas
//                  // compositing by hand in Rust, which is not faster and
//                  // is not "logic that belongs in Rust" per the project's
//                  // own compute-vs-framework-API rule — see handoff doc.
mod jni_bridge; // JNI entry points — implemented for root/archive/textparse/
                // Vulkan-capability-query. Renderer handle bridge (surface
                // lifecycle) intentionally deferred — see jni_bridge.rs's
                // "renderer bridge — NOT YET IMPLEMENTED" note.
mod renderer;
mod root;
mod shell_client; // TCP client for youki_shell_server — implemented, replaces
                  // ShellManager.kt's socket/protocol/state-machine logic
                  // (see shell_client.rs for what stays in Kotlin vs here).
mod textparse;   // Regex/parsing/sorting helpers — implemented (see textparse.rs).
mod window_geometry; // Pure-math window-bounds/launch-command logic ported
                     // from AppUtils.kt (see window_geometry.rs's module doc
                     // for the full breakdown of what moved vs what stayed
                     // Kotlin-side within that same file).

use android_logger::Config;
use log::LevelFilter;

/// Called once from Kotlin (e.g. Application.onCreate or the first JNI call)
/// to route Rust `log::*!` macros to logcat under the "YoukiEngine" tag.
/// Safe to call more than once; android_logger is idempotent about this.
#[allow(dead_code)]
fn init_logging() {
    android_logger::init_once(
        Config::default()
            .with_max_level(LevelFilter::Debug)
            .with_tag("YoukiEngine"),
    );
}

// Re-export the JNI entry points so `javac`/the JVM can resolve
// Java_com_youki_dex_livewallpaper_NativeBridge_* symbols from this crate
// root — see jni_bridge.rs's top-of-file note on the class-name assumption.
pub use jni_bridge::*;
