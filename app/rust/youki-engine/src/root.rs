//! root — port of RootManager.kt's compute + process-spawn logic.
//!
//! Per the handoff doc's "POSIX/OS-level calls (not Android Framework)"
//! category: running `su` as a subprocess is a plain POSIX exec, not an
//! Android Binder call, so it moves to Rust wholesale via
//! std::process::Command — no JNI round-trip needed for the actual process
//! spawn/read/write, unlike Shizuku (Binder-bound, stays Kotlin) or
//! `verify_freeform_flags`'s `Settings.Global` read (ContentResolver-bound,
//! stays Kotlin — see jni_bridge::root for the one JNI call that remains).
//!
//! Caching (60s TTL, "Gap 31" in the original comments) and the 10s
//! process.waitFor timeout ("Gap 40") and parallel stdout/stderr reads
//! ("Gap 41") are all preserved with the same reasoning as the original.

use once_cell::sync::Lazy;
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CACHE_TTL: Duration = Duration::from_secs(60);
const EXEC_TIMEOUT: Duration = Duration::from_secs(10);
const CHECK_TIMEOUT: Duration = Duration::from_secs(5);

struct RootCache {
    available: Option<bool>,
    checked_at: Option<Instant>,
}

static CACHE: Lazy<Mutex<RootCache>> = Lazy::new(|| {
    Mutex::new(RootCache {
        available: None,
        checked_at: None,
    })
});

/// Mirrors `alreadyGrantedThisSession` — set once grant_all() completes.
static ALREADY_GRANTED_THIS_SESSION: AtomicBool = AtomicBool::new(false);

/// Diagnostics: last executeRoot() call duration in millis, exposed for the
/// same "Diagnostics screen" use case the original diagnose() served.
static LAST_EXEC_MS: AtomicI64 = AtomicI64::new(-1);

/// Equivalent of `RootManager.isAvailable` — cached for 60s (Gap 31).
pub fn is_available() -> bool {
    let mut cache = CACHE.lock().unwrap();
    if let (Some(available), Some(checked_at)) = (cache.available, cache.checked_at) {
        if checked_at.elapsed() < CACHE_TTL {
            return available;
        }
    }
    let result = check_root();
    cache.available = Some(result);
    cache.checked_at = Some(Instant::now());
    result
}

/// Equivalent of `RootManager.invalidateCache()`.
pub fn invalidate_cache() {
    let mut cache = CACHE.lock().unwrap();
    cache.available = None;
    cache.checked_at = None;
}

fn check_root() -> bool {
    // Magisk/KernelSU presence check — purely informational logging in the
    // original (`log("Magisk/KernelSU directory found")`); the JNI bridge
    // forwards this string up to Kotlin's onLog callback if wired, see
    // jni_bridge::root.
    let magisk_present =
        std::path::Path::new("/data/adb/magisk").exists() || std::path::Path::new("/data/adb/ksu").exists();
    if magisk_present {
        log::info!("Magisk/KernelSU directory found");
    }

    match Command::new("su")
        .arg("-c")
        .arg("id")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(mut child) => {
            // Gap 41 equivalent: read stdout on a helper thread while the
            // main thread drains stderr, avoiding the classic pipe-deadlock
            // when both buffers fill before either is read.
            let mut stdout_pipe = child.stdout.take();
            let stdout_thread = std::thread::spawn(move || {
                let mut buf = String::new();
                if let Some(mut pipe) = stdout_pipe.take() {
                    let _ = pipe.read_to_string(&mut buf);
                }
                buf
            });

            if let Some(mut stderr_pipe) = child.stderr.take() {
                let mut discard = String::new();
                let _ = stderr_pipe.read_to_string(&mut discard);
            }

            let _ = wait_with_timeout(&mut child, CHECK_TIMEOUT);
            let output = stdout_thread.join().unwrap_or_default();

            let granted = output.contains("uid=0");
            if granted {
                log::info!("Root verified (uid=0)");
            } else {
                log::info!("su not root: {output}");
            }
            granted
        }
        Err(_) => {
            const STATIC_PATHS: [&str; 6] = [
                "/sbin/su",
                "/system/sbin/su",
                "/system/bin/su",
                "/system/xbin/su",
                "/su/bin/su",
                "/magisk/.core/bin/su",
            ];
            let found = STATIC_PATHS.iter().any(|p| is_executable(p));
            if found {
                log::info!("Root via static path");
            } else {
                log::info!("No root found");
            }
            found
        }
    }
}

fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// Equivalent of `RootManager.executeRoot()`. Blocking — callers on the
/// Kotlin side are expected to invoke this from a background dispatcher via
/// the JNI bridge, same as the original's `Dispatchers.IO` usage.
pub fn execute_root(command: &str) -> String {
    let started = Instant::now();
    let result = execute_root_inner(command);
    LAST_EXEC_MS.store(started.elapsed().as_millis() as i64, Ordering::Relaxed);
    result
}

fn execute_root_inner(command: &str) -> String {
    let mut child = match Command::new("su")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Root execution error: {e}");
            return "error".to_string();
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = writeln!(stdin, "{command}");
        let _ = writeln!(stdin, "exit");
        // stdin dropped here, closing the pipe — equivalent to os.close().
    }

    // Gap 41 equivalent: parallel stdout/stderr reads to avoid deadlock.
    let mut stdout_pipe = child.stdout.take();
    let stdout_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stdout_pipe.take() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf.trim().to_string()
    });

    let mut stderr_pipe = child.stderr.take();
    let stderr_thread = std::thread::spawn(move || {
        let mut buf = String::new();
        if let Some(mut pipe) = stderr_pipe.take() {
            let _ = pipe.read_to_string(&mut buf);
        }
        buf.trim().to_string()
    });

    // Gap 40 equivalent: 10-second timeout on the process itself.
    let _ = wait_with_timeout(&mut child, EXEC_TIMEOUT);

    let stdout = stdout_thread.join().unwrap_or_default();
    let stderr = stderr_thread.join().unwrap_or_default();

    let mut result = String::new();
    if !stdout.trim().is_empty() {
        result.push_str(&stdout);
    }
    if !stderr.trim().is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(&stderr);
    }
    if result.is_empty() {
        result.push_str("(no output)");
    }
    result
}

/// Polls `child.try_wait()` up to `timeout`, matching the semantics of
/// `Process.waitFor(timeout, unit)` (returns true if the process exited
/// within the timeout, false otherwise — does NOT kill the process on
/// timeout, same as the original, which also never killed it).
fn wait_with_timeout(child: &mut std::process::Child, timeout: Duration) -> bool {
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => return true,
            Ok(None) => {
                if start.elapsed() >= timeout {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => return false,
        }
    }
}

/// Equivalent of `RootManager.diagnose()`.
pub fn diagnose(cmd: &str) -> String {
    if !is_available() {
        return "Root: su not available (isAvailable = false)".to_string();
    }
    let out = execute_root(cmd);
    if out.trim().is_empty() {
        "stdout/stderr:\n(empty)".to_string()
    } else {
        format!("stdout/stderr:\n{out}")
    }
}

/// Equivalent of `RootManager.grantWriteSecureSettings()`. Returns the
/// command output for the JNI bridge to forward to the Kotlin-side
/// `onResult` callback.
pub fn grant_write_secure_settings(package_name: &str) -> String {
    log::info!("Granting WRITE_SECURE_SETTINGS via root to {package_name}");
    execute_root(&format!(
        "pm grant {package_name} android.permission.WRITE_SECURE_SETTINGS"
    ))
}

/// Marks the "auto root grant" as already run this session — mirrors
/// `alreadyGrantedThisSession`. `force` bypasses the check, same as the
/// original's `requestPermission(force = true)`.
pub fn should_run_grant_all(force: bool) -> bool {
    is_available() && (force || !ALREADY_GRANTED_THIS_SESSION.load(Ordering::Relaxed))
}

pub fn mark_grant_all_done() {
    ALREADY_GRANTED_THIS_SESSION.store(true, Ordering::Relaxed);
}

