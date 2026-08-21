package com.youki.dex.livewallpaper

import android.view.Surface

/**
 * Thin JNI declaration surface for `youki_engine` (the Rust core).
 *
 * This object holds ONLY `external fun` declarations — no logic, no state,
 * no computation. Every symbol below must match a `Java_com_youki_dex_livewallpaper_NativeBridge_*`
 * export in `app/rust/youki-engine/src/jni_bridge.rs` EXACTLY (JNI resolves
 * by mangled name; a mismatch fails at runtime with UnsatisfiedLinkError,
 * not at compile time).
 *
 * Per project decision: all logic — root/shell parsing, zip archive
 * handling, text parsing, and rendering (OpenGL ES / Vulkan) — lives in
 * Rust. Kotlin callers of this object are expected to be thin wrappers
 * themselves (see e.g. RootManager, FileManagerFragment, WallpaperGLEngine
 * equivalents), never containing the logic itself.
 */
object NativeBridge {

    init {
        System.loadLibrary("youki_engine")
    }

    // ── root.rs bridge ──────────────────────────────────────────────────

    /** Whether root access is available on this device. Cached natively; see [nativeInvalidateRootCache]. */
    external fun nativeIsRootAvailable(): Boolean

    /** Invalidates the native-side cached root-availability result. */
    external fun nativeInvalidateRootCache()

    /**
     * Executes [command] via `su -c`, blocking until it completes.
     * Call from a background dispatcher (e.g. Dispatchers.IO) — this does
     * not switch threads itself.
     */
    external fun nativeExecuteRoot(command: String): String

    /** Grants WRITE_SECURE_SETTINGS to [packageName] via root/shell, returning the raw command output. */
    external fun nativeGrantWriteSecureSettings(packageName: String): String

    /** Builds the full ordered list of `pm grant` / `appops set` shell commands for onboarding's "grant all" step. */
    external fun nativeBuildGrantAllCommands(packageName: String, sdkInt: Int): Array<String>

    // ── archive.rs bridge ────────────────────────────────────────────────

    /** Extracts the zip at [zipPath] into [destDir]. Returns success/failure. */
    external fun nativeExtractZip(zipPath: String, destDir: String, deleteAfter: Boolean): Boolean

    /** Compresses the single file/dir at [sourcePath] into a new zip at [zipPath]. */
    external fun nativeCompressToZip(sourcePath: String, zipPath: String): Boolean

    // ── textparse.rs bridge ─────────────────────────────────────────────

    /** Whether a shell command's raw [output] indicates success, per the project's existing success-parsing rules. */
    external fun nativeIsCommandSuccess(output: String): Boolean

    /** Sanitizes untrusted [input] before it's interpolated into a shell command. */
    external fun nativeSanitize(input: String): String

    /** Parses a newly-created Android user ID out of `pm create-user`'s raw output; -1 if no match. */
    external fun nativeExtractCreatedUserId(raw: String): Int

    /**
     * Parses `pm list users` raw output. Returns a semicolon-separated
     * `"id,name,isCurrent,isRunning"` list — decode with [parseUsersResult].
     */
    external fun nativeParseUsers(raw: String, currentUserId: Int): String

    /** One parsed user — see [nativeParseUsers]'s return-format doc. */
    data class ParsedUser(val id: Int, val name: String, val isCurrent: Boolean, val isRunning: Boolean)

    /** Decodes [nativeParseUsers]'s semicolon/comma-encoded return string. */
    fun parseUsersResult(encoded: String): List<ParsedUser> {
        if (encoded.isEmpty()) return emptyList()
        return encoded.split(";").mapNotNull { entry ->
            val parts = entry.split(",")
            if (parts.size != 4) return@mapNotNull null
            ParsedUser(
                id = parts[0].toIntOrNull() ?: return@mapNotNull null,
                name = parts[1],
                isCurrent = parts[2].toBoolean(),
                isRunning = parts[3].toBoolean(),
            )
        }
    }

