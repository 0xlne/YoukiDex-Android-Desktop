//! window_geometry — pure-math window-bounds/launch-command logic ported
//! from AppUtils.kt.
//!
//! SCOPE NOTE (read before extending this file): AppUtils.kt is ~730 lines,
//! and the large majority of it is direct Android Framework API calls
//! (PackageManager, LauncherApps, ActivityManager, UserManager,
//! reflection into ActivityOptions' hidden methods/fields) with no
//! separable computation — per this project's own precedent (see
//! textparse.rs's module doc and desktop_grid.rs's "zero Android
//! Framework dependency" framing), those stay in Kotlin, not because of
//! any reluctance to port them, but because there is no logic left once
//! the Framework call is excluded — porting would mean reimplementing
//! PackageManager/LauncherApps by hand, which is not what "logic" means
//! in this project's own split.
//!
//! What DID move here — the parts of AppUtils.kt that are pure
//! arithmetic/string-building with no Android type crossing the
//! boundary:
//!   - `makeLaunchBounds()`      -> `compute_launch_bounds()`
//!   - `buildShellLaunchCommand()`'s bounds-string + `am start` command
//!     assembly -> `build_launch_command()`
//!   - `resizeTask()`'s two shell command strings -> `build_resize_commands()`
//!   - `getDockSizeConfig()`'s preset lookup table -> `dock_size_config()`
//!
//! What stayed in Kotlin from those same functions: `makeLaunchBounds`'s
//! calls into `DeviceUtils.getDisplayMetrics/getStatusBarHeight/
//! getNavBarHeight` (Framework `Resources`/`WindowManager` calls) — Kotlin
//! now gathers those four ints and passes them in, rather than this
//! module calling back into Android itself (this crate has no Context).
//! Likewise `setWindowingMode`'s reflection into `ActivityOptions`'
//! hidden API surface stays in Kotlin: reflection *is* the Android-typed
//! object being manipulated, there's no computation to extract from it.

/// Mirrors Kotlin's `android.graphics.Rect` shape (left/top/right/bottom
/// in pixels) without depending on any Android type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct LaunchBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Direct port of `AppUtils.makeLaunchBounds()`'s arithmetic. All Android
/// Framework-derived inputs (display size, status/nav bar heights) are
/// passed in as plain ints — Kotlin gathers them via `DeviceUtils` before
/// calling this.
///
/// `mode` matches the original's string switch exactly: "standard",
/// "maximized", "portrait", "tiled-left", "tiled-top", "tiled-right",
/// "tiled-bottom". Any other value falls through to all-zero bounds,
/// same as the original's `when` with no matching branch (Kotlin's
/// `when` here has no `else`, so unmatched values silently leave
/// left/top/right/bottom at their initial 0 — this port keeps that
/// behavior rather than silently picking a "safer" default; if that
/// silent-zero behavior for unrecognized modes was accidental in the
/// original rather than intentional, that's a pre-existing behavior to
/// flag back to the original codebase, not something to quietly change
/// during a port).
pub fn compute_launch_bounds(
    mode: &str,
    device_width: i32,
    device_height: i32,
    status_height: i32,
    nav_height: i32,
    dock_height: i32,
    apply_navbar_fix: bool,
    scale_factor: f32,
) -> LaunchBounds {
    let diff = (dock_height - nav_height).max(0);
    let usable_height = if apply_navbar_fix {
        device_height - diff - status_height
    } else {
        device_height - dock_height - status_height
    };

    let mut b = LaunchBounds::default();
    match mode {
        "standard" => {
            b.left = (device_width as f32 / (5.0 * scale_factor)) as i32;
            b.top = ((usable_height + status_height) as f32 / (7.0 * scale_factor)) as i32;
            b.right = device_width - b.left;
            b.bottom = usable_height + dock_height - b.top;
        }
        "maximized" => {
            b.right = device_width;
            b.bottom = usable_height;
        }
        "portrait" => {
            b.left = device_width / 3;
            b.top = usable_height / 15;
            b.right = device_width - b.left;
            b.bottom = usable_height + dock_height - b.top;
        }
        "tiled-left" => {
            b.right = device_width / 2;
            b.bottom = usable_height;
        }
        "tiled-top" => {
            b.right = device_width;
            b.bottom = (usable_height + status_height) / 2;
        }
        "tiled-right" => {
            b.left = device_width / 2;
            b.right = device_width;
            b.bottom = usable_height;
        }
        "tiled-bottom" => {
            b.right = device_width;
            b.top = (usable_height + status_height) / 2;
            b.bottom = usable_height + status_height;
        }
        _ => {} // no matching mode -> all-zero bounds, same as the Kotlin original
    }
    b
}

