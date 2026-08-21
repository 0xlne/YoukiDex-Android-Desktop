//! textparse — regex/parsing/sorting helpers ported from MultiUserManager.kt
//! and AppUtils.kt. All functions here are pure text-in, data-out compute
//! with no Android Framework dependency (no Context, no PackageManager, no
//! Binder) — the actual shell command execution (`runPrivileged`,
//! `ShellManager`/Shizuku calls) stays on the Kotlin/root.rs side; this
//! module only parses the resulting output strings.

use once_cell::sync::Lazy;
use regex::Regex;

// ── MultiUserManager.kt ─────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ParsedUser {
    pub id: i32,
    pub name: String,
    pub is_current: bool,
    pub is_running: bool,
}

// FIX (carried over verbatim from the Kotlin comment): (.*?) non-greedy
// instead of [^:}]+ — supports user names containing ":", since the flags
// segment at the end of the braces never contains ":", so backtracking
// automatically stops at the last ":" before "}". Format examples:
// "UserInfo{10:John:230} running" or "UserInfo{10:Work:Profile:404010}".
static USER_INFO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"UserInfo\{(\d+):(.*?):[^:{}]*\}(\s*running)?").unwrap()
});

/// Port of `MultiUserManager.parseUsers()`. `current_user_id` is passed in
/// from Kotlin (getCurrentUserId() reads android.os.Process.myUserHandle(),
/// an Android Framework call — stays in Kotlin, see handoff doc). Returns
/// users sorted by id ascending, same as the original's `sortedBy { it.id }`.
pub fn parse_users(raw: &str, current_user_id: i32) -> Vec<ParsedUser> {
    let mut result: Vec<ParsedUser> = USER_INFO_RE
        .captures_iter(raw)
        .filter_map(|caps| {
            let id: i32 = caps.get(1)?.as_str().parse().ok()?;
            let name = caps.get(2)?.as_str().trim().to_string();
            let is_running = caps
                .get(3)
                .map(|m| !m.as_str().trim().is_empty())
                .unwrap_or(false);
            Some(ParsedUser {
                id,
                name,
                is_current: id == current_user_id,
                is_running,
            })
        })
        .collect();
    result.sort_by_key(|u| u.id);
    result
}

static CREATED_USER_ID_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"created user id (\d+)").unwrap());

/// Port of the `Regex("""created user id (\d+)""")` used inline in both
/// `createUser()` and `createAndProvisionUser()` — factored into one shared
/// function since both call sites did the exact same extraction.
pub fn extract_created_user_id(raw: &str) -> Option<i32> {
    CREATED_USER_ID_RE
        .captures(raw)?
        .get(1)?
        .as_str()
        .parse()
        .ok()
}

/// Port of `String.isSuccess()` — a private extension function in the
/// original, used to decide whether a root/shell command's output indicates
/// success vs failure by scanning for known failure phrases.
pub fn is_success(output: &str) -> bool {
    let trimmed = output.trim();
    if trimmed.is_empty() || trimmed == "(no output)" {
        return true;
    }
    let lower = trimmed.to_lowercase();
    !lower.contains("exception")
        && !lower.contains("failed:")
        && !lower.contains("unknown command")
        && !lower.contains("no_privilege")
        && !lower.starts_with("error:")
}

static SANITIZE_RE: Lazy<Regex> = Lazy::new(|| {
    // Matches the original's character class exactly:
    // ["'\\`$;|&<>(){}]
    Regex::new(r#"["'\\`$;|&<>(){}]"#).unwrap()
});

/// Port of `String.sanitize()` — strips shell-metacharacters before a value
/// (e.g. a user-supplied profile name) is interpolated into a shell command
/// string, then truncates to 64 chars. SECURITY-RELEVANT: this is the one
/// thing standing between a user-entered name and shell injection via
/// `runPrivileged(context, "pm create-user \"$safeName\"")` — the character
/// class must match the original exactly, not be "improved"/narrowed or
/// widened without re-auditing every call site that relies on it.
pub fn sanitize(input: &str) -> String {
    let cleaned = SANITIZE_RE.replace_all(input, "");
    cleaned.chars().take(64).collect()
}

// ── AppUtils.kt ──────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct ParsedTaskEntry {
    pub task_id: i32,
    pub package_name: String,
}

static TASK_ID_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"taskId=(\d+)").unwrap());
static PKG_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"baseIntent=([a-zA-Z0-9_.]+)/").unwrap());

/// Port of the `am stack list` output-parsing loop inside AppUtils.kt's
/// task-listing function. Parses lines like:
/// `taskId=42 bounds=[...] running=true visible=true baseIntent=com.pkg/.Activity`
///
/// Excludes `self_pkg`/`launcher_pkg`/anything containing "systemui", and
/// de-duplicates by package name — same filtering as the original. Returns
/// at most `max` entries, same early-break behavior (`if (result.size >=
/// max) break`).
///
/// NOTE: the actual `am stack list` shell invocation
/// (`ShellManager`/Shizuku) stays in Kotlin (Binder/root-bound — see
/// handoff doc); this function only parses the resulting text. The
/// subsequent PackageManager lookups (getLaunchIntentForPackage,
/// getActivityInfo, loadLabel) in the original are Android Framework calls
/// that also stay in Kotlin, applied to the package names this function
/// returns.
pub fn parse_task_list(output: &str, self_pkg: &str, launcher_pkg: &str, max: usize) -> Vec<ParsedTaskEntry> {
    let mut result = Vec::new();
    let mut seen_pkgs: std::collections::HashSet<String> = std::collections::HashSet::new();

    for line in output.lines() {
        if result.len() >= max {
            break;
        }
        let Some(task_id_match) = TASK_ID_RE.captures(line) else {
            continue;
        };
        let Some(task_id) = task_id_match.get(1).and_then(|m| m.as_str().parse::<i32>().ok()) else {
            continue;
        };
        let Some(pkg_match) = PKG_RE.captures(line) else {
            continue;
        };
        let Some(pkg) = pkg_match.get(1).map(|m| m.as_str().to_string()) else {
            continue;
        };

        if pkg == self_pkg || pkg == launcher_pkg || pkg.contains("systemui") {
            continue;
        }
        if seen_pkgs.contains(&pkg) {
            continue;
        }
        seen_pkgs.insert(pkg.clone());

        result.push(ParsedTaskEntry {
            task_id,
            package_name: pkg,
        });
    }

    result
}

/// Case-*sensitive* alphabetical sort by label — matches
/// `apps.sortedWith(compareBy { it.name })` and
/// `appsInfo.sortedWith(compareBy { it.label.toString() })` in
/// AppUtils.kt's `getInstalledPackages()`/`getInstalledApps()`, both of
/// which use Kotlin's default String `compareTo` (case-sensitive,
/// codepoint-order) rather than a locale-aware or case-folded comparison.
/// Kept as a separate function from `sort_by_label_alphabetical` above
/// (which is case-*insensitive*, matching a different call site —
/// MultiUserManager's user list) rather than unifying them, since the two
/// call sites have always had genuinely different sort semantics and
/// forcing one shared function would silently change one of their
/// behaviors.
pub fn sort_by_label_case_sensitive(mut items: Vec<(String, i32)>) -> Vec<(String, i32)> {
    items.sort_by(|a, b| a.0.cmp(&b.0));
    items
}

/// Generic case-insensitive alphabetical sort by a caller-provided label,
/// covering the pattern used in `apps.sortedWith(compareBy { it.name })`
/// and `appsInfo.sortedWith(compareBy { it.label.toString() })`. Takes
/// ownership of the input Vec and returns it sorted, matching Kotlin's
/// `sortedWith` (returns a new sorted list rather than mutating in place —
/// same semantics preserved here even though Rust's `sort_by_key` mutates,
/// since the Vec is consumed and returned).
///
/// NOTE: the actual app label/name data still comes from PackageManager on
/// the Kotlin side (an Android Framework call) — this function only sorts
/// the already-resolved label strings the JNI bridge passes in.
pub fn sort_by_label_alphabetical(mut items: Vec<(String, i32)>) -> Vec<(String, i32)> {
    // Each tuple is (label, opaque_id) — opaque_id lets the JNI bridge
    // re-associate the sorted order back to whatever app/task object it
    // came from on the Kotlin side, without this function needing to know
    // anything about App/AppInfo types.
    items.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));
    items
}

/// Port of the `sortedWith(compareByDescending { it.lastTimeUsed })` usage
/// stat sort. Descending order, same as the original.
pub fn sort_by_last_used_descending(mut items: Vec<(i64, i32)>) -> Vec<(i64, i32)> {
    // Each tuple is (last_time_used, opaque_id), same "opaque id" pattern
    // as sort_by_label_alphabetical above.
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items
}

// ── MultiUserManager.kt::provisionUser ──────────────────────────────────

