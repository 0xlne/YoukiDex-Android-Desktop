//! jni_bridge — the actual `Java_*` JNI entry points the JVM resolves to
//! call into this crate. Split into submodules mirroring the Rust modules
//! they front (root, archive, textparse, renderer).
//!
//! NAMING NOTE (read before wiring this up to real Kotlin code): the
//! package prefix `com.youki.dex.livewallpaper` used below is the real,
//! confirmed package this project already uses (verified against
//! YoukiGLWallpaperService.kt / VideoEngine.kt's `package` declarations).
//! What is NOT yet confirmed is the specific Kotlin class name
//! (`NativeBridge`) these functions are named after — that class doesn't
//! exist in the current codebase and its name is this file's own choice,
//! not something copied from existing Kotlin. If the actual PR introduces
//! a differently-named Kotlin object/class to hold these `external fun`
//! declarations, every `Java_com_youki_dex_livewallpaper_NativeBridge_*`
//! symbol name below must be renamed to match exactly — JNI resolves these
//! by exact mangled name, silently failing (UnsatisfiedLinkError at
//! runtime, not a compile error) if they don't match.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, jstring, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;

// ── root.rs bridge ──────────────────────────────────────────────────────

/// Kotlin: `external fun nativeIsRootAvailable(): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsRootAvailable(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if crate::root::is_available() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeInvalidateRootCache()`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeInvalidateRootCache(
    _env: JNIEnv,
    _class: JClass,
) {
    crate::root::invalidate_cache();
}