    /**
     * Parses `am stack list` raw output into (taskId, packageName) entries,
     * excluding [selfPkg]/[launcherPkg]/anything containing "systemui",
     * de-duplicated by package, capped at [max] entries. Returns a
     * semicolon-separated `"taskId,packageName"` list — decode with
     * [parseTaskListResult].
     */
    external fun nativeParseTaskList(output: String, selfPkg: String, launcherPkg: String, max: Int): String

    /** One parsed running-task entry — see [nativeParseTaskList]'s return-format doc. */
    data class ParsedTaskEntry(val taskId: Int, val packageName: String)

    /** Decodes [nativeParseTaskList]'s semicolon/comma-encoded return string. */
    fun parseTaskListResult(encoded: String): List<ParsedTaskEntry> {
        if (encoded.isEmpty()) return emptyList()
        return encoded.split(";").mapNotNull { entry ->
            val parts = entry.split(",")
            if (parts.size != 2) return@mapNotNull null
            ParsedTaskEntry(
                taskId = parts[0].toIntOrNull() ?: return@mapNotNull null,
                packageName = parts[1],
            )
        }
    }

    /**
     * Case-insensitive alphabetical sort by [labels], returning the
     * matching [ids] reordered the same way. [labels] and [ids] must be
     * the same length (returns an empty array otherwise).
     */
    external fun nativeSortByLabelAlphabetical(labels: Array<String>, ids: IntArray): IntArray

    /**
     * Same as [nativeSortByLabelAlphabetical] but case-*sensitive*
     * (Kotlin's default codepoint-order String comparison) — use this one
     * for app-list sorts (matches the previous `compareBy { it.name }`
     * behavior); use the case-insensitive variant for user-list sorts.
     */
    external fun nativeSortByLabelCaseSensitive(labels: Array<String>, ids: IntArray): IntArray

    /**
     * Sorts [ids] by [lastUsedTimes] descending (most-recently-used
     * first), returning [ids] reordered. Arrays must be the same length.
     */
    external fun nativeSortByLastUsedDescending(lastUsedTimes: LongArray, ids: IntArray): IntArray

    // ── renderer bridge (OpenGL ES / Vulkan) ────────────────────────────

    /** Backend preference: matches Rust's BackendPreference variant order. */
    const val BACKEND_AUTO = 0
    const val BACKEND_FORCE_GLES = 1
    const val BACKEND_FORCE_VULKAN = 2

    /** Scale mode: matches Rust's ScaleMode variant order. */
    const val SCALE_STRETCH = 0
    const val SCALE_FIT = 1
    const val SCALE_COVER = 2
    const val SCALE_FREE = 3

    /**
     * Creates a renderer backend and returns an opaque native handle.
     * Ownership contract: the returned handle MUST be passed to
     * [nativeDestroyRenderer] exactly once, and never used again afterward
     * (use-after-free otherwise — this is a raw pointer on the Rust side).
     */
    external fun nativeCreateRenderer(preference: Int): Long

    /** Destroys the renderer identified by [handle]. See ownership contract above. */
    external fun nativeDestroyRenderer(handle: Long)

    /** Binds [surface] to the renderer's GPU context. Returns false if the surface/handle was invalid. */
    external fun nativeOnSurfaceCreated(handle: Long, surface: Surface): Boolean

    /** Notifies the renderer that the surface size changed. */
    external fun nativeOnSurfaceChanged(handle: Long, width: Int, height: Int)

    /** Draws one frame. [backgroundColor] is an "#AARRGGBB" hex string. */
    external fun nativeOnDrawFrame(
        handle: Long,
        scaleMode: Int,
        backgroundColor: String,
        brightness: Float,
        contrast: Float,
        saturation: Float,
        colorCorrectionEnabled: Boolean,
        hasNewFrame: Boolean,
    )

    /**
     * Returns a native handle to the video-input Surface (SurfaceTexture-backed
     * OES texture for GLES, AHardwareBuffer bridge for Vulkan) for ExoPlayer's
     * `setVideoSurface()`. 0 if the renderer handle is invalid.
     */
    external fun nativeVideoInputSurfaceHandle(handle: Long): Long

    /** Plain descriptive text of this device's Vulkan support, for onboarding / Advanced Settings. */
    external fun nativeQueryVulkanCapabilities(): String