/// One provisioning step: the shell command to run, and a human-readable
/// label for progress reporting / the result log. Mirrors the
/// `Pair<String, String>` entries in the Kotlin original's `steps` list.
#[derive(Clone, Debug)]
pub struct ProvisionStep {
    pub command: String,
    pub label: String,
}

/// Port of `provisionUser()`'s `steps` list construction — pure string
/// formatting, no Android Framework dependency. `pkg` is the app's package
/// name (`Context.packageName` — a Framework call, so Kotlin passes it
/// in). `service_component` and `admin_receiver_component` are passed in
/// pre-built by Kotlin too, since the original built them as
/// `"$pkg/com.youki.dex.services.DockService"` and `"$pkg/.DeviceAdminReceiver"`
/// — trivial string concatenation the caller can just as easily do, kept
/// as parameters here rather than hardcoding this crate's own copy of
/// those class-name string literals (avoids two places needing to agree
/// on the DockService/DeviceAdminReceiver class names staying in sync).
///
/// Labels that were `context.getString(R.string.mu_provision_step_*, ...)`
/// resource lookups in the original (an Android Framework/resources call)
/// are returned here as the same fixed English fallback strings the
/// original used for the *non*-localized steps (e.g. "WRITE_SECURE_SETTINGS")
/// — the three steps that used a *localized* string resource
/// (mu_provision_step_install_app, _install_shizuku, _accessibility) are
/// flagged below with their original resource id in a comment; Kotlin
/// should replace those three specific labels with
/// `context.getString(...)` after calling this, if localization of those
/// three specific labels still matters — this function itself has no
/// access to Android string resources to do that substitution.
pub fn build_provision_steps(pkg: &str, user_id: i32) -> Vec<ProvisionStep> {
    let service = format!("{pkg}/com.youki.dex.services.DockService");
    let admin_rcv = format!("{pkg}/.DeviceAdminReceiver");

    vec![
        ProvisionStep {
            command: format!("pm install-existing --user {user_id} {pkg}"),
            label: "Install app".to_string(), // original: R.string.mu_provision_step_install_app
        },
        ProvisionStep {
            command: format!("pm install-existing --user {user_id} moe.shizuku.privileged.api"),
            label: "Install Shizuku".to_string(), // original: R.string.mu_provision_step_install_shizuku
        },
        ProvisionStep {
            command: format!("pm grant --user {user_id} {pkg} android.permission.WRITE_SECURE_SETTINGS"),
            label: "WRITE_SECURE_SETTINGS".to_string(),
        },
        ProvisionStep {
            command: format!("pm grant --user {user_id} {pkg} android.permission.WRITE_SETTINGS"),
            label: "WRITE_SETTINGS".to_string(),
        },
        ProvisionStep {
            command: format!("pm grant --user {user_id} {pkg} android.permission.POST_NOTIFICATIONS"),
            label: "POST_NOTIFICATIONS".to_string(),
        },
        ProvisionStep {
            command: format!("pm grant --user {user_id} {pkg} android.permission.PACKAGE_USAGE_STATS"),
            label: "PACKAGE_USAGE_STATS".to_string(),
        },
        ProvisionStep {
            command: format!("pm grant --user {user_id} {pkg} android.permission.READ_MEDIA_IMAGES"),
            label: "READ_MEDIA_IMAGES".to_string(),
        },
        ProvisionStep {
            command: format!("pm grant --user {user_id} {pkg} android.permission.READ_MEDIA_VIDEO"),
            label: "READ_MEDIA_VIDEO".to_string(),
        },
        ProvisionStep {
            command: format!("appops set --user {user_id} {pkg} SYSTEM_ALERT_WINDOW allow"),
            label: "SYSTEM_ALERT_WINDOW".to_string(),
        },
        ProvisionStep {
            command: format!("settings put --user {user_id} secure enabled_accessibility_services {service}"),
            label: "Accessibility Service".to_string(),
        },
        ProvisionStep {
            command: format!("settings put --user {user_id} secure accessibility_enabled 1"),
            label: "Enable accessibility".to_string(), // original: R.string.mu_provision_step_accessibility
        },
        ProvisionStep {
            command: format!("settings put --user {user_id} secure enabled_notification_listeners {service}"),
            label: "Notification Listener".to_string(),
        },
        ProvisionStep {
            command: format!("dpm set-active-admin --user {user_id} {admin_rcv}"),
            label: "Device Admin".to_string(),
        },
        ProvisionStep {
            command: format!("appops set --user {user_id} {pkg} REQUEST_INSTALL_PACKAGES allow"),
            label: "REQUEST_INSTALL_PACKAGES".to_string(),
        },
    ]
}

#[cfg(test)]
mod provision_tests {
    use super::*;

    #[test]
    fn builds_expected_step_count_and_order() {
        let steps = build_provision_steps("com.youki.dex", 10);
        assert_eq!(steps.len(), 14);
        assert_eq!(steps[0].command, "pm install-existing --user 10 com.youki.dex");
        assert_eq!(steps[0].label, "Install app");
        assert_eq!(
            steps[12].command,
            "dpm set-active-admin --user 10 com.youki.dex/.DeviceAdminReceiver"
        );
    }

    #[test]
    fn service_component_used_in_both_accessibility_and_notification_steps() {
        let steps = build_provision_steps("com.youki.dex", 5);
        let service = "com.youki.dex/com.youki.dex.services.DockService";
        assert!(steps[9].command.contains(service)); // accessibility_services
        assert!(steps[11].command.contains(service)); // notification_listeners
    }
}

// ── Utils.kt::getBatteryDrawable ────────────────────────────────────────

/// Port of `Utils.getBatteryDrawable()`'s *classification* logic only —
/// the original returned an `R.drawable.*` resource id directly, but
/// resource ids are generated at Kotlin/Android build time and have no
/// meaningful Rust-side representation (this crate can't reference `R`).
/// So this function returns a bucket name instead; Kotlin maps that name
/// to the actual `R.drawable.battery_*` / `R.drawable.battery_charging_*`
/// constant. The bucket boundaries (0, 1-29, 30-49, 50-59, 60-79, 80-89,
/// 90-99, 100) and the charging/not-charging split are copied exactly
/// from the original's `when` expression.
///
/// Returns one of: "empty", "20", "30", "50", "60", "80", "90", "full" —
/// Kotlin prefixes with "battery_" or "battery_charging_" as appropriate.
pub fn battery_drawable_bucket(level: i32, plugged: bool) -> &'static str {
    let _ = plugged; // bucket boundaries are identical charging vs not; only the Kotlin-side prefix differs
    match level {
        0 => "empty",
        1..=29 => "20",
        30..=49 => "30",
        50..=59 => "50",
        60..=79 => "60",
        80..=89 => "80",
        90..=99 => "90",
        _ => "full", // covers 100 and any out-of-range value >99, same as the original's `else`
    }
}

#[cfg(test)]
mod battery_tests {
    use super::*;

    #[test]
    fn boundaries_match_original_when_expression() {
        assert_eq!(battery_drawable_bucket(0, false), "empty");
        assert_eq!(battery_drawable_bucket(1, false), "20");
        assert_eq!(battery_drawable_bucket(29, false), "20");
        assert_eq!(battery_drawable_bucket(30, false), "30");
        assert_eq!(battery_drawable_bucket(59, false), "50");
        assert_eq!(battery_drawable_bucket(60, false), "60");
        assert_eq!(battery_drawable_bucket(89, false), "80");
        assert_eq!(battery_drawable_bucket(90, false), "90");
        assert_eq!(battery_drawable_bucket(99, false), "90");
        assert_eq!(battery_drawable_bucket(100, false), "full");
    }

    #[test]
    fn plugged_flag_does_not_change_bucket_boundaries() {
        assert_eq!(battery_drawable_bucket(45, true), battery_drawable_bucket(45, false));
    }
}

// ── ShizukoManager.kt::getForegroundPackage / getRunningPackages ────────

// Same regex the original used: matches "u0 <package.name>/" fragments in
// `dumpsys activity activities` output (the `u0` is the user id prefix
// dumpsys prints before each component name).
static RESUMED_ACTIVITY_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"u0\s+([\w.]+)/").unwrap()
});

/// Port of `ShizukoManager.getForegroundPackage()`'s parsing half. `raw`
/// is the output of `dumpsys activity activities | grep mResumedActivity`
/// (Kotlin still runs that shell command itself via ShizukoManager/
/// RootManager — this function only parses the resulting text). Returns
/// the first captured package name, or None if the pattern doesn't match
/// — same as the original's `?.groupValues?.get(1)`.
pub fn parse_foreground_package(raw: &str) -> Option<String> {
    RESUMED_ACTIVITY_RE
        .captures(raw)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}