/// Kotlin: `external fun nativeExecuteRoot(command: String): String`
/// Blocking — the Kotlin side is expected to call this from a background
/// dispatcher (e.g. Dispatchers.IO), same as the original executeRoot()'s
/// own calling convention.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeExecuteRoot(
    mut env: JNIEnv,
    _class: JClass,
    command: JString,
) -> jstring {
    let command: String = match env.get_string(&command) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::root::execute_root(&command);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeGrantWriteSecureSettings(packageName: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeGrantWriteSecureSettings(
    mut env: JNIEnv,
    _class: JClass,
    package_name: JString,
) -> jstring {
    let package_name: String = match env.get_string(&package_name) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::root::grant_write_secure_settings(&package_name);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildGrantAllCommands(packageName: String, sdkInt: Int): Array<String>`
///
/// jobjectArray<jstring> marshaling: allocate a `java/lang/String[]` of the
/// right length via `env.new_object_array` (element class "java/lang/String",
/// initial element a fresh empty JString — jni 0.21 requires a concrete
/// initial-value object, not null, for new_object_array's signature), then
/// fill each slot with `env.set_object_array_element`. Returns an empty
/// array (rather than a null jobjectArray) if the package name can't be
/// read or the array itself fails to allocate, so Kotlin's call site never
/// has to null-check the array itself — only iterate it.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildGrantAllCommands(
    mut env: JNIEnv,
    _class: JClass,
    package_name: JString,
    sdk_int: jint,
) -> jni::sys::jobjectArray {
    let package_name: String = match env.get_string(&package_name) {
        Ok(s) => s.into(),
        Err(_) => String::new(),
    };
    let commands = crate::root::build_grant_all_commands(&package_name, sdk_int);

    let string_class = match env.find_class("java/lang/String") {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };
    let initial = match env.new_string("") {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let array = match env.new_object_array(commands.len() as i32, &string_class, &initial) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    for (i, cmd) in commands.iter().enumerate() {
        let jcmd = match env.new_string(cmd) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let _ = env.set_object_array_element(&array, i as i32, &jcmd);
    }
    array.into_raw()
}

/// Kotlin: `external fun nativeRootDiagnose(cmd: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeRootDiagnose(
    mut env: JNIEnv,
    _class: JClass,
    cmd: JString,
) -> jstring {
    let cmd: String = match env.get_string(&cmd) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::root::diagnose(&cmd);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeShouldRunGrantAll(force: Boolean): Boolean`
/// Combines `isAvailable` + `alreadyGrantedThisSession` into one check —
/// mirrors `RootManager.requestPermission()`'s guard clause
/// (`if (!isAvailable) return; if (alreadyGrantedThisSession && !force) return`).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeShouldRunGrantAll(
    _env: JNIEnv,
    _class: JClass,
    force: jboolean,
) -> jboolean {
    if crate::root::should_run_grant_all(force != JNI_FALSE) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeMarkGrantAllDone()`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeMarkGrantAllDone(
    _env: JNIEnv,
    _class: JClass,
) {
    crate::root::mark_grant_all_done();
}

// ── archive.rs bridge ────────────────────────────────────────────────────

/// Kotlin: `external fun nativeExtractZip(zipPath: String, destDir: String, deleteAfter: Boolean): Boolean`
///
/// NOTE: progress reporting (the original's ProgressDialog updates) is not
/// wired up in this signature — the `ProgressCallback` parameter
/// `archive::extract_zip` accepts is Rust-closure-only and has no JNI
/// representation here yet. A real implementation should either poll a
/// shared progress value from Kotlin (simplest) or accept a JNI callback
/// object and invoke a method on it per progress tick (more idiomatic but
/// more JNI plumbing) — deferred rather than guessed at.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeExtractZip(
    mut env: JNIEnv,
    _class: JClass,
    zip_path: JString,
    dest_dir: JString,
    delete_after: jboolean,
) -> jboolean {
    let zip_path: String = match env.get_string(&zip_path) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    let dest_dir: String = match env.get_string(&dest_dir) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    let result = crate::archive::extract_zip(
        std::path::Path::new(&zip_path),
        std::path::Path::new(&dest_dir),
        delete_after != JNI_FALSE,
        None, // no progress callback wired up yet — see NOTE above
    );
    if result.is_ok() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeCompressToZip(sourcePath: String, zipPath: String): Boolean`
/// Single-source convenience wrapper over `archive::compress_to_zip`
/// (which takes a slice of sources) — matches `compressToZip()`'s original
/// single-file call site. A multi-file variant
/// (`nativeCompressMultipleToZip`, matching `doCompressMultiple`) is not
/// yet added; the underlying `archive::compress_to_zip` already supports it
/// (`&[PathBuf]`), only the JNI array-of-strings marshaling is missing,
/// same category of gap as `nativeBuildGrantAllCommands` above.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeCompressToZip(
    mut env: JNIEnv,
    _class: JClass,
    source_path: JString,
    zip_path: JString,
) -> jboolean {
    let source_path: String = match env.get_string(&source_path) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    let zip_path: String = match env.get_string(&zip_path) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    let sources = vec![std::path::PathBuf::from(source_path)];
    let result = crate::archive::compress_to_zip(&sources, std::path::Path::new(&zip_path), None);
    if result.is_ok() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ── textparse.rs bridge ─────────────────────────────────────────────────

/// Kotlin: `external fun nativeIsCommandSuccess(output: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsCommandSuccess(
    mut env: JNIEnv,
    _class: JClass,
    output: JString,
) -> jboolean {
    let output: String = match env.get_string(&output) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    if crate::textparse::is_success(&output) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeSanitize(input: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSanitize(
    mut env: JNIEnv,
    _class: JClass,
    input: JString,
) -> jstring {
    let input: String = match env.get_string(&input) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let sanitized = crate::textparse::sanitize(&input);
    match env.new_string(sanitized) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeExtractCreatedUserId(raw: String): Int` — returns
/// -1 if no match, matching the original's `?.toIntOrNull()` -> null ->
/// Kotlin-side null-check pattern collapsed into a single sentinel value
/// for the simpler jint return type (no Optional<Int> equivalent over JNI
/// without extra boxing machinery).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeExtractCreatedUserId(
    mut env: JNIEnv,
    _class: JClass,
    raw: JString,
) -> jint {
    let raw: String = match env.get_string(&raw) {
        Ok(s) => s.into(),
        Err(_) => return -1,
    };
    crate::textparse::extract_created_user_id(&raw).unwrap_or(-1)
}

/// Kotlin: `external fun nativeParseUsers(raw: String, currentUserId: Int): String`
///
/// Returns a semicolon-separated `"id,name,isCurrent,isRunning"` list — same
/// small bespoke text-encoding pattern used by nativeResolveDrop above (see
/// that function's doc comment for the full rationale: avoids a
/// jobjectArray<CustomObject> marshaling dance for a Vec<struct> return).
/// `name` cannot itself contain a `,` or `;` after textparse.rs's regex
/// capture (usernames from `UserInfo{id:name:flags}` can't contain `:` per
/// the regex's own non-greedy capture boundary, and `,`/`;` are not part of
/// that format either) so no escaping is needed on that field.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseUsers(
    mut env: JNIEnv,
    _class: JClass,
    raw: JString,
    current_user_id: jint,
) -> jstring {
    let raw: String = match env.get_string(&raw) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let users = crate::textparse::parse_users(&raw, current_user_id);
    let encoded = users
        .iter()
        .map(|u| format!("{},{},{},{}", u.id, u.name, u.is_current, u.is_running))
        .collect::<Vec<_>>()
        .join(";");
    match env.new_string(encoded) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeParseTaskList(output: String, selfPkg: String, launcherPkg: String, max: Int): String`
///
/// Returns a semicolon-separated `"taskId,packageName"` list. Package names
/// from `baseIntent=<pkg>/` can't contain `,` or `;` (Android package name
/// syntax: `[a-zA-Z][a-zA-Z0-9_]*(\.[a-zA-Z][a-zA-Z0-9_]*)+`, matching
/// textparse.rs's own PKG_RE character class), so no escaping is needed.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseTaskList(
    mut env: JNIEnv,
    _class: JClass,
    output: JString,
    self_pkg: JString,
    launcher_pkg: JString,
    max: jint,
) -> jstring {
    let output: String = match env.get_string(&output) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let self_pkg: String = match env.get_string(&self_pkg) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let launcher_pkg: String = match env.get_string(&launcher_pkg) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let entries = crate::textparse::parse_task_list(&output, &self_pkg, &launcher_pkg, max.max(0) as usize);
    let encoded = entries
        .iter()
        .map(|e| format!("{},{}", e.task_id, e.package_name))
        .collect::<Vec<_>>()
        .join(";");
    match env.new_string(encoded) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Shared helper: unmarshals a `(labels: Array<String>, ids: IntArray)` JNI
/// argument pair into a `Vec<(String, i32)>`. Used by both
/// nativeSortByLabelAlphabetical (case-insensitive) and
/// nativeSortByLabelCaseSensitive below — the two differ only in which
/// textparse.rs sort function they call afterward, not in how they
/// unmarshal their arguments. Returns `None` (rather than a Result) on any
/// marshaling failure or length mismatch; callers turn that into an empty
/// jintArray, matching both functions' existing "mismatched lengths return
/// empty rather than panicking" contract.
fn unmarshal_label_id_pairs(
    env: &mut JNIEnv,
    labels: &jni::objects::JObjectArray,
    ids: &jni::objects::JIntArray,
) -> Option<Vec<(String, i32)>> {
    let label_count = env.get_array_length(labels).ok()?;
    let id_count = env.get_array_length(ids).ok()?;
    if label_count != id_count {
        return Some(Vec::new());
    }
    let mut id_buf = vec![0i32; id_count as usize];
    env.get_int_array_region(ids, 0, &mut id_buf).ok()?;

    let mut pairs: Vec<(String, i32)> = Vec::with_capacity(label_count as usize);
    for i in 0..label_count {
        let jstr = match env.get_object_array_element(labels, i) {
            Ok(o) => jni::objects::JString::from(o),
            Err(_) => continue,
        };
        let label: String = match env.get_string(&jstr) {
            Ok(s) => s.into(),
            Err(_) => continue,
        };
        pairs.push((label, id_buf[i as usize]));
    }
    Some(pairs)
}

/// Kotlin: `external fun nativeSortByLabelAlphabetical(labels: Array<String>, ids: IntArray): IntArray`
///
/// Sorts the given (label, id) pairs case-insensitively by label and
/// returns just the reordered `ids` array — Kotlin re-associates each id
/// back to its original App/AppInfo object (this function never sees those
/// types, matching sort_by_label_alphabetical's own doc comment). `labels`
/// and `ids` must be the same length; mismatched lengths return an empty
/// array rather than panicking.
///
/// Case-*insensitive* — matches MultiUserManager's user-list sort. For the
/// case-*sensitive* variant matching AppUtils.kt's app-list sorts, see
/// nativeSortByLabelCaseSensitive below (see textparse.rs's doc comment on
/// why these are genuinely two different, both-correct, behaviors rather
/// than one that should be unified).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSortByLabelAlphabetical(
    mut env: JNIEnv,
    _class: JClass,
    labels: jni::objects::JObjectArray,
    ids: jni::objects::JIntArray,
) -> jni::sys::jintArray {
    let pairs = match unmarshal_label_id_pairs(&mut env, &labels, &ids) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };
    let sorted = crate::textparse::sort_by_label_alphabetical(pairs);
    let sorted_ids: Vec<i32> = sorted.iter().map(|(_, id)| *id).collect();
    make_int_array_n(&env, &sorted_ids)
}

/// Kotlin: `external fun nativeSortByLabelCaseSensitive(labels: Array<String>, ids: IntArray): IntArray`
///
/// Same shape as nativeSortByLabelAlphabetical above, but
/// case-*sensitive* — matches AppUtils.kt's `getInstalledPackages()` /
/// `getInstalledApps()`, both of which use Kotlin's default (case-
/// sensitive, codepoint-order) `String.compareTo` via `compareBy { it.name }`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSortByLabelCaseSensitive(
    mut env: JNIEnv,
    _class: JClass,
    labels: jni::objects::JObjectArray,
    ids: jni::objects::JIntArray,
) -> jni::sys::jintArray {
    let pairs = match unmarshal_label_id_pairs(&mut env, &labels, &ids) {
        Some(p) => p,
        None => return std::ptr::null_mut(),
    };
    let sorted = crate::textparse::sort_by_label_case_sensitive(pairs);
    let sorted_ids: Vec<i32> = sorted.iter().map(|(_, id)| *id).collect();
    make_int_array_n(&env, &sorted_ids)
}

/// Kotlin: `external fun nativeSortByLastUsedDescending(lastUsedTimes: LongArray, ids: IntArray): IntArray`
///
/// Same "sort pairs, return reordered ids" shape as
/// nativeSortByLabelAlphabetical above, for the descending-by-timestamp
/// case (usage-stats "recently used" ordering).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSortByLastUsedDescending(
    env: JNIEnv,
    _class: JClass,
    last_used_times: jni::objects::JLongArray,
    ids: jni::objects::JIntArray,
) -> jni::sys::jintArray {
    let time_count = match env.get_array_length(&last_used_times) {
        Ok(n) => n,
        Err(_) => return std::ptr::null_mut(),
    };
    let id_count = match env.get_array_length(&ids) {
        Ok(n) => n,
        Err(_) => return std::ptr::null_mut(),
    };
    if time_count != id_count {
        return make_int_array_n(&env, &[]);
    }
    let mut time_buf = vec![0i64; time_count as usize];
    if env.get_long_array_region(&last_used_times, 0, &mut time_buf).is_err() {
        return std::ptr::null_mut();
    }
    let mut id_buf = vec![0i32; id_count as usize];
    if env.get_int_array_region(&ids, 0, &mut id_buf).is_err() {
        return std::ptr::null_mut();
    }

    let pairs: Vec<(i64, i32)> = time_buf.into_iter().zip(id_buf).collect();
    let sorted = crate::textparse::sort_by_last_used_descending(pairs);
    let sorted_ids: Vec<i32> = sorted.iter().map(|(_, id)| *id).collect();
    make_int_array_n(&env, &sorted_ids)
}

/// Shared helper: builds a jintArray of arbitrary length from a slice.
/// (make_int_array_2 above is kept separate/inline for the fixed
/// 2-element case's slightly simpler call sites — this one exists for the
/// variable-length sort results above.)
fn make_int_array_n(env: &JNIEnv, values: &[i32]) -> jni::sys::jintArray {
    let arr = match env.new_int_array(values.len() as i32) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    if !values.is_empty() && env.set_int_array_region(&arr, 0, values).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

// ── renderer bridge ──────────────────────────────────────────────────────
//
// HANDLE OWNERSHIP MODEL (decided, not deferred): a `Box<dyn
// renderer::RendererBackend>` is leaked into a raw pointer via
// `Box::into_raw`, and the resulting address is returned to Kotlin as a
// `jlong`. Every subsequent call passes that `jlong` back in; each wrapper
// reconstructs a `&mut` reference via `&mut *(handle as *mut _)` for the
// duration of the call (never taking ownership back until
// nativeDestroyRenderer). `nativeDestroyRenderer` reconstructs the `Box`
// via `Box::from_raw` and lets it drop, running `on_surface_destroyed`
// first. This is a standard, well-established Rust FFI ownership pattern
// (not a guessed API surface like the EGL/Vulkan extension bindings
// elsewhere in this codebase) — the correctness burden is entirely on the
// Kotlin side calling nativeDestroyRenderer exactly once and never after
// using a stale handle, which is a normal C-FFI lifetime discipline, not
// something unique to this bridge.
//
// SAFETY WARNING for whoever wires this into Kotlin: a `jlong` handle used
// after `nativeDestroyRenderer` is a use-after-free. Kotlin's
// WallpaperGLEngine-equivalent (or its Rust-backed replacement) MUST null
// out/invalidate its handle immediately after calling
// nativeDestroyRenderer, and MUST NOT call any other native* renderer
// function afterward. This mirrors the exact same discipline
// WallpaperGLEngine.kt already had to maintain around its `released`
// boolean flag (see gl_backend.rs's on_surface_destroyed comment) — the
// risk class is identical, just moved from a Kotlin `Boolean` guard to a
// raw pointer's validity.

use crate::renderer::{self, BackendPreference, FrameConfig, ScaleMode};

/// Kotlin: `external fun nativeCreateRenderer(preference: Int): Long`
/// preference: 0 = Auto, 1 = ForceGles, 2 = ForceVulkan — matches
/// BackendPreference's variant order; Kotlin should use a matching IntDef/
/// sealed-class-to-Int mapping rather than passing arbitrary values (any
/// other value falls back to Auto, see the `_ =>` arm below).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeCreateRenderer(
    _env: JNIEnv,
    _class: JClass,
    preference: jint,
) -> jlong {
    let pref = match preference {
        1 => BackendPreference::ForceGles,
        2 => BackendPreference::ForceVulkan,
        _ => BackendPreference::Auto,
    };
    let backend = renderer::resolve_preference(pref);
    let instance = renderer::create_backend(backend);
    Box::into_raw(Box::new(instance)) as jlong
}

/// Kotlin: `external fun nativeDestroyRenderer(handle: Long)`
/// See the module-level SAFETY WARNING above — this consumes the handle;
/// Kotlin must not use it again afterward.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDestroyRenderer(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: caller contract (documented above and at every other
    // renderer bridge function) is that `handle` was produced by
    // nativeCreateRenderer and has not already been passed to
    // nativeDestroyRenderer.
    unsafe {
        let mut boxed: Box<Box<dyn renderer::RendererBackend>> =
            Box::from_raw(handle as *mut Box<dyn renderer::RendererBackend>);
        boxed.on_surface_destroyed();
        // `boxed` drops here, freeing the renderer.
    }
}

/// Kotlin: `external fun nativeOnSurfaceCreated(handle: Long, surface: android.view.Surface): Boolean`
///
/// NOTE: accepting a `android.view.Surface` object directly and converting
/// it to an `ANativeWindow*` requires `ndk_sys::ANativeWindow_fromSurface`
/// (via a `JObject` parameter + `env.get_raw()`-style pointer extraction) —
/// this specific conversion step is NOT implemented below; the function
/// signature and handle-dereferencing logic are complete and correct, but
/// the `todo!()` marks the one piece needing ndk_sys's exact
/// ANativeWindow_fromSurface binding signature verified against a real
/// build, same confidence category as the EGL/Vulkan extension bindings
/// flagged elsewhere in this codebase — not guessed at.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeOnSurfaceCreated(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    surface: jni::objects::JObject,
) -> jboolean {
    if handle == 0 {
        return JNI_FALSE;
    }
    // SAFETY: see module-level ownership model note. `handle` must be a
    // live pointer produced by nativeCreateRenderer.
    let backend = unsafe { &mut *(handle as *mut Box<dyn renderer::RendererBackend>) };

    // Convert the incoming android.view.Surface (a jobject) into an
    // ndk::native_window::NativeWindow using the ndk crate's own safe-ish
    // wrapper (NativeWindow::from_surface), rather than hand-rolling the
    // ndk_sys::ANativeWindow_fromSurface call + NonNull-check ourselves.
    // Source: docs.rs/ndk native_window.rs — `pub unsafe fn from_surface(env:
    // *mut JNIEnv, surface: jobject) -> Option<Self>`. It internally calls
    // ANativeWindow_fromSurface (which also bumps the native window's
    // refcount — the returned NativeWindow's Drop impl releases it), so no
    // separate ANativeWindow_release call is needed on our end.
    //
    // SAFETY: `env` is the live JNIEnv the JVM handed this JNI function, and
    // `surface` is the jobject the JVM handed us as the `surface` parameter —
    // both satisfy from_surface's documented safety contract.
    let env_ptr = env.get_raw();
    let surface_raw = surface.as_raw();
    let window = unsafe { ndk::native_window::NativeWindow::from_surface(env_ptr, surface_raw) };

    let window = match window {
        Some(w) => w,
        None => return JNI_FALSE,
    };

    match backend.on_surface_created(&window) {
        Ok(()) => JNI_TRUE,
        Err(_) => JNI_FALSE,
    }
}

/// Kotlin: `external fun nativeOnSurfaceChanged(handle: Long, width: Int, height: Int)`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeOnSurfaceChanged(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
    width: jint,
    height: jint,
) {
    if handle == 0 {
        return;
    }
    let backend = unsafe { &mut *(handle as *mut Box<dyn renderer::RendererBackend>) };
    backend.on_surface_changed(width, height);
}

/// Kotlin: `external fun nativeOnDrawFrame(handle: Long, scaleMode: Int, backgroundColor: String, brightness: Float, contrast: Float, saturation: Float, colorCorrectionEnabled: Boolean, hasNewFrame: Boolean)`
///
/// DESIGN DECISION (resolves the "per-frame config marshaling shape" TODO
/// flagged in an earlier version of this file): rather than a jfloatArray
/// or JSON blob, FrameConfig's fields are passed as individual JNI
/// parameters. This is more verbose per-call than a packed array, but
/// avoids any array-layout convention that has to be kept in sync by
/// position between Kotlin and Rust (a classic source of silent bugs if
/// one side reorders fields) — each parameter is self-describing by name
/// at the call site on both ends. scaleMode: 0=Stretch, 1=Fit, 2=Cover,
/// 3=Free, matching ScaleMode's declaration order in renderer::mod.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeOnDrawFrame(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    scale_mode: jint,
    background_color: JString,
    brightness: jfloat,
    contrast: jfloat,
    saturation: jfloat,
    color_correction_enabled: jboolean,
    has_new_frame: jboolean,
) {
    if handle == 0 {
        return;
    }
    let background_color: String = match env.get_string(&background_color) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let scale_mode = match scale_mode {
        0 => ScaleMode::Stretch,
        1 => ScaleMode::Fit,
        2 => ScaleMode::Cover,
        _ => ScaleMode::Free,
    };
    let config = FrameConfig {
        scale_mode,
        background_color,
        brightness,
        contrast,
        saturation,
        color_correction_enabled: color_correction_enabled != JNI_FALSE,
    };
    let backend = unsafe { &mut *(handle as *mut Box<dyn renderer::RendererBackend>) };
    backend.on_draw_frame(&config, has_new_frame != JNI_FALSE);
}

/// Kotlin: `external fun nativeVideoInputSurfaceHandle(handle: Long): Long`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeVideoInputSurfaceHandle(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) -> jlong {
    if handle == 0 {
        return 0;
    }
    let backend = unsafe { &*(handle as *const Box<dyn renderer::RendererBackend>) };
    backend.video_input_surface_handle()
}