    // ── desktop_grid bridge (launcher grid + drag-drop) ──────────────────
    //
    // All grid math (collision detection, Nova-style push-to-resolve
    // drag-drop, pixel<->cell conversion) lives in Rust — see
    // desktop_grid.rs. Kotlin's only remaining job around the grid is
    // walking its own View children (an inherently Framework-API-bound
    // step: LayoutParams/FrameLayout) and applying returned pixel
    // positions back to views. See DesktopGridManager for the thin
    // wrapper that owns a grid handle's lifecycle.

    /**
     * Creates a desktop occupancy grid and returns an opaque native handle.
     * Ownership contract: the returned handle MUST be passed to
     * [nativeDestroyDesktopGrid] exactly once, and never used again
     * afterward.
     */
    external fun nativeCreateDesktopGrid(columns: Int, rows: Int): Long

    /** Destroys the grid identified by [handle]. See ownership contract above. */
    external fun nativeDestroyDesktopGrid(handle: Long)

    /** Clears every placed item from the grid (does not destroy the handle). */
    external fun nativeDesktopGridClear(handle: Long)

    /**
     * Places [itemId] at the given cell, clamped to the grid's bounds.
     * Note: `external fun` cannot have default parameter values (JNI
     * requires every argument passed explicitly) — callers placing a
     * single-cell item must pass `colSpan = 1, rowSpan = 1` themselves;
     * see [nativeDesktopGridPlaceSingle] for that common case.
     */
    external fun nativeDesktopGridPlace(
        handle: Long,
        itemId: String,
        col: Int,
        row: Int,
        colSpan: Int,
        rowSpan: Int,
    )

    /** Convenience overload for the common single-cell (1x1) placement case. */
    fun nativeDesktopGridPlaceSingle(handle: Long, itemId: String, col: Int, row: Int) {
        nativeDesktopGridPlace(handle, itemId, col, row, 1, 1)
    }

    /** Removes [itemId] from the grid, if present. */
    external fun nativeDesktopGridRemove(handle: Long, itemId: String)

    /** Converts a pixel position to a `[col, row]` grid cell, clamped to bounds. */
    external fun nativePixelToCell(
        containerWidth: Int,
        containerHeight: Int,
        columns: Int,
        rows: Int,
        x: Int,
        y: Int,
    ): IntArray

    /** Converts a grid cell to its top-left `[pixelX, pixelY]` position. */
    external fun nativeCellToPixel(
        containerWidth: Int,
        containerHeight: Int,
        columns: Int,
        rows: Int,
        col: Int,
        row: Int,
    ): IntArray

    /**
     * Resolves a drag-drop release: places [itemId] at the dropped pixel
     * position, pushing neighbors out of the way (Nova-style) if occupied.
     * Returns a semicolon-separated `"itemId,col,row,pixelX,pixelY"` list —
     * one entry per item whose position is now current (the dragged item,
     * plus any items it pushed). Use [parseResolvedPlacements] to decode.
     */
    external fun nativeResolveDrop(
        handle: Long,
        itemId: String,
        draggedCurrentCol: Int,
        draggedCurrentRow: Int,
        dropPxX: Int,
        dropPxY: Int,
        containerWidth: Int,
        containerHeight: Int,
    ): String

    /** Returns `[col, row]` of the first free rect of the given span, or `[-1, -1]` if none exists. */
    external fun nativeFirstFreeRect(handle: Long, colSpan: Int, rowSpan: Int): IntArray

    /** Places [itemId] at ([preferredCol], [preferredRow]) if free, otherwise the first free cell. Returns the `[col, row]` actually used. */
    external fun nativeDesktopGridPlaceInFreeCell(
        handle: Long,
        itemId: String,
        preferredCol: Int,
        preferredRow: Int,
    ): IntArray

    /** One item's resolved position after [nativeResolveDrop] — see that function's return-format doc. */
    data class ResolvedPlacement(
        val itemId: String,
        val col: Int,
        val row: Int,
        val pixelX: Int,
        val pixelY: Int,
    )