/// Port of `ShizukoManager.getRunningPackages()`'s parsing half. `raw` is
/// the output of `dumpsys activity activities | grep 'Run #'`. Returns
/// distinct package names in first-seen order, matching the original's
/// `.distinct().toList()` (Kotlin's `distinct()` preserves first-seen
/// order, same as the IndexSet-like dedup below).
pub fn parse_running_packages(raw: &str) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for caps in RESUMED_ACTIVITY_RE.captures_iter(raw) {
        if let Some(m) = caps.get(1) {
            let pkg = m.as_str().to_string();
            if seen.insert(pkg.clone()) {
                result.push(pkg);
            }
        }
    }
    result
}

#[cfg(test)]
mod dumpsys_tests {
    use super::*;

    #[test]
    fn parses_single_resumed_activity() {
        let raw = "    mResumedActivity: ActivityRecord{abc u0 com.youki.dex/.MainActivity t5}";
        assert_eq!(parse_foreground_package(raw), Some("com.youki.dex".to_string()));
    }

    #[test]
    fn returns_none_when_no_match() {
        assert_eq!(parse_foreground_package("no match here"), None);
    }

    #[test]
    fn running_packages_deduplicates_preserving_order() {
        let raw = "\
            Run #3: ActivityRecord{1 u0 com.a/.Main t1}\n\
            Run #2: ActivityRecord{2 u0 com.b/.Main t2}\n\
            Run #1: ActivityRecord{3 u0 com.a/.Main t1}\n\
        ";
        assert_eq!(
            parse_running_packages(raw),
            vec!["com.a".to_string(), "com.b".to_string()]
        );
    }
}

// ── FontManager.kt ─────────────────────────────────────────────────────

/// Direct port of `FontManager.containsArabic()`. Checks each character's
/// Unicode codepoint against the same five ranges as the original: Arabic,
/// Arabic Supplement, Arabic Extended-A, Arabic Presentation Forms-A,
/// Arabic Presentation Forms-B.
pub fn contains_arabic(text: &str) -> bool {
    text.chars().any(|c| {
        let cp = c as u32;
        (0x0600..=0x06FF).contains(&cp)
            || (0x0750..=0x077F).contains(&cp)
            || (0x08A0..=0x08FF).contains(&cp)
            || (0xFB50..=0xFDFF).contains(&cp)
            || (0xFE70..=0xFEFF).contains(&cp)
    })
}

/// Mirrors which of the two typefaces `chooseFontForText()` picked, since
/// this crate has no `Typeface` type — Kotlin still owns the actual
/// Typeface objects and just needs to know which one (if either) to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontChoice {
    Arabic,
    Latin,
    None,
}

/// Direct port of `FontManager.chooseFontForText()`'s decision logic.
/// `has_arabic_tf`/`has_latin_tf` stand in for the original's nullable
/// `Typeface?` parameters — Kotlin passes whether each is non-null.
pub fn choose_font_for_text(text: &str, has_arabic_tf: bool, has_latin_tf: bool) -> FontChoice {
    if text.is_empty() {
        // Empty text -> whichever single font is available, if any
        return if has_arabic_tf {
            FontChoice::Arabic
        } else if has_latin_tf {
            FontChoice::Latin
        } else {
            FontChoice::None
        };
    }
    let is_arabic = contains_arabic(text);
    if is_arabic && has_arabic_tf {
        FontChoice::Arabic
    } else if !is_arabic && has_latin_tf {
        FontChoice::Latin
    } else if has_arabic_tf {
        FontChoice::Arabic
    } else if has_latin_tf {
        FontChoice::Latin
    } else {
        FontChoice::None
    }
}

#[cfg(test)]
mod font_tests {
    use super::*;

    #[test]
    fn detects_arabic_text() {
        assert!(contains_arabic("مرحبا"));
        assert!(!contains_arabic("hello"));
    }

    #[test]
    fn mixed_text_counts_as_arabic() {
        // original uses `.any { }` — a single Arabic char anywhere is enough
        assert!(contains_arabic("hello مرحبا"));
    }

    #[test]
    fn empty_text_picks_arabic_first_if_available() {
        assert_eq!(choose_font_for_text("", true, true), FontChoice::Arabic);
        assert_eq!(choose_font_for_text("", false, true), FontChoice::Latin);
        assert_eq!(choose_font_for_text("", false, false), FontChoice::None);
    }

    #[test]
    fn arabic_text_prefers_arabic_font_falls_back_to_latin() {
        assert_eq!(choose_font_for_text("مرحبا", true, true), FontChoice::Arabic);
        assert_eq!(choose_font_for_text("مرحبا", false, true), FontChoice::Latin);
        assert_eq!(choose_font_for_text("مرحبا", false, false), FontChoice::None);
    }

    #[test]
    fn latin_text_prefers_latin_font_falls_back_to_arabic() {
        assert_eq!(choose_font_for_text("hello", true, true), FontChoice::Latin);
        assert_eq!(choose_font_for_text("hello", true, false), FontChoice::Arabic);
        assert_eq!(choose_font_for_text("hello", false, false), FontChoice::None);
    }
}

// ── WorkshopSourcesActivity.kt ────────────────────────────────────────────
// This logic was duplicated verbatim across showAddDialog() and
// editSource() in the original — moving it here also fixes that
// duplication (one implementation instead of two copies that could drift).

/// Direct port of the `if (url.isNotBlank() && !url.startsWith("http")) url = "https://$url"`
/// normalization step, applied before validation/template-building.
pub fn normalize_source_url(url: &str) -> String {
    let trimmed = url.trim();
    if !trimmed.is_empty() && !trimmed.starts_with("http") {
        format!("https://{trimmed}")
    } else {
        trimmed.to_string()
    }
}

/// Direct port of the `searchUrl = when { ... }` template-building logic
/// from both showAddDialog() and editSource(). Expects an already-
/// normalized URL (see [normalize_source_url]).
pub fn build_search_url_template(url: &str) -> String {
    if url.contains("%s") {
        url.to_string()
    } else if url.contains('?') {
        format!("{url}&q=%s")
    } else {
        format!("{url}?q=%s")
    }
}

/// Validation-error reasons, mirroring the three `if (...) { snack(...); return }`
/// guard checks in showAddDialog()/editSource(), in the same check order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceValidationError {
    NameBlank,
    UrlInvalid,
    TypeBlank,
}

/// Direct port of the three validation guards, run in the original's
/// exact order (name, then url, then type) so the first failure reported
/// matches what the original would have shown first. `name`/`url` should
/// already be trimmed; `url` should already be normalized via
/// [normalize_source_url] before calling this (matching the original,
/// which normalizes before validating).
pub fn validate_source_fields(name: &str, url: &str, final_type: &str) -> Option<SourceValidationError> {
    if name.trim().is_empty() {
        return Some(SourceValidationError::NameBlank);
    }
    if url.trim().is_empty() || !url.starts_with("http") {
        return Some(SourceValidationError::UrlInvalid);
    }
    if final_type.trim().is_empty() {
        return Some(SourceValidationError::TypeBlank);
    }
    None
}

#[cfg(test)]
mod workshop_source_tests {
    use super::*;

    #[test]
    fn normalizes_bare_domain_to_https() {
        assert_eq!(normalize_source_url("example.com"), "https://example.com");
    }

    #[test]
    fn leaves_already_prefixed_url_untouched() {
        assert_eq!(normalize_source_url("http://example.com"), "http://example.com");
        assert_eq!(normalize_source_url("https://example.com"), "https://example.com");
    }

    #[test]
    fn blank_url_stays_blank() {
        assert_eq!(normalize_source_url("   "), "");
    }

    #[test]
    fn search_template_keeps_existing_placeholder() {
        assert_eq!(
            build_search_url_template("https://example.com/search?q=%s"),
            "https://example.com/search?q=%s"
        );
    }

    #[test]
    fn search_template_appends_to_existing_query() {
        assert_eq!(
            build_search_url_template("https://example.com?type=wallpaper"),
            "https://example.com?type=wallpaper&q=%s"
        );
    }

    #[test]
    fn search_template_adds_new_query() {
        assert_eq!(
            build_search_url_template("https://example.com"),
            "https://example.com?q=%s"
        );
    }

    #[test]
    fn validation_checks_name_first() {
        assert_eq!(
            validate_source_fields("", "https://x.com", "WALLPAPER"),
            Some(SourceValidationError::NameBlank)
        );
    }

    #[test]
    fn validation_checks_url_second() {
        assert_eq!(
            validate_source_fields("My Source", "not-a-url", "WALLPAPER"),
            Some(SourceValidationError::UrlInvalid)
        );
        assert_eq!(
            validate_source_fields("My Source", "", "WALLPAPER"),
            Some(SourceValidationError::UrlInvalid)
        );
    }