/// Kotlin: `external fun nativeQueryVulkanCapabilities(): String` — returns
/// a small descriptive string for the onboarding step / Advanced Settings
/// (e.g. "Vulkan 1.3 (Adreno 740)" or "Vulkan not supported"), matching the
/// handoff doc's "plain descriptive text only, no preview" decision. This
/// one IS implemented, since it only depends on
/// `renderer::query_vulkan_capabilities()` (already complete) and simple
/// string formatting — no renderer-handle lifetime questions apply here,
/// unlike the rest of the renderer bridge above.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeQueryVulkanCapabilities(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let caps = crate::renderer::query_vulkan_capabilities();
    let text = if caps.supported {
        format!(
            "Vulkan {}.{}.{} ({})",
            caps.api_version_major, caps.api_version_minor, caps.api_version_patch, caps.device_name
        )
    } else {
        "Vulkan not supported on this device".to_string()
    };
    // env is intentionally not `mut` here since new_string doesn't require
    // mutable access to a JNIEnv obtained by value in this jni crate
    // version's API shape — flagged as the same not-build-verified
    // confidence category as other JNI signatures in this file if this
    // turns out to need `mut env` instead.
    match env.new_string(text) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// NOTE: `jlong` (needed for the renderer handle bridge) is intentionally
// not imported yet — see the "renderer bridge — NOT YET IMPLEMENTED" note
// above. Add it back to the `jni::sys::{...}` import above when that part
// is written, rather than importing it unused now.