    /** Decodes [nativeResolveDrop]'s semicolon/comma-encoded return string. */
    fun parseResolvedPlacements(encoded: String): List<ResolvedPlacement> {
        if (encoded.isEmpty()) return emptyList()
        return encoded.split(";").mapNotNull { entry ->
            val parts = entry.split(",")
            if (parts.size != 5) return@mapNotNull null
            ResolvedPlacement(
                itemId = parts[0],
                col = parts[1].toIntOrNull() ?: return@mapNotNull null,
                row = parts[2].toIntOrNull() ?: return@mapNotNull null,
                pixelX = parts[3].toIntOrNull() ?: return@mapNotNull null,
                pixelY = parts[4].toIntOrNull() ?: return@mapNotNull null,
            )
        }
    }

    // ── shell_client bridge (youki_shell_server TCP protocol) ────────────
    // Replaces ShellManager.kt's socket handshake/exec/ping logic.

    /** Stores [token] from the SHELL_SERVER_READY broadcast as the active session token. */
    external fun nativeSetSessionToken(token: String)

    /** Clears the active shell session (token + connected state). Call from onDestroy(). */
    external fun nativeClearShellSession()

    /** Whether a shell session token is set AND the last connection attempt succeeded. */
    external fun nativeIsShellAvailable(): Boolean

    /**
     * Executes [cmd] over the youki_shell_server socket, blocking until the
     * `__DONE__` sentinel or a 10s timeout. Call from Dispatchers.IO.
     * Returns null if no session token is set or the round trip failed.
     */
    external fun nativeShellExecSync(cmd: String): String?

    /** Sends a PING and waits for PONG over a fresh connection; updates and returns the connected state. */
    external fun nativeShellPing(): Boolean

    // ── window_geometry bridge (AppUtils.kt's pure-math window logic) ────
    //
    // Only the arithmetic/string-building parts of AppUtils.kt moved here
    // — PackageManager/LauncherApps/ActivityManager/UserManager calls and
    // the ActivityOptions reflection stay in AppUtils.kt itself (Framework
    // API with no separable logic). See window_geometry.rs's module doc.

    /**
     * Computes launch bounds for [mode] ("standard"/"maximized"/"portrait"/
     * "tiled-left"/"tiled-top"/"tiled-right"/"tiled-bottom"). Returns
     * `[left, top, right, bottom]`; unrecognized modes yield all zeros,
     * matching the original Kotlin `when` with no `else` branch.
     */
    external fun nativeComputeLaunchBounds(
        mode: String,
        deviceWidth: Int,
        deviceHeight: Int,
        statusHeight: Int,
        navHeight: Int,
        dockHeight: Int,
        applyNavbarFix: Boolean,
        scaleFactor: Float,
    ): IntArray

    /** Builds the `am start` shell command for [component] with the given bounds. Empty string if [component] is empty. */
    external fun nativeBuildLaunchCommand(
        component: String,
        mode: String,
        left: Int,
        top: Int,
        right: Int,
        bottom: Int,
        displayId: Int,
    ): String

    /** Builds the `[modeCmd, resizeCmd]` shell command pair for `AppUtils.resizeTask()`. */
    external fun nativeBuildResizeCommands(
        taskId: Int,
        left: Int,
        top: Int,
        right: Int,
        bottom: Int,
    ): Array<String>

    /**
     * Looks up the dock-size preset for [preset] ("small"/"pc"/other).
     * Returns `[dockHeightDp, iconSizeDp, gridSizeDp, useSystemDensity(0/1)]`.
     * [normalDockHeightDp] is used only when [preset] doesn't match
     * "small"/"pc" (the original's `dock_height` preference value).
     */
    external fun nativeDockSizeConfig(preset: String, normalDockHeightDp: Int): IntArray

    // ── textparse bridge: MultiUserManager.provisionUser() steps ─────────

    /**
     * Builds the ordered list of `pm grant` / `settings put` / etc. shell
     * commands for provisioning a newly-created user, plus a
     * human-readable label per step. Returns a semicolon-separated
     * `"command,label"` list — decode with [parseProvisionSteps].
     *
     * NOTE: 3 of the 14 labels ("Install app", "Install Shizuku", "Enable
     * accessibility") were originally localized string-resource lookups
     * (`context.getString(R.string.mu_provision_step_*)`) in
     * MultiUserManager.kt — the Rust side has no access to Android string
     * resources, so it returns fixed English text for those three. If
     * localizing those specific three labels still matters, replace them
     * with `context.getString(...)` after decoding, matching by their
     * position in the returned list (indices 0, 1, 10).
     */
    external fun nativeBuildProvisionSteps(pkg: String, userId: Int): String