    #[test]
    fn validation_checks_type_third() {
        assert_eq!(
            validate_source_fields("My Source", "https://x.com", ""),
            Some(SourceValidationError::TypeBlank)
        );
    }

    #[test]
    fn validation_passes_with_all_fields_valid() {
        assert_eq!(
            validate_source_fields("My Source", "https://x.com", "WALLPAPER"),
            None
        );
    }
}

// ── fragments/ batch 1 ──────────────────────────────────────────────────
// Pure string/number logic pulled out of fragments/*.kt during the first
// pass over that directory. Each function below notes its Kotlin origin.
// Framework-bound glue (Preference wiring, dialogs, RecyclerView adapters,
// SharedPreferences reads/writes, File I/O) stays in Kotlin — only the
// parsing/formatting/decision logic that doesn't touch any Android API moved.

// ── DesktopPreferences.kt ──────────────────────────────────────────────

/// Port of the inline parsing in `DesktopPreferences.onCreatePreferences`'s
/// `desktop_grid_preset` ListPreference change listener: splits a
/// "<cols>x<rows>" string (e.g. "4x6") into (cols, rows), falling back to
/// the given defaults for a missing/non-numeric part on either side.
pub fn parse_grid_preset(value: &str, default_cols: i32, default_rows: i32) -> (i32, i32) {
    let mut parts = value.split('x');
    let cols = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(default_cols);
    let rows = parts
        .next()
        .and_then(|s| s.parse::<i32>().ok())
        .unwrap_or(default_rows);
    (cols, rows)
}

// ── AdvancedPreferences.kt::showCustomResolutionDialog ───────────────────

/// Port of the free-text resolution parser used by both
/// `AdvancedPreferences.showCustomResolutionDialog` (applies immediately)
/// and `AppearancePreferences.showCustomResInput` (stores the raw string —
/// that call site does no parsing itself, so only the Advanced one needed
/// this). Accepts "1920x1080" or "1920×1080" (either separator,
/// case-insensitive on "x"), trims whitespace around each number, and
/// requires both parts to be positive integers. Returns None for anything
/// else (wrong part count, non-numeric, zero/negative).
pub fn parse_resolution_string(text: &str) -> Option<(i32, i32)> {
    let parts: Vec<&str> = text.split(['x', 'X', '×']).collect();
    if parts.len() != 2 {
        return None;
    }
    let w: i32 = parts[0].trim().parse().ok()?;
    let h: i32 = parts[1].trim().parse().ok()?;
    if w > 0 && h > 0 {
        Some((w, h))
    } else {
        None
    }
}

/// Port of the `naturalW`/`naturalH` swap in `AdvancedPreferences.applyDisplaySize`.
/// `setForcedDisplaySize` expects natural (portrait) coordinates, but users
/// type PC-style landscape values (wide × tall), so the smaller dimension
/// is always the natural width and the larger is always the natural height.
pub fn natural_display_size(width: i32, height: i32) -> (i32, i32) {
    (width.min(height), width.max(height))
}

// ── InternalFilePickerFragment.kt / FileManagerFragment.kt ───────────────
// Both fragments carry their own copy of an extension→display-icon map and
// (the picker only) an extension→MIME map + wildcard MIME matcher. Folded
// into one shared implementation here; FileManagerFragment's `iconForExt`
// and InternalFilePickerFragment's icon set were checked and differ only
// in a couple of extra extensions (avi, m4a, bmp, svg, csv on the Manager
// side) — the union is used below so both call sites get the same coverage
// they already effectively wanted.

/// Which built-in icon bucket a file extension falls into. Kotlin maps each
/// variant to its own `R.drawable.ic_*` constant (that mapping needs the R
/// class, so it stays on the Kotlin side — this function only makes the
/// classification decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileIconKind {
    Video,
    Audio,
    Image,
    Pdf,
    Archive,
    Font,
    Apk,
    Text,
    Generic,
}

pub fn file_icon_kind(ext: &str) -> FileIconKind {
    match ext.to_lowercase().as_str() {
        "mp4" | "mkv" | "webm" | "mov" | "3gp" | "avi" => FileIconKind::Video,
        "mp3" | "flac" | "aac" | "ogg" | "wav" | "m4a" => FileIconKind::Audio,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" => FileIconKind::Image,
        "pdf" => FileIconKind::Pdf,
        "zip" | "rar" | "7z" | "tar" | "gz" => FileIconKind::Archive,
        "ttf" | "otf" | "woff" | "woff2" => FileIconKind::Font,
        "apk" => FileIconKind::Apk,
        "txt" | "log" | "md" | "json" | "xml" | "csv" => FileIconKind::Text,
        _ => FileIconKind::Generic,
    }
}

/// Port of `InternalFilePickerFragment.extToMime`.
pub fn ext_to_mime(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "mkv" => "video/x-matroska",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "apk" => "application/vnd.android.package-archive",
        "json" => "application/json",
        "xml" => "application/xml",
        "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}

/// Port of `InternalFilePickerFragment.matchesMime`. `allowed_mimes`
/// mirrors the fragment's `allowedMimes: List<String>` (e.g. `["image/*"]`
/// or `["*/*"]`); `file_ext` is the candidate file's extension (no dot).
pub fn mime_matches(allowed_mimes: &[&str], file_ext: &str) -> bool {
    if allowed_mimes.iter().any(|&a| a == "*/*") {
        return true;
    }
    let file_mime = ext_to_mime(file_ext);
    allowed_mimes.iter().any(|&allowed| {
        if allowed == file_mime {
            true
        } else if let Some(prefix) = allowed.strip_suffix("/*") {
            file_mime.starts_with(prefix)
        } else {
            false
        }
    })
}

// ── FileManagerFragment.kt::sortFiles ─────────────────────────────────────

/// Mirrors `FileManagerFragment.SortMode`. Kotlin keeps its own `File`
/// objects and calls `.length()`/`.lastModified()` directly since those are
/// filesystem calls this crate has no business making; this enum only
/// carries the *choice* of comparator across the boundary if ever needed,
/// and `sort_key_index` documents the comparator order for parity checks.
/// NOTE: not yet constructed from the JNI bridge — kept for the parity
/// documentation above and for when a sort-mode value actually needs to
/// cross the boundary.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortMode {
    NameAsc,
    NameDesc,
    SizeAsc,
    SizeDesc,
    DateAsc,
    DateDesc,
}

/// Port of `FileManagerFragment.fmtTime` (audio player's elapsed/duration
/// label). Formats milliseconds as "m:ss" (seconds zero-padded, minutes
/// not) — e.g. 65000 -> "1:05".
pub fn format_duration_mmss(ms: i32) -> String {
    let total_seconds = ms / 1000;
    let minutes = total_seconds / 60;
    let seconds = total_seconds % 60;
    format!("{minutes}:{seconds:02}")
}

// ── SoundsPreferences.kt::buildChmodCommands ──────────────────────────────

/// Port of `SoundsPreferences.buildChmodCommands`. Builds the chmod script
/// making a copied sound file readable by SystemUI (which runs as the
/// "system" user and needs execute on each parent dir plus read on the
/// file itself). Falls back to a single `chmod 644` on the file when the
/// path doesn't have at least three parent directories, matching the
/// original's early-return behavior for `?: return`.
pub fn build_chmod_commands(file_path: &str) -> String {
    let single = format!("chmod 644 \"{file_path}\"");
    let sounds_dir = match file_path.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent,
        _ => return single,
    };
    let files_dir = match sounds_dir.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent,
        _ => return single,
    };
    let data_dir = match files_dir.rsplit_once('/') {
        Some((parent, _)) if !parent.is_empty() => parent,
        _ => return single,
    };
    format!(
        "chmod 711 \"{data_dir}\"\nchmod 711 \"{files_dir}\"\nchmod 711 \"{sounds_dir}\"\nchmod 644 \"{file_path}\""
    )
}

// ── HelpAboutPreferences.kt::handleVersionTap ─────────────────────────────

/// Port of the tap-counter state machine in
/// `HelpAboutPreferences.handleVersionTap`: a fixed window from the *first*
/// tap (not a sliding window), so slow tapping never accumulates — if the
/// window since the first tap has expired, the count restarts at 1 instead
/// of incrementing. Kotlin owns `now`/`System.currentTimeMillis()` (a
/// platform call) and the resulting side effects (launching the Easter
/// Egg, flipping the dev-mode preference); this function only decides the
/// next (count, first_tap_at, unlocked) state.
///
/// Returns `(new_count, new_first_tap_at, reached_threshold)`.
pub fn easter_egg_tap_state(
    now_ms: i64,
    count: i32,
    first_tap_at_ms: i64,
    window_ms: i64,
    taps_required: i32,
) -> (i32, i64, bool) {
    let (mut new_count, new_first_tap_at) = if count == 0 || now_ms - first_tap_at_ms > window_ms
    {
        (1, now_ms)
    } else {
        (count + 1, first_tap_at_ms)
    };
    let reached = new_count >= taps_required;
    if reached {
        new_count = 0;
    }
    (new_count, new_first_tap_at, reached)
}