const WINDOWING_MODE_FULLSCREEN: i32 = 1;
const WINDOWING_MODE_FREEFORM: i32 = 5;

/// Direct port of `AppUtils.buildShellLaunchCommand()`'s string assembly.
/// `component` must already be the flattened `pkg/.Activity` string
/// (Kotlin resolves that via `PackageManager.getLaunchIntentForPackage`
/// before calling this — that resolution is a Framework call, not logic).
/// Returns an empty string if `component` is empty, matching the
/// original's `?: return ""` short-circuit.
pub fn build_launch_command(
    component: &str,
    mode: &str,
    bounds: LaunchBounds,
    display_id: i32,
) -> String {
    if component.is_empty() {
        return String::new();
    }
    let wm = if mode == "fullscreen" {
        WINDOWING_MODE_FULLSCREEN
    } else {
        WINDOWING_MODE_FREEFORM
    };
    format!(
        "am start -n {component} --windowingMode {wm} --display {display_id} --launch-bounds \"{} {} {} {}\"",
        bounds.left, bounds.top, bounds.right, bounds.bottom
    )
}

/// Direct port of the two shell command strings built inside
/// `AppUtils.resizeTask()`. The actual execution (ShellManager socket ->
/// Shizuku -> Root fallback chain, plus the `Thread.sleep(150)` between
/// them) stays in Kotlin — those are process/IPC calls, not computation.
pub fn build_resize_commands(task_id: i32, bounds: LaunchBounds) -> (String, String) {
    let mode_cmd = format!("am task set-windowing-mode {task_id} {WINDOWING_MODE_FREEFORM}");
    let resize_cmd = format!(
        "am task resize {task_id} {} {} {} {}",
        bounds.left, bounds.top, bounds.right, bounds.bottom
    );
    (mode_cmd, resize_cmd)
}

/// One dock-size preset — mirrors Kotlin's `AppUtils.DockSizeConfig` data
/// class shape (the `toHeightPx`/`toGridSizePx`/`toIconSizePx` dp->px
/// conversions themselves stay in Kotlin: they call `Utils.dpToPx`, which
/// needs `Context`/`Resources` display density — a Framework value this
/// crate has no access to).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DockSizeConfig {
    pub dock_height_dp: i32,
    pub icon_size_dp: i32,
    pub grid_size_dp: i32,
    pub use_system_density: bool,
}