// ── desktop_grid.rs bridge ──────────────────────────────────────────────
//
// HANDLE OWNERSHIP MODEL: identical pattern to the renderer bridge above —
// a `DesktopOccupancyGrid` is leaked into a raw pointer via `Box::into_raw`
// and handed to Kotlin as a `jlong`. Kotlin (DesktopGridManager or
// LauncherActivity's thin wrapper) must call nativeDestroyDesktopGrid
// exactly once, and never use the handle afterward.
//
// Per-item state (occupied cells) lives entirely in this Rust-side grid —
// Kotlin no longer keeps its own DesktopOccupancyGrid/CellRect/PushDirection
// (those Kotlin types are deleted; see DesktopGridPrefs.kt). Kotlin's job on
// every drag-drop is: (1) walk its View children to build the "item_id at
// (col,row)" list (this part must stay Kotlin — it reads LayoutParams), (2)
// call nativePlaceItem for each to mirror that into the Rust grid, (3) call
// nativeResolveDrop, (4) apply the returned pixel positions to views and
// persist them (also must stay Kotlin — SharedPreferences).

use crate::desktop_grid::{CellRect, DesktopOccupancyGrid};

/// Kotlin: `external fun nativeCreateDesktopGrid(columns: Int, rows: Int): Long`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeCreateDesktopGrid(
    _env: JNIEnv,
    _class: JClass,
    columns: jint,
    rows: jint,
) -> jlong {
    let grid = DesktopOccupancyGrid::new(columns, rows);
    Box::into_raw(Box::new(grid)) as jlong
}

/// Kotlin: `external fun nativeDestroyDesktopGrid(handle: Long)`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDestroyDesktopGrid(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    // SAFETY: caller contract — handle was produced by nativeCreateDesktopGrid
    // and has not already been passed to nativeDestroyDesktopGrid.
    unsafe {
        drop(Box::from_raw(handle as *mut DesktopOccupancyGrid));
    }
}

/// Kotlin: `external fun nativeDesktopGridClear(handle: Long)`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDesktopGridClear(
    _env: JNIEnv,
    _class: JClass,
    handle: jlong,
) {
    if handle == 0 {
        return;
    }
    let grid = unsafe { &mut *(handle as *mut DesktopOccupancyGrid) };
    grid.clear();
}

/// Kotlin: `external fun nativeDesktopGridPlace(handle: Long, itemId: String, col: Int, row: Int, colSpan: Int, rowSpan: Int)`
/// Used to mirror a View child's current (col,row) into the Rust-side grid
/// before a drop is resolved — see the module-level note above on why
/// Kotlin still walks its own View children first.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDesktopGridPlace(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: JString,
    col: jint,
    row: jint,
    col_span: jint,
    row_span: jint,
) {
    if handle == 0 {
        return;
    }
    let item_id: String = match env.get_string(&item_id) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let grid = unsafe { &mut *(handle as *mut DesktopOccupancyGrid) };
    grid.place(&item_id, CellRect::with_span(col, row, col_span, row_span));
}

/// Kotlin: `external fun nativeDesktopGridRemove(handle: Long, itemId: String)`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDesktopGridRemove(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: JString,
) {
    if handle == 0 {
        return;
    }
    let item_id: String = match env.get_string(&item_id) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    let grid = unsafe { &mut *(handle as *mut DesktopOccupancyGrid) };
    grid.remove(&item_id);
}

/// Kotlin: `external fun nativePixelToCell(containerWidth: Int, containerHeight: Int, columns: Int, rows: Int, x: Int, y: Int): IntArray`
/// Returns `[col, row]`. A plain 2-element IntArray is used (not a jlong-packed
/// pair or a custom object) since jni 0.21's primitive-array marshaling
/// (`new_int_array` + `set_int_array_region`) is simpler and cheaper than
/// object-array/JSON marshaling for a fixed 2-element case — see
/// nativeBuildGrantAllCommands above for the heavier jobjectArray<String>
/// pattern used where the element type isn't a JNI primitive.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativePixelToCell(
    env: JNIEnv,
    _class: JClass,
    container_width: jint,
    container_height: jint,
    columns: jint,
    rows: jint,
    x: jint,
    y: jint,
) -> jni::sys::jintArray {
    let (col, row) = crate::desktop_grid::pixel_to_cell(
        x,
        y,
        container_width,
        container_height,
        columns,
        rows,
    );
    make_int_array_2(&env, col, row)
}

/// Kotlin: `external fun nativeCellToPixel(containerWidth: Int, containerHeight: Int, columns: Int, rows: Int, col: Int, row: Int): IntArray`
/// Returns `[pixelX, pixelY]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeCellToPixel(
    env: JNIEnv,
    _class: JClass,
    container_width: jint,
    container_height: jint,
    columns: jint,
    rows: jint,
    col: jint,
    row: jint,
) -> jni::sys::jintArray {
    let (px, py) = crate::desktop_grid::cell_to_pixel(
        col,
        row,
        container_width,
        container_height,
        columns,
        rows,
    );
    make_int_array_2(&env, px, py)
}