// ── WorkshopFragment.kt ────────────────────────────────────────────────

/// Port of `WorkshopFragment.isAdUrl` + its `AD_DOMAINS` list — the ad/
/// tracker domain blocklist used by the in-app WebView's ad blocker.
const AD_DOMAINS: &[&str] = &[
    "googlesyndication.com",
    "doubleclick.net",
    "googletagmanager.com",
    "googletagservices.com",
    "adservice.google.com",
    "pagead2.googlesyndication.com",
    "taboola.com",
    "outbrain.com",
    "popads.net",
    "popcash.net",
    "propellerads.com",
    "adsterra.com",
    "exoclick.com",
    "trafficjunky.net",
    "juicyads.com",
    "adskeeper.co.uk",
    "mgid.com",
    "revcontent.com",
    "onesignal.com",
    "pushcrew.com",
    "pushassist.com",
    "izooto.com",
    "ad.plus",
    "ads.yahoo.com",
    "adsymptotic.com",
    "criteo.com",
    "smartadserver.com",
    "cdn.syndication.twimg.com",
];

pub fn is_ad_url(url: &str) -> bool {
    let lower = url.to_lowercase();
    AD_DOMAINS.iter().any(|d| lower.contains(d))
}

/// Port of `WorkshopFragment.mimeToExt` (download-handler's MIME→extension
/// guess — deliberately narrower/looser than `ext_to_mime`'s reverse
/// mapping above, matched by substring rather than exact value, so kept as
/// its own function rather than merged).
pub fn mime_to_ext(mime: &str) -> &'static str {
    if mime.contains("mp4") {
        "mp4"
    } else if mime.contains("webm") {
        "webm"
    } else if mime.contains("quicktime") {
        "mov"
    } else if mime.contains("zip") {
        "zip"
    } else if mime.contains("ttf") || mime.contains("font") {
        "ttf"
    } else if mime.contains("otf") {
        "otf"
    } else {
        ""
    }
}

/// Port of `WorkshopFragment.nameFromCd` — extracts a human-readable name
/// from an HTTP `Content-Disposition` header value, e.g.
/// `attachment; filename="my_video.mp4"` -> "my video". Returns an empty
/// string if there's no `filename=` segment, matching the original.
pub fn name_from_content_disposition(cd: &str) -> String {
    let lower = cd.to_lowercase();
    if !lower.contains("filename=") {
        return String::new();
    }
    let after = match cd.split_once("filename=") {
        Some((_, rest)) => rest,
        None => return String::new(),
    };
    let trimmed = after.trim_matches(|c| c == '"' || c == ' ' || c == '\'');
    let before_semi = trimmed.split(';').next().unwrap_or(trimmed);
    let after_last_slash = before_semi
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .unwrap_or(before_semi);
    let before_last_dot = after_last_slash
        .rsplit_once('.')
        .map(|(head, _)| head)
        .unwrap_or(after_last_slash);
    let mut result = String::with_capacity(before_last_dot.len());
    let mut last_was_sep = false;
    for c in before_last_dot.chars() {
        if c == '_' || c == '-' {
            if !last_was_sep {
                result.push(' ');
            }
            last_was_sep = true;
        } else {
            result.push(c);
            last_was_sep = false;
        }
    }
    result.trim().to_string()
}

/// Port of `WorkshopFragment.fmtSize` — formats a byte count as
/// "B"/"KB"/"MB" with the same rounding the original's `DecimalFormat`
/// patterns produce ("#.#" for KB — one decimal, trailing zero dropped;
/// "#.##" for MB — up to two decimals, trailing zeros dropped).
pub fn format_file_size(bytes: i64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{} KB", format_trimmed(kb, 1));
    }
    let mb = kb / 1024.0;
    format!("{} MB", format_trimmed(mb, 2))
}

/// Rounds `value` to `decimals` places and drops trailing zeros (and a
/// trailing '.' if nothing follows it), matching Java `DecimalFormat`'s
/// "#.#"/"#.##" pattern behavior (as opposed to Rust's `{:.1}` which always
/// keeps the fixed number of decimal places).
fn format_trimmed(value: f64, decimals: usize) -> String {
    let factor = 10f64.powi(decimals as i32);
    let rounded = (value * factor).round() / factor;
    let s = format!("{rounded:.decimals$}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.is_empty() || s == "-" {
        "0".to_string()
    } else {
        s
    }
}