    /** One provisioning step — see [nativeBuildProvisionSteps]'s return-format doc. */
    data class ProvisionStep(val command: String, val label: String)

    /** Decodes [nativeBuildProvisionSteps]'s semicolon/comma-encoded return string. */
    fun parseProvisionSteps(encoded: String): List<ProvisionStep> {
        if (encoded.isEmpty()) return emptyList()
        return encoded.split(";").mapNotNull { entry ->
            val parts = entry.split(",", limit = 2)
            if (parts.size != 2) return@mapNotNull null
            ProvisionStep(command = parts[0], label = parts[1])
        }
    }

    // ── root.rs bridge (remaining pieces) ─────────────────────────────────
    // nativeIsRootAvailable / nativeInvalidateRootCache / nativeExecuteRoot /
    // nativeGrantWriteSecureSettings / nativeBuildGrantAllCommands already
    // exist from earlier work. These four close the gap for RootManager.kt's
    // remaining un-bridged calls: diagnose() and the alreadyGrantedThisSession
    // guard used by requestPermission().

    /** Runs [cmd] via su and returns a formatted diagnostics report. */
    external fun nativeRootDiagnose(cmd: String): String

    /** True if root is available AND (force is set OR grant-all hasn't run yet this session). */
    external fun nativeShouldRunGrantAll(force: Boolean): Boolean

    /** Marks the root grant-all batch as completed for this process lifetime. */
    external fun nativeMarkGrantAllDone()

    /**
     * Classifies [level]/[plugged] into a battery-icon bucket
     * ("empty"/"20"/"30"/"50"/"60"/"80"/"90"/"full"). Prefix with
     * "battery_" or "battery_charging_" and resolve via R.drawable — see
     * [com.youki.dex.utils.Utils.getBatteryDrawable].
     */
    external fun nativeBatteryDrawableBucket(level: Int, plugged: Boolean): String

    // ── textparse bridge: ShizukoManager dumpsys parsing ──────────────────

    /** Extracts the foreground package name from `dumpsys activity activities | grep mResumedActivity` output. Null if no match. */
    external fun nativeParseForegroundPackage(raw: String): String?

    /** Extracts distinct running package names (first-seen order) from `dumpsys activity activities | grep 'Run #'` output. Returns a `;`-separated string. */
    external fun nativeParseRunningPackages(raw: String): String

    // ── window_geometry bridge: OnSwipeListener ───────────────────────────

    /** Classifies a swipe gesture from (x1,y1) to (x2,y2) into "UP"/"DOWN"/"LEFT"/"RIGHT". */
    external fun nativeSwipeDirection(x1: Float, y1: Float, x2: Float, y2: Float): String

    // ── textparse bridge: FontManager Arabic/font-choice logic ────────────

    /** True if [text] contains any Arabic-range Unicode character. */
    external fun nativeContainsArabic(text: String): Boolean

    /** Returns "ARABIC"/"LATIN"/"NONE" — which typeface [text] should use, given which fonts are configured. */
    external fun nativeChooseFontForText(text: String, hasArabicTf: Boolean, hasLatinTf: Boolean): String

    // ── window_geometry bridge: UserSwitcherPopup luminance ───────────────

    /** WCAG relative-luminance check on a packed 0xAARRGGBB color; true if luminance > 0.35. */
    external fun nativeIsBubbleLight(argb: Int): Boolean

    // ── window_geometry bridge: EasterEggActivity ─────────────────────────

    /** Letterbox/pillarbox target size preserving aspect ratio. Returns `[width, height]`, or null if videoW/videoH <= 0. */
    external fun nativeFitSurfaceToVideo(videoW: Int, videoH: Int, screenW: Int, screenH: Int): IntArray?