/// Kotlin: `external fun nativeResolveDrop(handle: Long, itemId: String, draggedCurrentCol: Int, draggedCurrentRow: Int, dropPxX: Int, dropPxY: Int, containerWidth: Int, containerHeight: Int): String`
///
/// Returns a flat, semicolon-separated "itemId,col,row,pixelX,pixelY" list
/// (one entry per item whose position is now current — the dragged item
/// plus any items it pushed), one line per item joined with `;`. A small
/// bespoke text format rather than a jobjectArray<CustomObject> or JSON, to
/// avoid needing an env.new_object/set_field_id dance for a Kotlin data
/// class across JNI (same category of complexity concern the original
/// jni_bridge.rs flagged for parse_users/parse_task_list's Vec<struct>
/// TODO) — Kotlin splits on `;` then `,`, four cheap String operations, no
/// JSON dependency needed on either side for this one call.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeResolveDrop(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: JString,
    dragged_current_col: jint,
    dragged_current_row: jint,
    drop_px_x: jint,
    drop_px_y: jint,
    container_width: jint,
    container_height: jint,
) -> jstring {
    if handle == 0 {
        return std::ptr::null_mut();
    }
    let item_id: String = match env.get_string(&item_id) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let grid = unsafe { &mut *(handle as *mut DesktopOccupancyGrid) };
    let placements = crate::desktop_grid::resolve_drop(
        grid,
        &item_id,
        dragged_current_col,
        dragged_current_row,
        drop_px_x,
        drop_px_y,
        container_width,
        container_height,
    );
    let encoded = placements
        .iter()
        .map(|p| format!("{},{},{},{},{}", p.item_id, p.col, p.row, p.pixel_x, p.pixel_y))
        .collect::<Vec<_>>()
        .join(";");
    match env.new_string(encoded) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeFirstFreeRect(handle: Long, colSpan: Int, rowSpan: Int): IntArray`
/// Returns `[col, row]`, or `[-1, -1]` if no free rect of that span exists.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeFirstFreeRect(
    env: JNIEnv,
    _class: JClass,
    handle: jlong,
    col_span: jint,
    row_span: jint,
) -> jni::sys::jintArray {
    if handle == 0 {
        return make_int_array_2(&env, -1, -1);
    }
    let grid = unsafe { &*(handle as *const DesktopOccupancyGrid) };
    match grid.first_free_rect(col_span, row_span, None) {
        Some(rect) => make_int_array_2(&env, rect.col, rect.row),
        None => make_int_array_2(&env, -1, -1),
    }
}

/// Kotlin: `external fun nativeDesktopGridPlaceInFreeCell(handle: Long, itemId: String, preferredCol: Int, preferredRow: Int): IntArray`
/// Returns `[col, row]` actually used (preferred, if free; otherwise the
/// first available free cell).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDesktopGridPlaceInFreeCell(
    mut env: JNIEnv,
    _class: JClass,
    handle: jlong,
    item_id: JString,
    preferred_col: jint,
    preferred_row: jint,
) -> jni::sys::jintArray {
    if handle == 0 {
        return make_int_array_2(&env, preferred_col, preferred_row);
    }
    let item_id: String = match env.get_string(&item_id) {
        Ok(s) => s.into(),
        Err(_) => return make_int_array_2(&env, preferred_col, preferred_row),
    };
    let grid = unsafe { &mut *(handle as *mut DesktopOccupancyGrid) };
    let rect = grid.place_in_free_cell(&item_id, preferred_col, preferred_row);
    make_int_array_2(&env, rect.col, rect.row)
}

/// Shared helper: builds a 2-element `jintArray` `[a, b]`. Returns a
/// zero-length array (never null) if allocation fails, so call sites never
/// need a null check on top of an index-out-of-bounds one.
fn make_int_array_2(env: &JNIEnv, a: i32, b: i32) -> jni::sys::jintArray {
    let arr = match env.new_int_array(2) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    if env.set_int_array_region(&arr, 0, &[a, b]).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

// ── shell_client.rs bridge ──────────────────────────────────────────────
// Replaces ShellManager.kt's socket-handling logic. What stays in Kotlin:
// the SHELL_SERVER_READY BroadcastReceiver (calls nativeSetSessionToken
// with the extracted token), and the Shizuku/Root launch calls (Binder-
// bound framework APIs — same category as root.rs's own root-check split).

/// Kotlin: `external fun nativeSetSessionToken(token: String)`
/// Called from the SHELL_SERVER_READY BroadcastReceiver's onReceive.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSetSessionToken(
    mut env: JNIEnv,
    _class: JClass,
    token: JString,
) {
    let token: String = match env.get_string(&token) {
        Ok(s) => s.into(),
        Err(_) => return,
    };
    crate::shell_client::set_session_token(token);
}

/// Kotlin: `external fun nativeClearShellSession()`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeClearShellSession(
    _env: JNIEnv,
    _class: JClass,
) {
    crate::shell_client::clear_session();
}

/// Kotlin: `external fun nativeIsShellAvailable(): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsShellAvailable(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if crate::shell_client::is_available() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeShellExecSync(cmd: String): String?`
/// Blocking — call from Dispatchers.IO. Returns null on failure/no session.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeShellExecSync(
    mut env: JNIEnv,
    _class: JClass,
    cmd: JString,
) -> jstring {
    let cmd: String = match env.get_string(&cmd) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    match crate::shell_client::exec_sync(&cmd) {
        Some(result) => match env.new_string(result) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeShellPing(): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeShellPing(
    _env: JNIEnv,
    _class: JClass,
) -> jboolean {
    if crate::shell_client::ping() {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ── window_geometry.rs bridge ───────────────────────────────────────────
// Replaces the pure-math portions of AppUtils.kt — see window_geometry.rs's
// module doc for exactly which functions moved vs stayed in Kotlin.

use crate::window_geometry::{self, LaunchBounds};

/// Kotlin: `external fun nativeComputeLaunchBounds(mode: String, deviceWidth: Int, deviceHeight: Int, statusHeight: Int, navHeight: Int, dockHeight: Int, applyNavbarFix: Boolean, scaleFactor: Float): IntArray`
/// Returns `[left, top, right, bottom]`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeComputeLaunchBounds(
    mut env: JNIEnv,
    _class: JClass,
    mode: JString,
    device_width: jint,
    device_height: jint,
    status_height: jint,
    nav_height: jint,
    dock_height: jint,
    apply_navbar_fix: jboolean,
    scale_factor: jfloat,
) -> jni::sys::jintArray {
    let mode: String = match env.get_string(&mode) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let b = window_geometry::compute_launch_bounds(
        &mode,
        device_width,
        device_height,
        status_height,
        nav_height,
        dock_height,
        apply_navbar_fix != JNI_FALSE,
        scale_factor,
    );
    let arr = match env.new_int_array(4) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    if env.set_int_array_region(&arr, 0, &[b.left, b.top, b.right, b.bottom]).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeBuildLaunchCommand(component: String, mode: String, left: Int, top: Int, right: Int, bottom: Int, displayId: Int): String`
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildLaunchCommand(
    mut env: JNIEnv,
    _class: JClass,
    component: JString,
    mode: JString,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
    display_id: jint,
) -> jstring {
    let component: String = match env.get_string(&component) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let mode: String = match env.get_string(&mode) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let bounds = LaunchBounds { left, top, right, bottom };
    let cmd = window_geometry::build_launch_command(&component, &mode, bounds, display_id);
    match env.new_string(cmd) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildResizeCommands(taskId: Int, left: Int, top: Int, right: Int, bottom: Int): Array<String>`
/// Returns `[modeCmd, resizeCmd]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildResizeCommands<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    task_id: jint,
    left: jint,
    top: jint,
    right: jint,
    bottom: jint,
) -> jni::sys::jobjectArray {
    let bounds = LaunchBounds { left, top, right, bottom };
    let (mode_cmd, resize_cmd) = window_geometry::build_resize_commands(task_id, bounds);

    // Build both Java strings FIRST, before touching find_class/new_object_array —
    // avoids any question of overlapping &mut env borrows across calls, since
    // each `env.*` call here is a fully separate, sequential statement with no
    // value from a previous call still borrowing `env` when the next one runs.
    let j_mode_cmd = match env.new_string(mode_cmd) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let j_resize_cmd = match env.new_string(resize_cmd) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };

    let string_class = match env.find_class("java/lang/String") {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };

    // `new_object_array`'s initial-value parameter takes `impl Into<JObject>`;
    // JString -> JObject is an explicit `.into()` (JString derefs to JObject
    // but does not implicitly coerce through a generic `Into` bound), and
    // since we need TWO distinct String objects in the array (not the same
    // initial value at both indices, which is what passing one JString as
    // the fill value for a size-2 array would give us for the ELEMENT we
    // don't immediately overwrite), we create the array with a `null`
    // initial element and set both indices explicitly instead of relying
    // on the constructor's fill value for either slot.
    let arr = match env.new_object_array(2, string_class, jni::objects::JObject::null()) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    if env.set_object_array_element(&arr, 0, j_mode_cmd).is_err() {
        return std::ptr::null_mut();
    }
    if env.set_object_array_element(&arr, 1, j_resize_cmd).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeDockSizeConfig(preset: String, normalDockHeightDp: Int): IntArray`
/// Returns `[dockHeightDp, iconSizeDp, gridSizeDp, useSystemDensity (0/1)]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDockSizeConfig(
    mut env: JNIEnv,
    _class: JClass,
    preset: JString,
    normal_dock_height_dp: jint,
) -> jni::sys::jintArray {
    let preset: String = match env.get_string(&preset) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let cfg = window_geometry::dock_size_config(&preset, normal_dock_height_dp);
    let arr = match env.new_int_array(4) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    let use_system_density = if cfg.use_system_density { 1 } else { 0 };
    if env
        .set_int_array_region(
            &arr,
            0,
            &[cfg.dock_height_dp, cfg.icon_size_dp, cfg.grid_size_dp, use_system_density],
        )
        .is_err()
    {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

// ── textparse.rs::build_provision_steps bridge ────────────────────────────
// Replaces MultiUserManager.kt's provisionUser() steps-list construction.

/// Kotlin: `external fun nativeBuildProvisionSteps(pkg: String, userId: Int): String`
///
/// Returns a semicolon-separated `"command,label"` list — same bespoke
/// text-encoding pattern as nativeParseUsers/nativeResolveDrop above
/// (avoids jobjectArray<Pair<String,String>> marshaling for a small,
/// fixed-shape list). Neither `command` nor `label` can contain a `,` or
/// `;`: commands are built entirely from fixed shell-command templates,
/// package names (Android enforces `[a-zA-Z0-9_.]` for package names —
/// no comma/semicolon possible), and integer user ids; labels are fixed
/// English strings from a hardcoded list in build_provision_steps — see
/// that function's doc comment for the caveat about which three labels
/// were originally localized string-resource lookups on the Kotlin side.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildProvisionSteps(
    mut env: JNIEnv,
    _class: JClass,
    pkg: JString,
    user_id: jint,
) -> jstring {
    let pkg: String = match env.get_string(&pkg) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let steps = crate::textparse::build_provision_steps(&pkg, user_id);
    let encoded = steps
        .iter()
        .map(|s| format!("{},{}", s.command, s.label))
        .collect::<Vec<_>>()
        .join(";");
    match env.new_string(encoded) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── textparse.rs::battery_drawable_bucket bridge ──────────────────────────
// Replaces Utils.kt's getBatteryDrawable() classification logic.

/// Kotlin: `external fun nativeBatteryDrawableBucket(level: Int, plugged: Boolean): String`
/// Returns one of "empty"/"20"/"30"/"50"/"60"/"80"/"90"/"full" — Kotlin
/// maps this to the actual R.drawable.battery_* / R.drawable.battery_charging_*
/// constant (resource ids don't exist on the Rust side).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBatteryDrawableBucket(
    env: JNIEnv,
    _class: JClass,
    level: jint,
    plugged: jboolean,
) -> jstring {
    let bucket = crate::textparse::battery_drawable_bucket(level, plugged != JNI_FALSE);
    match env.new_string(bucket) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── textparse.rs::parse_foreground_package / parse_running_packages bridge ──
// Replaces ShizukoManager.kt's dumpsys-output regex parsing.

/// Kotlin: `external fun nativeParseForegroundPackage(raw: String): String?`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseForegroundPackage(
    mut env: JNIEnv,
    _class: JClass,
    raw: JString,
) -> jstring {
    let raw: String = match env.get_string(&raw) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    match crate::textparse::parse_foreground_package(&raw) {
        Some(pkg) => match env.new_string(pkg) {
            Ok(s) => s.into_raw(),
            Err(_) => std::ptr::null_mut(),
        },
        None => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeParseRunningPackages(raw: String): String`
/// Returns a semicolon-separated package-name list (never contains `;` —
/// Android package names are restricted to `[a-zA-Z0-9_.]`).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseRunningPackages(
    mut env: JNIEnv,
    _class: JClass,
    raw: JString,
) -> jstring {
    let raw: String = match env.get_string(&raw) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let pkgs = crate::textparse::parse_running_packages(&raw);
    match env.new_string(pkgs.join(";")) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── window_geometry.rs::swipe_direction bridge ────────────────────────────
// Replaces OnSwipeListener.kt's angle/direction computation.

/// Kotlin: `external fun nativeSwipeDirection(x1: Float, y1: Float, x2: Float, y2: Float): String`
/// Returns "UP"/"DOWN"/"LEFT"/"RIGHT".
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSwipeDirection(
    env: JNIEnv,
    _class: JClass,
    x1: jfloat,
    y1: jfloat,
    x2: jfloat,
    y2: jfloat,
) -> jstring {
    let dir = crate::window_geometry::swipe_direction(x1, y1, x2, y2);
    match env.new_string(dir.as_str()) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── textparse.rs::contains_arabic / choose_font_for_text bridge ─────────
// Replaces FontManager.kt's Arabic-detection and font-choice logic.

/// Kotlin: `external fun nativeContainsArabic(text: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeContainsArabic(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) -> jboolean {
    let text: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    if crate::textparse::contains_arabic(&text) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeChooseFontForText(text: String, hasArabicTf: Boolean, hasLatinTf: Boolean): String`
/// Returns "ARABIC"/"LATIN"/"NONE" — Kotlin maps that back to its actual
/// Typeface? reference (this crate doesn't hold Typeface objects).
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeChooseFontForText(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
    has_arabic_tf: jboolean,
    has_latin_tf: jboolean,
) -> jstring {
    let text: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let choice = crate::textparse::choose_font_for_text(
        &text,
        has_arabic_tf != JNI_FALSE,
        has_latin_tf != JNI_FALSE,
    );
    let s = match choice {
        crate::textparse::FontChoice::Arabic => "ARABIC",
        crate::textparse::FontChoice::Latin => "LATIN",
        crate::textparse::FontChoice::None => "NONE",
    };
    match env.new_string(s) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── window_geometry.rs::is_bubble_light bridge ───────────────────────────
// Replaces UserSwitcherPopup.kt's inline WCAG luminance calculation.

/// Kotlin: `external fun nativeIsBubbleLight(argb: Int): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsBubbleLight(
    _env: JNIEnv,
    _class: JClass,
    argb: jint,
) -> jboolean {
    if crate::window_geometry::is_bubble_light(argb) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ── window_geometry.rs::fit_surface_to_video / is_exit_swipe bridge ──────
// Replaces EasterEggActivity.kt's letterbox math and swipe-threshold check.

/// Kotlin: `external fun nativeFitSurfaceToVideo(videoW: Int, videoH: Int, screenW: Int, screenH: Int): IntArray?`
/// Returns `[targetWidth, targetHeight]`, or null if videoW/videoH <= 0.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeFitSurfaceToVideo(
    env: JNIEnv,
    _class: JClass,
    video_w: jint,
    video_h: jint,
    screen_w: jint,
    screen_h: jint,
) -> jni::sys::jintArray {
    match crate::window_geometry::fit_surface_to_video(video_w, video_h, screen_w, screen_h) {
        Some((w, h)) => {
            let arr = match env.new_int_array(2) {
                Ok(a) => a,
                Err(_) => return std::ptr::null_mut(),
            };
            if env.set_int_array_region(&arr, 0, &[w, h]).is_err() {
                return std::ptr::null_mut();
            }
            arr.into_raw()
        }
        None => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeIsExitSwipe(dx: Float, dy: Float, velocityX: Float, minDistancePx: Float, minVelocity: Float): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsExitSwipe(
    _env: JNIEnv,
    _class: JClass,
    dx: jfloat,
    dy: jfloat,
    velocity_x: jfloat,
    min_distance_px: jfloat,
    min_velocity: jfloat,
) -> jboolean {
    if crate::window_geometry::is_exit_swipe(dx, dy, velocity_x, min_distance_px, min_velocity) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

// ── textparse.rs::WorkshopSourcesActivity helpers bridge ─────────────────

/// Kotlin: `external fun nativeNormalizeSourceUrl(url: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeNormalizeSourceUrl(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jstring {
    let url: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::normalize_source_url(&url);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildSearchUrlTemplate(url: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildSearchUrlTemplate(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jstring {
    let url: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::build_search_url_template(&url);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeValidateSourceFields(name: String, url: String, finalType: String): String?`
/// Returns "NAME_BLANK"/"URL_INVALID"/"TYPE_BLANK", or null if valid.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeValidateSourceFields(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
    url: JString,
    final_type: JString,
) -> jstring {
    let name: String = match env.get_string(&name) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let url: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let final_type: String = match env.get_string(&final_type) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    match crate::textparse::validate_source_fields(&name, &url, &final_type) {
        None => std::ptr::null_mut(),
        Some(err) => {
            let s = match err {
                crate::textparse::SourceValidationError::NameBlank => "NAME_BLANK",
                crate::textparse::SourceValidationError::UrlInvalid => "URL_INVALID",
                crate::textparse::SourceValidationError::TypeBlank => "TYPE_BLANK",
            };
            match env.new_string(s) {
                Ok(s) => s.into_raw(),
                Err(_) => std::ptr::null_mut(),
            }
        }
    }
}

// ── textparse.rs::parse_cpu_usage bridge ──────────────────────────────────
// Replaces PerfectServer.kt's getCpuUsage() parsing + delta arithmetic.
// Kotlin still does the /proc/stat file read and owns the prevTotal/
// prevIdle/lastValue state across calls — see parse_cpu_usage's doc comment.

/// Kotlin: `external fun nativeParseCpuUsage(line: String, prevTotal: Long, prevIdle: Long, lastValue: Int): LongArray?`
/// Returns `[percent, total, idlePlusIowait]` packed as longs (percent
/// fits safely; using one array type avoids a second JNI call), or null
/// if `line` doesn't parse — Kotlin should keep showing its own
/// `lastCpuValue` and NOT update prevTotal/prevIdle in that case, same as
/// the original's `if (parts.size < 4) return lastCpuValue` guard.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseCpuUsage(
    mut env: JNIEnv,
    _class: JClass,
    line: JString,
    prev_total: jlong,
    prev_idle: jlong,
    last_value: jint,
) -> jni::sys::jlongArray {
    let line: String = match env.get_string(&line) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    match crate::textparse::parse_cpu_usage(&line, prev_total, prev_idle, last_value) {
        Some(sample) => {
            let arr = match env.new_long_array(3) {
                Ok(a) => a,
                Err(_) => return std::ptr::null_mut(),
            };
            let values: [i64; 3] = [sample.percent as i64, sample.total, sample.idle_plus_iowait];
            if env.set_long_array_region(&arr, 0, &values).is_err() {
                return std::ptr::null_mut();
            }
            arr.into_raw()
        }
        None => std::ptr::null_mut(),
    }
}

// ── window_geometry.rs::dock_layout_preset bridge ─────────────────────────
// Replaces DockLayoutDialog.kt's per-`which` settings-bundle selection.

/// Kotlin: `external fun nativeDockLayoutPreset(which: Int): BooleanArray` for the 6 bools,
/// paired with `external fun nativeDockLayoutPresetStrings(which: Int): Array<String>` for the 4 strings.
/// Split into two calls since JNI has no single heterogeneous-array return type.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDockLayoutPresetBools(
    env: JNIEnv,
    _class: JClass,
    which: jint,
) -> jni::sys::jbooleanArray {
    let p = crate::window_geometry::dock_layout_preset(which);
    let arr = match env.new_boolean_array(5) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    let values: [jboolean; 5] = [
        if p.enable_nav { JNI_TRUE } else { JNI_FALSE },
        if p.enable_qs_wifi_vol_date_notif { JNI_TRUE } else { JNI_FALSE },
        if p.app_menu_fullscreen { JNI_TRUE } else { JNI_FALSE },
        if p.show_notifications { JNI_TRUE } else { JNI_FALSE },
        if p.enable_qs_pin { JNI_TRUE } else { JNI_FALSE },
    ];
    if env.set_boolean_array_region(&arr, 0, &values).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeDockLayoutPresetStrings(which: Int): Array<String>`
/// Returns `[maxRunningApps, maxRunningAppsLandscape, dockActivationArea, activationMethod]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeDockLayoutPresetStrings(
    mut env: JNIEnv,
    _class: JClass,
    which: jint,
) -> jni::sys::jobjectArray {
    let p = crate::window_geometry::dock_layout_preset(which);
    let values = [
        p.max_running_apps,
        p.max_running_apps_landscape,
        p.dock_activation_area,
        p.activation_method,
    ];

    let j_strings: Vec<_> = match values.iter().map(|s| env.new_string(s)).collect::<Result<Vec<_>, _>>() {
        Ok(v) => v,
        Err(_) => return std::ptr::null_mut(),
    };
    let string_class = match env.find_class("java/lang/String") {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };
    let arr = match env.new_object_array(4, string_class, jni::objects::JObject::null()) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    for (i, s) in j_strings.into_iter().enumerate() {
        if env.set_object_array_element(&arr, i as i32, s).is_err() {
            return std::ptr::null_mut();
        }
    }
    arr.into_raw()
}

// ── textparse.rs::fragments batch 1 bridge ────────────────────────────────

/// Kotlin: `external fun nativeParseGridPreset(value: String, defaultCols: Int, defaultRows: Int): IntArray`
/// Returns `[cols, rows]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseGridPreset(
    mut env: JNIEnv,
    _class: JClass,
    value: JString,
    default_cols: jint,
    default_rows: jint,
) -> jni::sys::jintArray {
    let value: String = match env.get_string(&value) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let (cols, rows) = crate::textparse::parse_grid_preset(&value, default_cols, default_rows);
    let arr = match env.new_int_array(2) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    if env.set_int_array_region(&arr, 0, &[cols, rows]).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeParseResolutionString(text: String): IntArray?`
/// Returns `[width, height]`, or `null` if the text doesn't parse.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseResolutionString(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) -> jni::sys::jintArray {
    let text: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    match crate::textparse::parse_resolution_string(&text) {
        Some((w, h)) => {
            let arr = match env.new_int_array(2) {
                Ok(a) => a,
                Err(_) => return std::ptr::null_mut(),
            };
            if env.set_int_array_region(&arr, 0, &[w, h]).is_err() {
                return std::ptr::null_mut();
            }
            arr.into_raw()
        }
        None => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeNaturalDisplaySize(width: Int, height: Int): IntArray`
/// Returns `[naturalWidth, naturalHeight]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeNaturalDisplaySize(
    env: JNIEnv,
    _class: JClass,
    width: jint,
    height: jint,
) -> jni::sys::jintArray {
    let (nw, nh) = crate::textparse::natural_display_size(width, height);
    let arr = match env.new_int_array(2) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    if env.set_int_array_region(&arr, 0, &[nw, nh]).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeFileIconKind(ext: String): Int`
/// Returns an ordinal: 0=Video, 1=Audio, 2=Image, 3=Pdf, 4=Archive, 5=Font,
/// 6=Apk, 7=Text, 8=Generic. Kotlin maps the ordinal to its `R.drawable.ic_*`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeFileIconKind(
    mut env: JNIEnv,
    _class: JClass,
    ext: JString,
) -> jint {
    let ext: String = match env.get_string(&ext) {
        Ok(s) => s.into(),
        Err(_) => return 8,
    };
    use crate::textparse::FileIconKind::*;
    match crate::textparse::file_icon_kind(&ext) {
        Video => 0,
        Audio => 1,
        Image => 2,
        Pdf => 3,
        Archive => 4,
        Font => 5,
        Apk => 6,
        Text => 7,
        Generic => 8,
    }
}

/// Kotlin: `external fun nativeExtToMime(ext: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeExtToMime(
    mut env: JNIEnv,
    _class: JClass,
    ext: JString,
) -> jstring {
    let ext: String = match env.get_string(&ext) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::ext_to_mime(&ext);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeMimeMatches(allowedMimes: Array<String>, fileExt: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeMimeMatches(
    mut env: JNIEnv,
    _class: JClass,
    allowed_mimes: jni::objects::JObjectArray,
    file_ext: JString,
) -> jboolean {
    let file_ext: String = match env.get_string(&file_ext) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    let len = match env.get_array_length(&allowed_mimes) {
        Ok(l) => l,
        Err(_) => return JNI_FALSE,
    };
    let mut owned: Vec<String> = Vec::with_capacity(len as usize);
    for i in 0..len {
        let elem = match env.get_object_array_element(&allowed_mimes, i) {
            Ok(e) => e,
            Err(_) => return JNI_FALSE,
        };
        let jstr = jni::objects::JString::from(elem);
        let s: String = match env.get_string(&jstr) {
            Ok(s) => s.into(),
            Err(_) => return JNI_FALSE,
        };
        owned.push(s);
    }
    let refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
    if crate::textparse::mime_matches(&refs, &file_ext) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeFormatDurationMmSs(ms: Int): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeFormatDurationMmSs(
    env: JNIEnv,
    _class: JClass,
    ms: jint,
) -> jstring {
    let result = crate::textparse::format_duration_mmss(ms);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildChmodCommands(filePath: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildChmodCommands(
    mut env: JNIEnv,
    _class: JClass,
    file_path: JString,
) -> jstring {
    let file_path: String = match env.get_string(&file_path) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::build_chmod_commands(&file_path);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeEasterEggTapState(nowMs: Long, count: Int, firstTapAtMs: Long, windowMs: Long, tapsRequired: Int): LongArray`
/// Returns `[newCount, newFirstTapAtMs, reachedThreshold(0/1)]`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeEasterEggTapState(
    env: JNIEnv,
    _class: JClass,
    now_ms: jlong,
    count: jint,
    first_tap_at_ms: jlong,
    window_ms: jlong,
    taps_required: jint,
) -> jni::sys::jlongArray {
    let (new_count, new_first_tap_at, reached) = crate::textparse::easter_egg_tap_state(
        now_ms,
        count,
        first_tap_at_ms,
        window_ms,
        taps_required,
    );
    let arr = match env.new_long_array(3) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    let reached_val: i64 = if reached { 1 } else { 0 };
    if env
        .set_long_array_region(&arr, 0, &[new_count as i64, new_first_tap_at, reached_val])
        .is_err()
    {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeIsAdUrl(url: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsAdUrl(
    mut env: JNIEnv,
    _class: JClass,
    url: JString,
) -> jboolean {
    let url: String = match env.get_string(&url) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    if crate::textparse::is_ad_url(&url) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeMimeToExt(mime: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeMimeToExt(
    mut env: JNIEnv,
    _class: JClass,
    mime: JString,
) -> jstring {
    let mime: String = match env.get_string(&mime) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::mime_to_ext(&mime);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeNameFromContentDisposition(cd: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeNameFromContentDisposition(
    mut env: JNIEnv,
    _class: JClass,
    cd: JString,
) -> jstring {
    let cd: String = match env.get_string(&cd) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::name_from_content_disposition(&cd);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeFormatFileSize(bytes: Long): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeFormatFileSize(
    env: JNIEnv,
    _class: JClass,
    bytes: jlong,
) -> jstring {
    let result = crate::textparse::format_file_size(bytes);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeSanitizeFilenameBase(base: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSanitizeFilenameBase(
    mut env: JNIEnv,
    _class: JClass,
    base: JString,
) -> jstring {
    let base: String = match env.get_string(&base) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::sanitize_filename_base(&base);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── textparse.rs::PerformanceFragment bridge ───────────────────────────────

/// Kotlin: `external fun nativeBuildFpsCommand(hz: Int, reset: Boolean): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildFpsCommand(
    env: JNIEnv,
    _class: JClass,
    hz: jint,
    reset: jboolean,
) -> jstring {
    let result = crate::textparse::build_fps_command(hz, reset == JNI_TRUE);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildThermalCommand(mode: Int): String`
/// `mode` ordinal: 0=Balanced, 1=Performance, 2=Gaming, 3=Powersave.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildThermalCommand(
    env: JNIEnv,
    _class: JClass,
    mode: jint,
) -> jstring {
    use crate::textparse::ThermalMode::*;
    let mode = match mode {
        0 => Balanced,
        1 => Performance,
        2 => Gaming,
        _ => Powersave,
    };
    let result = crate::textparse::build_thermal_command(mode);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildZramEnableCommand(bytes: Long): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildZramEnableCommand(
    env: JNIEnv,
    _class: JClass,
    bytes: jlong,
) -> jstring {
    let result = crate::textparse::build_zram_enable_command(bytes);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildZramDisableCommand(): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildZramDisableCommand(
    env: JNIEnv,
    _class: JClass,
) -> jstring {
    let result = crate::textparse::build_zram_disable_command();
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeZramBucketLabel(bytes: Long, offLabel: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeZramBucketLabel(
    mut env: JNIEnv,
    _class: JClass,
    bytes: jlong,
    off_label: JString,
) -> jstring {
    let off_label: String = match env.get_string(&off_label) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::zram_bucket_label(bytes, &off_label);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

/// Kotlin: `external fun nativeBuildInlineShellCommand(cmd: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeBuildInlineShellCommand(
    mut env: JNIEnv,
    _class: JClass,
    cmd: JString,
) -> jstring {
    let cmd: String = match env.get_string(&cmd) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::build_inline_shell_command(&cmd);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── textparse.rs::small sections bridge (AppAdapter/Utils/plugins) ────────

/// Kotlin: `external fun nativeIsCalcExpression(text: String): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeIsCalcExpression(
    mut env: JNIEnv,
    _class: JClass,
    text: JString,
) -> jboolean {
    let text: String = match env.get_string(&text) {
        Ok(s) => s.into(),
        Err(_) => return JNI_FALSE,
    };
    if crate::textparse::is_calc_expression(&text) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Kotlin: `external fun nativeSolveArithmeticExpression(expression: String): Double`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSolveArithmeticExpression(
    mut env: JNIEnv,
    _class: JClass,
    expression: JString,
) -> jni::sys::jdouble {
    let expression: String = match env.get_string(&expression) {
        Ok(s) => s.into(),
        Err(_) => return 0.0,
    };
    crate::textparse::solve_arithmetic_expression(&expression)
}

/// Kotlin: `external fun nativeParseModulePropLine(line: String): Array<String>?`
/// Returns `[key, value]`, or `null` if the line has no `=`.
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeParseModulePropLine(
    mut env: JNIEnv,
    _class: JClass,
    line: JString,
) -> jni::sys::jobjectArray {
    let line: String = match env.get_string(&line) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let (key, value) = match crate::textparse::parse_module_prop_line(&line) {
        Some(kv) => kv,
        None => return std::ptr::null_mut(),
    };
    let string_class = match env.find_class("java/lang/String") {
        Ok(c) => c,
        Err(_) => return std::ptr::null_mut(),
    };
    let key_jstr = match env.new_string(&key) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    let arr = match env.new_object_array(2, &string_class, &key_jstr) {
        Ok(a) => a,
        Err(_) => return std::ptr::null_mut(),
    };
    let value_jstr = match env.new_string(&value) {
        Ok(s) => s,
        Err(_) => return std::ptr::null_mut(),
    };
    if env.set_object_array_element(&arr, 1, &value_jstr).is_err() {
        return std::ptr::null_mut();
    }
    arr.into_raw()
}

/// Kotlin: `external fun nativeSanitizePluginId(raw: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSanitizePluginId(
    mut env: JNIEnv,
    _class: JClass,
    raw: JString,
) -> jstring {
    let raw: String = match env.get_string(&raw) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::sanitize_plugin_id(&raw);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── textparse.rs::VideoFileStore bridge ────────────────────────────────────

/// Kotlin: `external fun nativeSanitizeVideoFilename(name: String): String`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativeSanitizeVideoFilename(
    mut env: JNIEnv,
    _class: JClass,
    name: JString,
) -> jstring {
    let name: String = match env.get_string(&name) {
        Ok(s) => s.into(),
        Err(_) => return std::ptr::null_mut(),
    };
    let result = crate::textparse::sanitize_video_filename(&name);
    match env.new_string(result) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}

// ── window_geometry.rs::PerfectServer bridge ────────────────────────────────

/// Kotlin: `external fun nativePointInViewBounds(rawX: Float, rawY: Float, viewScreenX: Int, viewScreenY: Int, viewWidth: Int, viewHeight: Int): Boolean`
#[no_mangle]
pub extern "system" fn Java_com_youki_dex_livewallpaper_NativeBridge_nativePointInViewBounds(
    _env: JNIEnv,
    _class: JClass,
    raw_x: jfloat,
    raw_y: jfloat,
    view_screen_x: jint,
    view_screen_y: jint,
    view_width: jint,
    view_height: jint,
) -> jboolean {
    if crate::window_geometry::point_in_view_bounds(
        raw_x,
        raw_y,
        view_screen_x,
        view_screen_y,
        view_width,
        view_height,
    ) {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}