/// Port of the sanitizing half of `WorkshopFragment.uniqueFile`: strips
/// filesystem-illegal characters from a proposed base filename, trims
/// whitespace, caps the length at 80 chars, and falls back to "file" if
/// that leaves nothing. The actual collision-suffix loop
/// (`while (f.exists())`) stays in Kotlin since it touches the filesystem.
pub fn sanitize_filename_base(base: &str) -> String {
    let cleaned: String = base
        .chars()
        .map(|c| {
            if matches!(c, '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim();
    let capped: String = trimmed.chars().take(80).collect();
    if capped.trim().is_empty() {
        "file".to_string()
    } else {
        capped
    }
}

#[cfg(test)]
mod fragments_batch1_tests {
    use super::*;

    #[test]
    fn grid_preset_parses_both_parts() {
        assert_eq!(parse_grid_preset("4x6", 5, 5), (4, 6));
    }

    #[test]
    fn grid_preset_falls_back_on_garbage() {
        assert_eq!(parse_grid_preset("x", 4, 5), (4, 5));
        assert_eq!(parse_grid_preset("abc", 4, 5), (4, 5));
    }

    #[test]
    fn resolution_string_accepts_x_and_multiplication_sign() {
        assert_eq!(parse_resolution_string("1920x1080"), Some((1920, 1080)));
        assert_eq!(parse_resolution_string("1920×1080"), Some((1920, 1080)));
        assert_eq!(parse_resolution_string(" 1920 x 1080 "), Some((1920, 1080)));
    }

    #[test]
    fn resolution_string_rejects_invalid() {
        assert_eq!(parse_resolution_string("1920"), None);
        assert_eq!(parse_resolution_string("0x1080"), None);
        assert_eq!(parse_resolution_string("-100x1080"), None);
        assert_eq!(parse_resolution_string("abcxdef"), None);
    }

    #[test]
    fn natural_size_swaps_landscape_input() {
        // user types 1920x1080 (landscape) -> natural portrait is 1080x1920
        assert_eq!(natural_display_size(1920, 1080), (1080, 1920));
        assert_eq!(natural_display_size(1080, 1920), (1080, 1920));
    }

    #[test]
    fn mime_matches_wildcard() {
        assert!(mime_matches(&["*/*"], "anything"));
        assert!(mime_matches(&["image/*"], "png"));
        assert!(!mime_matches(&["image/*"], "mp4"));
        assert!(mime_matches(&["font/ttf", "font/otf"], "otf"));
        assert!(!mime_matches(&["font/ttf"], "pdf"));
    }

    #[test]
    fn file_icon_kind_covers_common_extensions() {
        assert_eq!(file_icon_kind("MP4"), FileIconKind::Video);
        assert_eq!(file_icon_kind("csv"), FileIconKind::Text);
        assert_eq!(file_icon_kind("weird"), FileIconKind::Generic);
    }

    #[test]
    fn duration_formats_mmss() {
        assert_eq!(format_duration_mmss(65_000), "1:05");
        assert_eq!(format_duration_mmss(5_000), "0:05");
        assert_eq!(format_duration_mmss(600_000), "10:00");
    }

    #[test]
    fn chmod_commands_full_path() {
        let cmds = build_chmod_commands("/data/data/com.youki.dex/files/sounds/startup_sound.mp3");
        assert!(cmds.contains("chmod 711 \"/data/data/com.youki.dex/files\""));
        assert!(cmds.contains("chmod 711 \"/data/data/com.youki.dex/files/sounds\""));
        assert!(cmds.ends_with("chmod 644 \"/data/data/com.youki.dex/files/sounds/startup_sound.mp3\""));
    }

    #[test]
    fn chmod_commands_short_path_falls_back() {
        assert_eq!(build_chmod_commands("startup_sound.mp3"), "chmod 644 \"startup_sound.mp3\"");
    }

    #[test]
    fn easter_egg_first_tap_starts_count() {
        let (count, first_tap, reached) = easter_egg_tap_state(1000, 0, 0, 1500, 7);
        assert_eq!(count, 1);
        assert_eq!(first_tap, 1000);
        assert!(!reached);
    }

    #[test]
    fn easter_egg_expired_window_resets() {
        // 6th tap already happened, but this tap comes in 2000ms later (> 1500ms window)
        let (count, first_tap, reached) = easter_egg_tap_state(3000, 5, 1000, 1500, 7);
        assert_eq!(count, 1); // restarted, not 6
        assert_eq!(first_tap, 3000);
        assert!(!reached);
    }

    #[test]
    fn easter_egg_seventh_tap_within_window_triggers() {
        let (count, _first_tap, reached) = easter_egg_tap_state(1400, 6, 1000, 1500, 7);
        assert_eq!(count, 0); // reset after triggering
        assert!(reached);
    }

    #[test]
    fn ad_url_matches_known_domain() {
        assert!(is_ad_url("https://pagead2.googlesyndication.com/ads"));
        assert!(is_ad_url("HTTPS://DOUBLECLICK.NET/x"));
        assert!(!is_ad_url("https://example.com/article"));
    }

    #[test]
    fn mime_to_ext_matches_by_substring() {
        assert_eq!(mime_to_ext("video/mp4"), "mp4");
        assert_eq!(mime_to_ext("application/x-font-ttf"), "ttf");
        assert_eq!(mime_to_ext("application/pdf"), "");
    }

    #[test]
    fn name_from_cd_extracts_and_cleans() {
        assert_eq!(
            name_from_content_disposition("attachment; filename=\"my_cool-video.mp4\""),
            "my cool video"
        );
        assert_eq!(name_from_content_disposition("attachment"), "");
    }

    #[test]
    fn name_from_cd_handles_path_like_filename() {
        assert_eq!(
            name_from_content_disposition("filename=folder/name_here.zip"),
            "name here"
        );
    }

    #[test]
    fn file_size_formats_bytes_kb_mb() {
        assert_eq!(format_file_size(500), "500 B");
        assert_eq!(format_file_size(2048), "2 KB");
        assert_eq!(format_file_size(1536), "1.5 KB");
        assert_eq!(format_file_size(5 * 1024 * 1024), "5 MB");
    }

    #[test]
    fn sanitize_filename_strips_illegal_chars_and_caps_length() {
        assert_eq!(sanitize_filename_base("my:cool*file?.txt"), "my_cool_file_.txt");
        assert_eq!(sanitize_filename_base("   "), "file");
        let long = "a".repeat(200);
        assert_eq!(sanitize_filename_base(&long).len(), 80);
    }
}

// ── fragments/PerformanceFragment.kt ──────────────────────────────────────
// The bulk of this file (setupTweakToggles: 11 TweakDef entries with
// enableCmd/disableCmd) is fixed shell-script text — no branching, no loop
// over variable data, no Android API calls. It's data, not logic, so it
// stays in Kotlin verbatim rather than becoming a giant Rust string
// constant that would only add JNI marshaling overhead for zero benefit.
// The functions below are the file's actual parametrized logic.

/// Port of `PerformanceFragment.applyFps`'s command builder (the
/// `resetFps` twin uses the same eleven settings keys with `delete`
/// instead of `put $hz`/`i32 $hz`, handled by the `reset` flag below).
/// `hz` is ignored when `reset` is true.
pub fn build_fps_command(hz: i32, reset: bool) -> String {
    const SYSTEM_KEYS: &[&str] = &[
        "peak_refresh_rate",
        "min_refresh_rate",
        "max_refresh_rate",
        "user_refresh_rate",
        "miui_refresh_rate",
        "thermal_limit_refresh_rate",
    ];
    const SECURE_KEYS: &[&str] = &["max_refresh_rate", "user_refresh_rate", "miui_refresh_rate"];
    const GLOBAL_KEYS: &[&str] = &["preferred_refresh_rate", "user_preferred_refresh_rate"];

    let mut lines: Vec<String> = Vec::with_capacity(12);
    for key in SYSTEM_KEYS {
        lines.push(settings_line("system", key, hz, reset));
    }
    for key in SECURE_KEYS {
        lines.push(settings_line("secure", key, hz, reset));
    }
    for key in GLOBAL_KEYS {
        lines.push(settings_line("global", key, hz, reset));
    }
    let sf_value = if reset { 0 } else { hz };
    lines.push(format!(
        "service call SurfaceFlinger 1035 i32 {sf_value} 2>/dev/null || true"
    ));
    lines.join("\n") + "\n"
}

fn settings_line(namespace: &str, key: &str, hz: i32, reset: bool) -> String {
    if reset {
        format!("settings delete {namespace} {key} 2>/dev/null || true")
    } else {
        format!("settings put {namespace} {key} {hz} 2>/dev/null || true")
    }
}

/// Mirrors `PerformanceFragment.ThermalMode`. Kotlin owns `labelRes`
/// (needs the R class); this enum only carries which mode was picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThermalMode {
    Balanced,
    Performance,
    Gaming,
    Powersave,
}

/// Port of `PerformanceFragment.buildThermalCmd`.
pub fn build_thermal_command(mode: ThermalMode) -> String {
    let lines: &[&str] = match mode {
        ThermalMode::Balanced => &[
            "settings put global restricted_networking_mode 0 2>/dev/null || true",
            "settings put system thermal_config_index 0 2>/dev/null || true",
            "setprop vendor.thermal.config thermal-normal 2>/dev/null || true",
            "setprop persist.vendor.qti.games.gt.propvhigh 0 2>/dev/null || true",
        ],
        ThermalMode::Performance => &[
            "settings put system thermal_config_index 1 2>/dev/null || true",
            "setprop vendor.thermal.config thermal-camera 2>/dev/null || true",
            "setprop persist.vendor.qti.games.gt.propvhigh 1 2>/dev/null || true",
            "setprop persist.sys.perf.topAppRenderThreadBoost.enable 1 2>/dev/null || true",
        ],
        ThermalMode::Gaming => &[
            "settings put system thermal_config_index 2 2>/dev/null || true",
            "setprop vendor.thermal.config gaming-cdev-table 2>/dev/null || true",
            "setprop persist.vendor.qti.games.gt.propvhigh 1 2>/dev/null || true",
            "setprop persist.sys.perf.topAppRenderThreadBoost.enable 1 2>/dev/null || true",
            "setprop persist.game_optimizing_service.perf_tune_enable 1 2>/dev/null || true",
        ],
        ThermalMode::Powersave => &[
            "settings put global low_power 1 2>/dev/null || true",
            "settings put system thermal_config_index 3 2>/dev/null || true",
            "setprop vendor.thermal.config thermal-battery 2>/dev/null || true",
            "setprop persist.vendor.qti.games.gt.propvhigh 0 2>/dev/null || true",
        ],
    };
    let mut result = lines.join("\n");
    result.push('\n');
    result
}

/// Port of `PerformanceFragment.applyZram`'s command builder.
pub fn build_zram_enable_command(bytes: i64) -> String {
    format!(
        "swapoff /dev/block/zram0 2>/dev/null || true\n\
         echo 1 > /sys/block/zram0/reset 2>/dev/null || true\n\
         echo {bytes} > /sys/block/zram0/disksize 2>/dev/null || true\n\
         mkswap /dev/block/zram0 2>/dev/null || true\n\
         swapon /dev/block/zram0 2>/dev/null || true\n"
    )
}

/// Port of `PerformanceFragment.disableZram`'s command builder.
pub fn build_zram_disable_command() -> &'static str {
    "swapoff /dev/block/zram0 2>/dev/null || true\n\
     echo 0 > /sys/block/zram0/disksize 2>/dev/null || true\n"
}

/// Port of the `onZram` callback in `PerformanceFragment.loadCurrentValues`:
/// buckets a raw `/sys/block/zram0/disksize` byte count into the same
/// display label `zramValues` offers in the picker dialog. Kotlin passes
/// `off_label` pre-formatted (it embeds a localized string resource, e.g.
/// "Off (0)") since that needs `getString`.
pub fn zram_bucket_label(bytes: i64, off_label: &str) -> String {
    if bytes == 0 {
        off_label.to_string()
    } else if bytes < 536_870_912 {
        "256 MB".to_string()
    } else if bytes < 1_073_741_824 {
        "512 MB".to_string()
    } else if bytes < 2_147_483_648 {
        "1 GB".to_string()
    } else if bytes < 3_221_225_472 {
        "2 GB".to_string()
    } else if bytes < 4_294_967_296 {
        "3 GB".to_string()
    } else if bytes < 6_442_450_944 {
        "4 GB".to_string()
    } else if bytes < 8_589_934_592 {
        "6 GB".to_string()
    } else {
        "8 GB".to_string()
    }
}

/// Port of `PerformanceFragment.execShell`'s inline-script join: Android's
/// cacheDir is often mounted `noexec`, so multi-line scripts can't be
/// written to a `.sh` file and executed — instead, lines are joined with
/// `" ; "` and passed inline via `sh -c '...'`, which never touches the
/// filesystem.
pub fn build_inline_shell_command(cmd: &str) -> String {
    let inline = cmd.trim().replace('\n', " ; ");
    format!("sh -c '{inline}'")
}

#[cfg(test)]
mod performance_fragment_tests {
    use super::*;

    #[test]
    fn fps_command_sets_all_eleven_keys() {
        let cmd = build_fps_command(120, false);
        assert!(cmd.contains("settings put system peak_refresh_rate 120 2>/dev/null || true"));
        assert!(cmd.contains("settings put secure miui_refresh_rate 120 2>/dev/null || true"));
        assert!(cmd.contains("settings put global user_preferred_refresh_rate 120 2>/dev/null || true"));
        assert!(cmd.contains("service call SurfaceFlinger 1035 i32 120 2>/dev/null || true"));
        assert_eq!(cmd.lines().count(), 12);
    }

    #[test]
    fn fps_command_reset_deletes_keys_and_zeroes_surfaceflinger() {
        let cmd = build_fps_command(999, true); // hz ignored when reset
        assert!(cmd.contains("settings delete system peak_refresh_rate 2>/dev/null || true"));
        assert!(!cmd.contains("999"));
        assert!(cmd.contains("service call SurfaceFlinger 1035 i32 0 2>/dev/null || true"));
    }

    #[test]
    fn thermal_command_matches_each_mode() {
        assert!(build_thermal_command(ThermalMode::Balanced).contains("thermal-normal"));
        assert!(build_thermal_command(ThermalMode::Performance).contains("thermal-camera"));
        assert!(build_thermal_command(ThermalMode::Gaming).contains("gaming-cdev-table"));
        assert!(build_thermal_command(ThermalMode::Powersave).contains("low_power 1"));
    }

    #[test]
    fn zram_enable_embeds_byte_count() {
        let cmd = build_zram_enable_command(1073741824);
        assert!(cmd.contains("echo 1073741824 > /sys/block/zram0/disksize"));
        assert!(cmd.starts_with("swapoff"));
        assert!(cmd.ends_with("swapon /dev/block/zram0 2>/dev/null || true\n"));
    }

    #[test]
    fn zram_disable_is_fixed() {
        assert_eq!(
            build_zram_disable_command(),
            "swapoff /dev/block/zram0 2>/dev/null || true\n\
             echo 0 > /sys/block/zram0/disksize 2>/dev/null || true\n"
        );
    }

    #[test]
    fn zram_bucket_boundaries() {
        assert_eq!(zram_bucket_label(0, "Off (0)"), "Off (0)");
        assert_eq!(zram_bucket_label(1, "Off (0)"), "256 MB");
        assert_eq!(zram_bucket_label(536_870_911, "Off (0)"), "256 MB");
        assert_eq!(zram_bucket_label(536_870_912, "Off (0)"), "512 MB");
        assert_eq!(zram_bucket_label(1_073_741_824, "Off (0)"), "1 GB");
        assert_eq!(zram_bucket_label(8_589_934_592, "Off (0)"), "8 GB");
        assert_eq!(zram_bucket_label(99_999_999_999, "Off (0)"), "8 GB");
    }

    #[test]
    fn inline_shell_joins_multiline_with_semicolons() {
        assert_eq!(
            build_inline_shell_command("echo a\necho b\necho c\n"),
            "sh -c 'echo a ; echo b ; echo c'"
        );
    }

    #[test]
    fn inline_shell_trims_surrounding_whitespace() {
        assert_eq!(
            build_inline_shell_command("  \n echo x \n  "),
            "sh -c 'echo x'"
        );
    }
}

// ── adapters/AppAdapter.kt + utils/Utils.kt::solve ────────────────────────

/// Port of `AppAdapter`'s `CALC_REGEX` = `^[0-9]+(\.[0-9]+)?[-+/*][0-9]+(\.[0-9]+)?`
/// (compiled once, matched on every keystroke in the app-drawer search box
/// to detect "12+7"-style queries). Implemented by hand instead of pulling
/// in a regex engine, since the pattern is a fixed small grammar: an
/// integer or decimal, one operator from `+-*/`, then another integer or
/// decimal, anchored on both ends (Kotlin's `matches`, not `find`).
pub fn is_calc_expression(text: &str) -> bool {
    fn number_len(s: &[u8]) -> Option<usize> {
        let mut i = 0;
        while i < s.len() && s[i].is_ascii_digit() {
            i += 1;
        }
        if i == 0 {
            return None; // needs at least one digit before an optional '.'
        }
        if i < s.len() && s[i] == b'.' {
            let mut j = i + 1;
            while j < s.len() && s[j].is_ascii_digit() {
                j += 1;
            }
            if j == i + 1 {
                return None; // '.' with no digits after it
            }
            return Some(j);
        }
        Some(i)
    }

    let bytes = text.as_bytes();
    let first_len = match number_len(bytes) {
        Some(n) => n,
        None => return false,
    };
    if first_len >= bytes.len() {
        return false; // need an operator + second number after it
    }
    if !matches!(bytes[first_len], b'+' | b'-' | b'*' | b'/') {
        return false;
    }
    let rest = &bytes[first_len + 1..];
    match number_len(rest) {
        Some(n) => n == rest.len(), // anchored: nothing left over
        None => false,
    }
}

/// Port of `Utils.solve` — a tiny non-precedence two-operand calculator for
/// the app-drawer's inline calculator. Searches for the operator from the
/// *right* (matching the original's `lastIndexOf`) so negative numbers like
/// "-5+3" aren't mistaken for the '-' being the operator at index 0;
/// division by zero returns 0.0 rather than propagating `Infinity`/`NaN`,
/// matching the original's explicit check. Returns 0.0 for anything that
/// doesn't parse, same as the original's `catch (NumberFormatException)`.
pub fn solve_arithmetic_expression(expression: &str) -> f64 {
    let e = expression.trim();
    let bytes = e.as_bytes();

    let add_idx = e.rfind('+');
    // Kotlin's `expression.drop(1).lastIndexOf('-')` then `+1` if found —
    // i.e. search for '-' starting from index 1, so a leading '-' (negative
    // first operand) is never picked up as the operator.
    let sub_idx = if bytes.len() > 1 {
        e[1..].rfind('-').map(|i| i + 1)
    } else {
        None
    };
    let mul_idx = e.rfind('*');
    let div_idx = e.rfind('/');

    let parse = |s: &str| s.parse::<f64>().ok();

    if let Some(i) = add_idx.filter(|&i| i > 0) {
        return match (parse(&e[..i]), parse(&e[i + 1..])) {
            (Some(a), Some(b)) => a + b,
            _ => 0.0,
        };
    }
    if let Some(i) = sub_idx.filter(|&i| i > 0) {
        return match (parse(&e[..i]), parse(&e[i + 1..])) {
            (Some(a), Some(b)) => a - b,
            _ => 0.0,
        };
    }
    if let Some(i) = mul_idx.filter(|&i| i > 0) {
        return match (parse(&e[..i]), parse(&e[i + 1..])) {
            (Some(a), Some(b)) => a * b,
            _ => 0.0,
        };
    }
    if let Some(i) = div_idx.filter(|&i| i > 0) {
        return match (parse(&e[..i]), parse(&e[i + 1..])) {
            (Some(a), Some(b)) => {
                if b == 0.0 {
                    0.0
                } else {
                    a / b
                }
            }
            _ => 0.0,
        };
    }
    e.parse::<f64>().unwrap_or(0.0)
}

// ── plugins/PluginManager.kt::readMeta (module.prop branch) ───────────────

/// Port of the `module.prop` (Magisk format) line-parsing step in
/// `PluginManager.readMeta`: given the file's lines already filtered to
/// those containing `=`, splits each into a `(key, value)` pair, trimming
/// both sides. Kotlin still owns the file read and the `.filter { it.contains("=") }`
/// step; this is the per-line split/trim that has no I/O in it.
pub fn parse_module_prop_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    Some((key.trim().to_string(), value.trim().to_string()))
}