    /** Whether a fling gesture (dx, dy, velocityX) qualifies as a deliberate horizontal exit swipe. */
    external fun nativeIsExitSwipe(dx: Float, dy: Float, velocityX: Float, minDistancePx: Float, minVelocity: Float): Boolean

    // ── textparse bridge: WorkshopSourcesActivity source-form helpers ─────

    /** Prefixes [url] with "https://" if non-blank and not already starting with "http". */
    external fun nativeNormalizeSourceUrl(url: String): String

    /** Builds a `%s`-placeholder search-URL template from an already-normalized [url]. */
    external fun nativeBuildSearchUrlTemplate(url: String): String

    /** Validates the add/edit-source form fields in order (name, url, type). Returns "NAME_BLANK"/"URL_INVALID"/"TYPE_BLANK", or null if valid. */
    external fun nativeValidateSourceFields(name: String, url: String, finalType: String): String?

    // ── textparse bridge: PerfectServer CPU usage sampling ────────────────

    /**
     * Parses one `/proc/stat` line and computes the delta-based CPU busy
     * percentage. Returns `[percent, total, idlePlusIowait]`, or null if
     * [line] doesn't parse (fewer than 4 fields) — on null, keep showing
     * the previous percentage and do NOT update [prevTotal]/[prevIdle].
     */
    external fun nativeParseCpuUsage(line: String, prevTotal: Long, prevIdle: Long, lastValue: Int): LongArray?

    // ── window_geometry bridge: DockLayoutDialog presets ───────────────────

    /** Returns `[enableNav, enableQsWifiVolDateNotif, appMenuFullscreen, showNotifications, enableQsPin]` for the selected layout index. */
    external fun nativeDockLayoutPresetBools(which: Int): BooleanArray

    /** Returns `[maxRunningApps, maxRunningAppsLandscape, dockActivationArea, activationMethod]` for the selected layout index. */
    external fun nativeDockLayoutPresetStrings(which: Int): Array<String>

    // ── textparse bridge: fragments/ batch 1 ───────────────────────────────

    /** Splits a "<cols>x<rows>" grid preset string. Returns `[cols, rows]`, falling back to the defaults for a missing/non-numeric part. */
    external fun nativeParseGridPreset(value: String, defaultCols: Int, defaultRows: Int): IntArray

    /** Parses free-text resolution input ("1920x1080" or "1920×1080"). Returns `[width, height]`, or null if invalid. */
    external fun nativeParseResolutionString(text: String): IntArray?

    /** Swaps (width, height) so the smaller value is always first — the natural (portrait) size `setForcedDisplaySize` expects. Returns `[naturalWidth, naturalHeight]`. */
    external fun nativeNaturalDisplaySize(width: Int, height: Int): IntArray

    /** Classifies a file extension into an icon bucket. Ordinal: 0=Video, 1=Audio, 2=Image, 3=Pdf, 4=Archive, 5=Font, 6=Apk, 7=Text, 8=Generic. */
    external fun nativeFileIconKind(ext: String): Int

    /** Maps a file extension to a MIME type string, or "application/octet-stream" if unknown. */
    external fun nativeExtToMime(ext: String): String

    /** True if [fileExt] matches any entry in [allowedMimes] (exact match, or a "type" prefix wildcard ending in a slash-star); a bare slash-star always matches. */
    external fun nativeMimeMatches(allowedMimes: Array<String>, fileExt: String): Boolean

    /** Formats milliseconds as "m:ss" (e.g. 65000 -> "1:05"). */
    external fun nativeFormatDurationMmSs(ms: Int): String

    /** Builds the chmod script making a copied sound file readable by SystemUI (711 on each parent dir, 644 on the file). Falls back to a single `chmod 644` if [filePath] doesn't have 3+ parent directories. */
    external fun nativeBuildChmodCommands(filePath: String): String

    /**
     * Advances the version-tap Easter Egg counter given the current tap
     * time. Returns `[newCount, newFirstTapAtMs, reachedThreshold(0/1)]`.
     * Uses a fixed window from the *first* tap — a stale window restarts
     * the count at 1 rather than incrementing.
     */
    external fun nativeEasterEggTapState(nowMs: Long, count: Int, firstTapAtMs: Long, windowMs: Long, tapsRequired: Int): LongArray