/// Port of `ShizukoManager.buildGrantAllCommands()` — pure string-building
/// logic (no Binder call), shared conceptually between the root and Shizuku
/// grant-all paths. `sdk_int` is passed in from Kotlin (Build.VERSION.SDK_INT
/// has no meaningful Rust-side equivalent — it's a value read from the
/// Android framework, not computed). Verified against the full original
/// function body (ShizukoManager.kt lines 35-84) — an earlier draft of this
/// port only covered the first ~15 commands and is now complete.
pub fn build_grant_all_commands(pkg: &str, sdk_int: i32) -> Vec<String> {
    let notif_service = format!("{pkg}/com.youki.dex.services.NotificationService");
    let admin_rcv = format!("{pkg}/.DeviceAdminReceiver");

    let mut cmds = vec![
        format!("pm grant {pkg} android.permission.WRITE_SECURE_SETTINGS"),
        format!("pm grant {pkg} android.permission.WRITE_SETTINGS"),
        format!("pm grant {pkg} android.permission.PACKAGE_USAGE_STATS"),
        format!("pm grant {pkg} android.permission.REQUEST_INSTALL_PACKAGES"),
        format!("pm grant {pkg} android.permission.MANAGE_USERS"),
        format!("pm grant {pkg} android.permission.CREATE_USERS"),
        format!("pm grant {pkg} android.permission.INTERACT_ACROSS_USERS"),
        format!("pm grant {pkg} android.permission.INTERACT_ACROSS_USERS_FULL"),
    ];

    // Build.VERSION_CODES.TIRAMISU == 33.
    if sdk_int >= 33 {
        cmds.push(format!("pm grant {pkg} android.permission.POST_NOTIFICATIONS"));
        cmds.push(format!("pm grant {pkg} android.permission.READ_MEDIA_IMAGES"));
        cmds.push(format!("pm grant {pkg} android.permission.READ_MEDIA_VIDEO"));
        cmds.push(format!(
            "device_config put privacy_sandbox app_allow_packages_to_use_system_overlay {pkg}"
        ));
    } else {
        cmds.push(format!("pm grant {pkg} android.permission.READ_EXTERNAL_STORAGE"));
        cmds.push(format!("pm grant {pkg} android.permission.WRITE_EXTERNAL_STORAGE"));
    }

    cmds.push(format!("appops set {pkg} SYSTEM_ALERT_WINDOW allow"));
    cmds.push(format!("appops set {pkg} READ_MEDIA_VISUAL_USER_SELECTED allow"));
    cmds.push(format!(
        "settings put secure enabled_notification_listeners {notif_service}"
    ));
    cmds.push(format!("dpm set-active-admin {admin_rcv}"));

    // ── Freeform / desktop windowing — merged in from the original's
    // enableFreeformWindowing(), now part of the single grant-all batch so
    // root OR Shizuku both cover it the instant privilege is obtained.
    cmds.push("settings put global enable_freeform_support 1".to_string());
    cmds.push("settings put global force_desktop_mode_on_external_displays 0".to_string());
    if sdk_int >= 31 {
        cmds.push("wm set-multi-window-config --freeformWindowManagement true".to_string());
    }
    // Android 11+ developer-option equivalent of "Force activities to be
    // resizable" — without this, apps declaring resizeableActivity=false
    // get pushed back to fullscreen even inside a freeform task.
    cmds.push("settings put global development_force_resizable_activities 1".to_string());
    // Android 15 (API 35): Desktop Windowing Mode. Without these, freeform
    // windows on API 35 render without the system caption bar.
    if sdk_int >= 35 {
        cmds.push("wm set-multi-window-config --supportsDesktopWindowing true".to_string());
        cmds.push("wm set-multi-window-config --enableDesktopMode true".to_string());
        // Undocumented but observed AOSP 15/16 desktop-mode developer flag —
        // best-effort extra: harmless no-op if the device doesn't recognize it.
        cmds.push("settings put secure desktop_mode_enabled 1".to_string());
    }

    cmds
}

/// Returns the last execute_root() call duration in milliseconds, or -1 if
/// none has run yet. Diagnostics-only, mirrors the spirit of the original's
/// Diagnostics screen use case.
/// NOTE: not yet called — awaiting its JNI bridge entry point from the
/// Kotlin Diagnostics screen.
#[allow(dead_code)]
pub fn last_exec_millis() -> i64 {
    LAST_EXEC_MS.load(Ordering::Relaxed)
}

// NOTE: `verifyFreeformFlags()` from the original is NOT ported here — it
// reads android.provider.Settings.Global via ContentResolver, a Binder-
// backed Android Framework API with no Rust/NDK equivalent. That function
// stays in Kotlin. See jni_bridge::root for the JNI call shape Rust expects
// Kotlin to invoke after grant_all's commands finish (mirroring the
// original's call order: run commands -> verifyFreeformFlags() -> notify
// grantedListeners).