// ── plugins/PluginInstaller.kt::sanitizeId ─────────────────────────────────

/// Port of `PluginInstaller.sanitizeId`: strips everything but
/// ASCII letters/digits/underscore/hyphen from a proposed plugin id, caps
/// it at 64 chars, and falls back to "plugin" if that leaves nothing.
pub fn sanitize_plugin_id(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let capped: String = cleaned.chars().take(64).collect();
    if capped.is_empty() {
        "plugin".to_string()
    } else {
        capped
    }
}

#[cfg(test)]
mod small_sections_tests {
    use super::*;

    #[test]
    fn calc_expression_matches_simple_forms() {
        assert!(is_calc_expression("12+7"));
        assert!(is_calc_expression("3.5*2"));
        assert!(is_calc_expression("10/4"));
        assert!(is_calc_expression("9-2"));
        assert!(is_calc_expression("1.5+2.25"));
    }

    #[test]
    fn calc_expression_rejects_non_matches() {
        assert!(!is_calc_expression("hello"));
        assert!(!is_calc_expression("12"));
        assert!(!is_calc_expression("12+"));
        assert!(!is_calc_expression("+12"));
        assert!(!is_calc_expression("12+7 "));   // trailing space -> not anchored
        assert!(!is_calc_expression("12 +7"));   // space breaks the number
        assert!(!is_calc_expression("12+7+3"));  // only one operator allowed
        assert!(!is_calc_expression(""));
    }