/// Direct port of `AppUtils.getDockSizeConfig()`'s preset table. The
/// "normal" branch's `dock_height` comes from a preference string in the
/// original (`prefs.getString("dock_height", "56")?.toIntOrNull() ?: 56`)
/// — Kotlin reads that preference and passes the already-parsed int in,
/// since SharedPreferences access is a Framework/Context-bound call.
pub fn dock_size_config(preset: &str, normal_dock_height_dp: i32) -> DockSizeConfig {
    match preset {
        "small" => DockSizeConfig {
            dock_height_dp: 40,
            icon_size_dp: 34,
            grid_size_dp: 42,
            use_system_density: false,
        },
        "pc" => DockSizeConfig {
            dock_height_dp: 30,
            icon_size_dp: 24,
            grid_size_dp: 32,
            use_system_density: true,
        },
        _ => DockSizeConfig {
            dock_height_dp: normal_dock_height_dp,
            icon_size_dp: 50,
            grid_size_dp: 52,
            use_system_density: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maximized_bounds_fill_usable_area() {
        let b = compute_launch_bounds("maximized", 1080, 2400, 80, 60, 56, false, 1.0);
        assert_eq!(b.left, 0);
        assert_eq!(b.top, 0);
        assert_eq!(b.right, 1080);
        assert_eq!(b.bottom, 2400 - 56 - 80);
    }

    #[test]
    fn unknown_mode_yields_zero_bounds() {
        let b = compute_launch_bounds("nonexistent", 1080, 2400, 80, 60, 56, false, 1.0);
        assert_eq!(b, LaunchBounds::default());
    }

    #[test]
    fn build_launch_command_empty_component_short_circuits() {
        let cmd = build_launch_command("", "fullscreen", LaunchBounds::default(), 0);
        assert!(cmd.is_empty());
    }

    #[test]
    fn build_launch_command_fullscreen_uses_mode_1() {
        let bounds = LaunchBounds { left: 1, top: 2, right: 3, bottom: 4 };
        let cmd = build_launch_command("com.pkg/.Main", "fullscreen", bounds, 0);
        assert!(cmd.contains("--windowingMode 1"));
        assert!(cmd.contains("--launch-bounds \"1 2 3 4\""));
    }

    #[test]
    fn build_launch_command_other_modes_use_freeform_5() {
        let cmd = build_launch_command("com.pkg/.Main", "standard", LaunchBounds::default(), 2);
        assert!(cmd.contains("--windowingMode 5"));
        assert!(cmd.contains("--display 2"));
    }

    #[test]
    fn resize_commands_match_expected_shape() {
        let bounds = LaunchBounds { left: 10, top: 20, right: 30, bottom: 40 };
        let (mode_cmd, resize_cmd) = build_resize_commands(42, bounds);
        assert_eq!(mode_cmd, "am task set-windowing-mode 42 5");
        assert_eq!(resize_cmd, "am task resize 42 10 20 30 40");
    }

    #[test]
    fn dock_size_presets_match_originals() {
        assert_eq!(dock_size_config("small", 56).dock_height_dp, 40);
        assert_eq!(dock_size_config("pc", 56).use_system_density, true);
        assert_eq!(dock_size_config("normal", 72).dock_height_dp, 72);
        assert_eq!(dock_size_config("anything-else", 72).dock_height_dp, 72);
    }
}

// ── OnSwipeListener.kt ────────────────────────────────────────────────────

/// Mirrors `OnSwipeListener.Direction`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

impl SwipeDirection {
    /// Kotlin's `name` property equivalent, for the JNI bridge to return a string.
    pub fn as_str(self) -> &'static str {
        match self {
            SwipeDirection::Up => "UP",
            SwipeDirection::Down => "DOWN",
            SwipeDirection::Left => "LEFT",
            SwipeDirection::Right => "RIGHT",
        }
    }
}

/// Direct port of `OnSwipeListener.getAngle()`. Returns degrees in [0, 360).
fn swipe_angle(x1: f32, y1: f32, x2: f32, y2: f32) -> f64 {
    let rad = ((y1 - y2) as f64).atan2((x2 - x1) as f64) + std::f64::consts::PI;
    (rad * 180.0 / std::f64::consts::PI + 180.0) % 360.0
}

fn in_range(angle: f64, init: f32, end: f32) -> bool {
    angle >= init as f64 && angle < end as f64
}

/// Direct port of `OnSwipeListener.Direction.fromAngle()`, called via
/// `getDirection()`. Boundaries copied exactly from the original's
/// if/else-if chain (order matters: UP checked first, RIGHT's range is
/// split across the 0°/360° wraparound, DOWN next, everything else falls
/// to LEFT).
pub fn swipe_direction(x1: f32, y1: f32, x2: f32, y2: f32) -> SwipeDirection {
    let angle = swipe_angle(x1, y1, x2, y2);
    if in_range(angle, 45.0, 135.0) {
        SwipeDirection::Up
    } else if in_range(angle, 0.0, 45.0) || in_range(angle, 315.0, 360.0) {
        SwipeDirection::Right
    } else if in_range(angle, 225.0, 315.0) {
        SwipeDirection::Down
    } else {
        SwipeDirection::Left
    }
}

#[cfg(test)]
mod swipe_tests {
    use super::*;

    #[test]
    fn rightward_swipe_is_right() {
        // moving from (0,0) to (100,0): finger travels right
        assert_eq!(swipe_direction(0.0, 0.0, 100.0, 0.0), SwipeDirection::Right);
    }

    #[test]
    fn leftward_swipe_is_left() {
        assert_eq!(swipe_direction(100.0, 0.0, 0.0, 0.0), SwipeDirection::Left);
    }

    #[test]
    fn upward_swipe_is_up() {
        // screen y grows downward, so moving to a smaller y is an upward swipe
        assert_eq!(swipe_direction(0.0, 100.0, 0.0, 0.0), SwipeDirection::Up);
    }

    #[test]
    fn downward_swipe_is_down() {
        assert_eq!(swipe_direction(0.0, 0.0, 0.0, 100.0), SwipeDirection::Down);
    }
}

// ── UserSwitcherPopup.kt ─────────────────────────────────────────────────

/// Direct port of `UserSwitcherPopup.show()`'s inline `lin()` helper — the
/// WCAG 2.0 sRGB-to-linear-light conversion for one channel (input in
/// [0.0, 1.0]).
fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.03928 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Direct port of the relative-luminance computation in
/// `UserSwitcherPopup.show()`. `argb` is a packed 0xAARRGGBB int (Android
/// `Color` format — alpha is ignored, matching the original which only
/// reads red/green/blue via `Color.red/green/blue`). Returns the WCAG 2.0
/// relative luminance in [0.0, 1.0].
pub fn relative_luminance(argb: i32) -> f64 {
    let r = ((argb >> 16) & 0xFF) as f64 / 255.0;
    let g = ((argb >> 8) & 0xFF) as f64 / 255.0;
    let b = (argb & 0xFF) as f64 / 255.0;
    0.2126 * srgb_to_linear(r) + 0.7152 * srgb_to_linear(g) + 0.0722 * srgb_to_linear(b)
}

/// Direct port of `val isBubbleLight = lum > 0.35` plus the two color
/// picks that follow it. Returns true if [argb]'s luminance exceeds the
/// original's 0.35 threshold (note: NOT the WCAG-standard 0.5 threshold —
/// this project's own choice, kept as-is rather than "corrected" to the
/// textbook value, since 0.35 was almost certainly chosen deliberately to
/// switch to dark text slightly earlier / err toward readability).
pub fn is_bubble_light(argb: i32) -> bool {
    relative_luminance(argb) > 0.35
}

#[cfg(test)]
mod luminance_tests {
    use super::*;

    #[test]
    fn white_is_light() {
        assert!(is_bubble_light(0xFFFFFFFFu32 as i32));
        assert!((relative_luminance(0xFFFFFFFFu32 as i32) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn black_is_not_light() {
        assert!(!is_bubble_light(0xFF000000u32 as i32));
        assert!(relative_luminance(0xFF000000u32 as i32).abs() < 1e-9);
    }

    #[test]
    fn matches_default_bubble_color_from_kotlin() {
        // Color.argb(255, 28, 28, 30) from the original's default bubbleColor
        let argb = (255 << 24) | (28 << 16) | (28 << 8) | 30;
        // A very dark near-black color — should not be classified as light.
        assert!(!is_bubble_light(argb));
    }

    #[test]
    fn mid_gray_luminance_boundary() {
        // sRGB 0.5 gray: linear-light value is below 0.5 due to gamma,
        // sanity-check it lands in a plausible range rather than asserting
        // an exact literal (avoids hand-copying a possibly-wrong constant).
        let lum = relative_luminance(0xFF808080u32 as i32);
        assert!(lum > 0.15 && lum < 0.30);
    }
}

// ── EasterEggActivity.kt::fitSurfaceToVideo ──────────────────────────────

/// Direct port of `EasterEggActivity.fitSurfaceToVideo()`'s letterbox/
/// pillarbox math. Returns `(targetWidth, targetHeight)`. Returns `None`
/// if `video_w`/`video_h` are non-positive, matching the original's
/// `if (videoW <= 0 || videoH <= 0) return` guard (a no-op in Kotlin —
/// here the caller must decide what "no-op" means, since Rust has no
/// implicit "leave the View untouched" concept).
pub fn fit_surface_to_video(
    video_w: i32,
    video_h: i32,
    screen_w: i32,
    screen_h: i32,
) -> Option<(i32, i32)> {
    if video_w <= 0 || video_h <= 0 {
        return None;
    }
    let video_ratio = video_w as f32 / video_h as f32;
    let screen_ratio = screen_w as f32 / screen_h as f32;

    if video_ratio > screen_ratio {
        // Video is relatively wider -> fill full width, black bars top/bottom
        Some((screen_w, (screen_w as f32 / video_ratio) as i32))
    } else {
        // Video is relatively taller -> fill full height, black bars left/right
        Some(((screen_h as f32 * video_ratio) as i32, screen_h))
    }
}

// ── EasterEggActivity.kt swipe-to-exit threshold ─────────────────────────

/// Direct port of the fling-gesture threshold check inside
/// `EasterEggActivity`'s `GestureDetector.SimpleOnGestureListener.onFling`.
/// Distinct from `swipe_direction` above: this only answers "was this
/// fling decisive enough to count as an exit gesture", not which of the
/// four cardinal directions it was.
pub fn is_exit_swipe(
    dx: f32,
    dy: f32,
    velocity_x: f32,
    min_distance_px: f32,
    min_velocity: f32,
) -> bool {
    dx.abs() > dy.abs() && dx.abs() > min_distance_px && velocity_x.abs() > min_velocity
}

#[cfg(test)]
mod easter_egg_tests {
    use super::*;

    #[test]
    fn wider_video_than_screen_fills_width() {
        // 16:9 video on a 9:16 (portrait) screen -> video is relatively
        // much wider than the screen, so width should be maxed out.
        let (w, h) = fit_surface_to_video(1920, 1080, 1080, 1920).unwrap();
        assert_eq!(w, 1080);
        assert!(h < 1920); // letterboxed, not filling the full height
    }

    #[test]
    fn taller_video_than_screen_fills_height() {
        // 9:16 video on a 16:9 (landscape) screen -> video is relatively
        // taller, so height should be maxed out.
        let (w, h) = fit_surface_to_video(1080, 1920, 1920, 1080).unwrap();
        assert_eq!(h, 1080);
        assert!(w < 1920); // pillarboxed
    }

    #[test]
    fn non_positive_dimensions_return_none() {
        assert_eq!(fit_surface_to_video(0, 100, 1080, 1920), None);
        assert_eq!(fit_surface_to_video(100, -5, 1080, 1920), None);
    }

    #[test]
    fn exit_swipe_requires_all_three_conditions() {
        // Passes all three thresholds
        assert!(is_exit_swipe(100.0, 10.0, 300.0, 80.0, 200.0));
        // Fails: not horizontal-dominant (dy > dx)
        assert!(!is_exit_swipe(50.0, 100.0, 300.0, 80.0, 200.0));
        // Fails: below minimum distance
        assert!(!is_exit_swipe(50.0, 10.0, 300.0, 80.0, 200.0));
        // Fails: below minimum velocity
        assert!(!is_exit_swipe(100.0, 10.0, 100.0, 80.0, 200.0));
    }
}

// ── DockLayoutDialog.kt ──────────────────────────────────────────────────

/// One dock-layout preset's full settings bundle — mirrors the `editor.put*`
/// calls inside `DockLayoutDialog`'s `setSingleChoiceItems` callback.
/// Booleans/strings only — Kotlin still owns the actual SharedPreferences
/// writes (a Framework/Context-bound operation this crate has no access to).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockLayoutPreset {
    pub enable_nav: bool,             // enable_nav_back/home/recents (all three, same value)
    pub enable_qs_wifi_vol_date_notif: bool, // enable_qs_wifi/vol/date/notif (all four, same value)
    pub app_menu_fullscreen: bool,
    pub max_running_apps: &'static str,
    pub max_running_apps_landscape: &'static str,
    pub dock_activation_area: &'static str,
    pub activation_method: &'static str,
    pub show_notifications: bool,
    pub enable_qs_pin: bool,
}

/// Direct port of the per-`which` value selection inside
/// `DockLayoutDialog`'s single-choice callback. `which` is the selected
/// list index (0/1/2 in the original's `R.array.layouts`; any other value
/// falls through to the same branch as 2, matching the original's
/// `else ->` branches). `launch_mode` ("standard") and
/// `launch_games_fullscreen` (false) are constant across all presets in
/// the original — Kotlin can set those two directly without calling this.
pub fn dock_layout_preset(which: i32) -> DockLayoutPreset {
    DockLayoutPreset {
        enable_nav: which != 0,
        enable_qs_wifi_vol_date_notif: which != 0,
        app_menu_fullscreen: which != 2,
        max_running_apps: match which {
            0 => "4",
            1 => "10",
            _ => "15",
        },
        max_running_apps_landscape: match which {
            0 => "8",
            1 => "10",
            _ => "15",
        },
        dock_activation_area: if which == 2 { "5" } else { "25" },
        activation_method: if which != 2 { "handle" } else { "swipe" },
        show_notifications: which != 0,
        enable_qs_pin: which != 2,
    }
}

#[cfg(test)]
mod dock_layout_tests {
    use super::*;

    #[test]
    fn preset_0_is_minimal() {
        let p = dock_layout_preset(0);
        assert!(!p.enable_nav);
        assert!(!p.enable_qs_wifi_vol_date_notif);
        assert_eq!(p.max_running_apps, "4");
        assert_eq!(p.dock_activation_area, "25");
        assert_eq!(p.activation_method, "handle");
    }

    #[test]
    fn preset_2_uses_swipe_activation_and_small_area() {
        let p = dock_layout_preset(2);
        assert!(p.enable_nav);
        assert!(!p.app_menu_fullscreen);
        assert_eq!(p.dock_activation_area, "5");
        assert_eq!(p.activation_method, "swipe");
        assert!(!p.enable_qs_pin);
    }

    #[test]
    fn out_of_range_which_falls_through_like_preset_2_branches() {
        // matches the original's `else ->` behavior for unmapped `which` values
        let p = dock_layout_preset(99);
        assert_eq!(p.max_running_apps, "15");
        assert_eq!(p.dock_activation_area, "25"); // NOTE: only `== 2` triggers "5", so 99 -> "25"
    }
}

// ── PerfectServer.kt::isTouchOnView ────────────────────────────────────────

/// Port of `PerfectServer.isTouchOnView`: true if the raw touch point
/// (`event.rawX`/`rawY`, absolute screen coordinates) falls within a view's
/// on-screen bounds. Kotlin still owns the `View.getLocationOnScreen`
/// call and the visibility check (`view.visibility != View.VISIBLE`) since
/// both need the live View; this is only the bounds arithmetic, called at
/// each ACTION_UP to decide whether a status-area tap landed on the
/// Bluetooth/WiFi icon or should fall through to opening the notification
/// panel.
pub fn point_in_view_bounds(
    raw_x: f32,
    raw_y: f32,
    view_screen_x: i32,
    view_screen_y: i32,
    view_width: i32,
    view_height: i32,
) -> bool {
    raw_x >= view_screen_x as f32
        && raw_x <= (view_screen_x + view_width) as f32
        && raw_y >= view_screen_y as f32
        && raw_y <= (view_screen_y + view_height) as f32
}

#[cfg(test)]
mod touch_bounds_tests {
    use super::*;

    #[test]
    fn point_inside_bounds() {
        assert!(point_in_view_bounds(50.0, 50.0, 0, 0, 100, 100));
    }

    #[test]
    fn point_on_edge_is_inside() {
        // original uses <= / >=, so the exact edge counts as a hit
        assert!(point_in_view_bounds(100.0, 100.0, 0, 0, 100, 100));
        assert!(point_in_view_bounds(0.0, 0.0, 0, 0, 100, 100));
    }

    #[test]
    fn point_outside_bounds() {
        assert!(!point_in_view_bounds(150.0, 50.0, 0, 0, 100, 100));
        assert!(!point_in_view_bounds(50.0, -1.0, 0, 0, 100, 100));
    }

    #[test]
    fn point_respects_view_screen_offset() {
        // view positioned at (200, 300) on screen, 50x50
        assert!(point_in_view_bounds(220.0, 320.0, 200, 300, 50, 50));
        assert!(!point_in_view_bounds(100.0, 100.0, 200, 300, 50, 50));
    }
}