    /** True if [url] matches a known ad/tracker domain (WebView ad blocker). */
    external fun nativeIsAdUrl(url: String): Boolean

    /** Guesses a file extension from a download's MIME type via substring match (looser than [nativeExtToMime]'s reverse mapping). Returns "" if unrecognized. */
    external fun nativeMimeToExt(mime: String): String

    /** Extracts and cleans a display name from an HTTP `Content-Disposition` header's `filename=` segment (underscores/dashes become spaces, extension stripped). Returns "" if there's no `filename=`. */
    external fun nativeNameFromContentDisposition(cd: String): String

    /** Formats a byte count as "B"/"KB"/"MB", matching the original `DecimalFormat` "#.#"/"#.##" rounding (trailing zeros dropped). */
    external fun nativeFormatFileSize(bytes: Long): String

    /** Sanitizes a proposed filename: strips filesystem-illegal characters, trims whitespace, caps at 80 chars, falls back to "file" if empty. Does not resolve filename collisions — caller still checks `File.exists()`. */
    external fun nativeSanitizeFilenameBase(base: String): String

    // ── textparse bridge: PerformanceFragment.kt ────────────────────────────

    /** Builds the 12-line shell script setting (or, if [reset], deleting) all refresh-rate keys the app tracks. [hz] is ignored when [reset] is true. */
    external fun nativeBuildFpsCommand(hz: Int, reset: Boolean): String

    /** Builds the thermal-profile shell script for the given mode. Ordinal: 0=Balanced, 1=Performance, 2=Gaming, 3=Powersave. */
    external fun nativeBuildThermalCommand(mode: Int): String

    /** Builds the shell script that reformats zram0 to the given size in bytes and swaps it back on. */
    external fun nativeBuildZramEnableCommand(bytes: Long): String

    /** Builds the fixed shell script that swaps off and zeroes zram0's disksize. */
    external fun nativeBuildZramDisableCommand(): String

    /** Buckets a raw zram0 disksize byte count into the same size label the picker dialog uses (e.g. "512 MB"). [offLabel] is the pre-localized string to return when [bytes] is 0. */
    external fun nativeZramBucketLabel(bytes: Long, offLabel: String): String

    /** Joins a multi-line shell script into a single `sh -c '...'` invocation (Android's cacheDir is often noexec, so scripts can't be written to a file and executed). */
    external fun nativeBuildInlineShellCommand(cmd: String): String

    // ── textparse bridge: small sections (AppAdapter/Utils.solve/plugins) ───

    /** True if [text] matches the app-drawer's inline-calculator pattern: `<number>[+-* /]<number>`, anchored on both ends. */
    external fun nativeIsCalcExpression(text: String): Boolean

    /** Evaluates a simple two-operand arithmetic expression (searches for the operator from the right, so a leading '-' isn't mistaken for the operator). Returns 0.0 for anything that doesn't parse, and for division by zero. */
    external fun nativeSolveArithmeticExpression(expression: String): Double

    /** Splits one `module.prop` (Magisk format) line already known to contain `=` into `[key, value]`, both trimmed. Returns null if there's no `=`. */
    external fun nativeParseModulePropLine(line: String): Array<String>?

    /** Sanitizes a proposed plugin id: keeps only ASCII letters/digits/underscore/hyphen, caps at 64 chars, falls back to "plugin" if empty. */
    external fun nativeSanitizePluginId(raw: String): String

    // ── textparse bridge: livewallpaper/data/VideoFileStore.kt ─────────────

    /** Strips anything but ASCII letters/digits/underscore/hyphen from a proposed video display name and appends ".mp4". */
    external fun nativeSanitizeVideoFilename(name: String): String

    // ── window_geometry bridge: services/PerfectServer.kt ───────────────────

    /** True if the raw touch point (absolute screen coords) falls within a view's on-screen bounds (edges inclusive). */
    external fun nativePointInViewBounds(rawX: Float, rawY: Float, viewScreenX: Int, viewScreenY: Int, viewWidth: Int, viewHeight: Int): Boolean
}