    #[test]
    fn solve_handles_all_four_operators() {
        assert_eq!(solve_arithmetic_expression("12+7"), 19.0);
        assert_eq!(solve_arithmetic_expression("9-2"), 7.0);
        assert_eq!(solve_arithmetic_expression("3*4"), 12.0);
        assert_eq!(solve_arithmetic_expression("10/4"), 2.5);
    }

    #[test]
    fn solve_handles_negative_first_operand() {
        // '-' at index 0 is the sign, not the operator; the real operator
        // ('+' here) must still be found correctly.
        assert_eq!(solve_arithmetic_expression("-5+3"), -2.0);
    }

    #[test]
    fn solve_division_by_zero_returns_zero() {
        assert_eq!(solve_arithmetic_expression("5/0"), 0.0);
    }

    #[test]
    fn solve_falls_back_to_plain_number() {
        assert_eq!(solve_arithmetic_expression("42"), 42.0);
        assert_eq!(solve_arithmetic_expression("  3.5  "), 3.5);
    }

    #[test]
    fn solve_returns_zero_for_garbage() {
        assert_eq!(solve_arithmetic_expression("abc"), 0.0);
        assert_eq!(solve_arithmetic_expression(""), 0.0);
    }

    #[test]
    fn module_prop_line_splits_and_trims() {
        assert_eq!(
            parse_module_prop_line("name = My Cool Mod"),
            Some(("name".to_string(), "My Cool Mod".to_string()))
        );
        assert_eq!(
            parse_module_prop_line("version=2.1"),
            Some(("version".to_string(), "2.1".to_string()))
        );
    }

    #[test]
    fn module_prop_line_without_equals_returns_none() {
        assert_eq!(parse_module_prop_line("not a prop line"), None);
    }

    #[test]
    fn sanitize_plugin_id_strips_illegal_chars() {
        assert_eq!(sanitize_plugin_id("my plugin!!"), "my_plugin__");
        assert_eq!(sanitize_plugin_id("valid_id-123"), "valid_id-123");
    }

    #[test]
    fn sanitize_plugin_id_empty_falls_back() {
        assert_eq!(sanitize_plugin_id(""), "plugin");
    }

    #[test]
    fn sanitize_plugin_id_caps_at_64_chars() {
        let long = "a".repeat(200);
        assert_eq!(sanitize_plugin_id(&long).len(), 64);
    }
}

// ── livewallpaper/data/VideoFileStore.kt::importVideo ──────────────────────

/// Port of the filename-sanitizing half of `VideoFileStore.importVideo`:
/// given a suggested display name (already resolved to a fallback like
/// "video_<timestamp>" by Kotlin if the picker gave none), strips anything
/// but ASCII letters/digits/underscore/hyphen and appends ".mp4". Kotlin
/// still owns picking the fallback name (needs `System.currentTimeMillis()`)
/// and the actual file copy.
pub fn sanitize_video_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{cleaned}.mp4")
}

#[cfg(test)]
mod video_file_store_tests {
    use super::*;

    #[test]
    fn sanitize_strips_illegal_chars_and_appends_extension() {
        assert_eq!(sanitize_video_filename("my cool video!"), "my_cool_video_.mp4");
    }

    #[test]
    fn sanitize_leaves_clean_names_untouched() {
        assert_eq!(sanitize_video_filename("video_1699999999999"), "video_1699999999999.mp4");
    }
}

// ── PerfectServer.kt::getCpuUsage ─────────────────────────────────────────

/// Result of one CPU-usage sample — mirrors the (percent, updated prevCpuTotal,
/// updated prevCpuIdle) triple the original mutated as instance fields.
/// Kotlin owns the actual state (prevCpuTotal/prevCpuIdle/lastCpuValue as
/// class fields) and passes the previous values in each call, since this
/// crate has no per-Service-instance state slot to hold them in (unlike
/// shell_client.rs's session token, which is genuinely global/singleton —
/// CPU sampling state is Kotlin-object-scoped, not process-scoped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CpuSample {
    pub percent: i32,
    pub total: i64,
    pub idle_plus_iowait: i64,
}

/// Direct port of `PerfectServer.getCpuUsage()`'s parsing + delta
/// arithmetic. `line` is the first line of `/proc/stat` (Kotlin still
/// does the file read — this crate has no filesystem access requirement
/// here and keeping the read in Kotlin matches this project's existing
/// pattern of the Framework/IO call staying put while pure computation
/// moves). `prev_total`/`prev_idle` are the previous call's
/// `total`/`idle_plus_iowait` (0 on the very first call). `last_value` is
/// the fallback to return if this line can't be parsed or `diffTotal` is
/// non-positive — matching the original's `return lastCpuValue` /
/// `else lastCpuValue` branches exactly.
///
/// Returns `None` only if `line` doesn't even split into the minimum 4
/// required fields (user/nice/system/idle) — Kotlin should treat that the
/// same as its `if (parts.size < 4) return lastCpuValue` guard, i.e. keep
/// showing the previous `last_value` and NOT update prev_total/prev_idle.
pub fn parse_cpu_usage(
    line: &str,
    prev_total: i64,
    prev_idle: i64,
    last_value: i32,
) -> Option<CpuSample> {
    let parts: Vec<&str> = line.trim().split_whitespace().skip(1).collect();
    if parts.len() < 4 {
        return None;
    }
    let user: i64 = parts[0].parse().ok()?;
    let nice: i64 = parts[1].parse().ok()?;
    let system: i64 = parts[2].parse().ok()?;
    let idle: i64 = parts[3].parse().ok()?;
    let iowait: i64 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);
    let irq: i64 = parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0);
    let softirq: i64 = parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0);

    let total = user + nice + system + idle + iowait + irq + softirq;
    let diff_total = total - prev_total;
    let diff_idle = (idle + iowait) - prev_idle;

    let percent = if diff_total <= 0 {
        last_value
    } else {
        (((100 * (diff_total - diff_idle)) / diff_total) as i32).clamp(0, 100)
    };

    Some(CpuSample {
        percent,
        total,
        idle_plus_iowait: idle + iowait,
    })
}

#[cfg(test)]
mod cpu_tests {
    use super::*;

    #[test]
    fn first_call_with_zero_prev_state() {
        // cpu  1000 200 300 8500 0 0 0  (user nice system idle iowait irq softirq)
        let line = "cpu  1000 200 300 8500 0 0 0";
        let sample = parse_cpu_usage(line, 0, 0, 0).unwrap();
        // total = 1000+200+300+8500 = 10000, diffTotal = 10000 (from 0)
        // diffIdle = 8500 (from 0) -> busy% = 100*(10000-8500)/10000 = 15
        assert_eq!(sample.total, 10000);
        assert_eq!(sample.idle_plus_iowait, 8500);
        assert_eq!(sample.percent, 15);
    }

    #[test]
    fn subsequent_call_uses_delta() {
        let line = "cpu  1000 200 300 8500 0 0 0";
        let first = parse_cpu_usage(line, 0, 0, 0).unwrap();

        // Second sample: total grew by 1000, idle grew by 900 -> 100 busy out of 1000 -> 10%
        let line2 = "cpu  1100 250 350 9400 0 0 0"; // total=11100, idle=9400
        let second = parse_cpu_usage(line2, first.total, first.idle_plus_iowait, first.percent).unwrap();
        assert_eq!(second.total, 11100);
        let diff_total = 11100 - 10000; // 1100
        let diff_idle = 9400 - 8500; // 900
        let expected_pct = (100 * (diff_total - diff_idle) / diff_total) as i32;
        assert_eq!(second.percent, expected_pct);
    }

    #[test]
    fn malformed_line_returns_none() {
        assert_eq!(parse_cpu_usage("cpu  100", 0, 0, 42), None);
    }

    #[test]
    fn non_positive_diff_falls_back_to_last_value() {
        // Same totals as before -> diffTotal = 0 -> should return last_value
        let line = "cpu  1000 200 300 8500 0 0 0";
        let sample = parse_cpu_usage(line, 10000, 8500, 42).unwrap();
        assert_eq!(sample.percent, 42);
    }
}
