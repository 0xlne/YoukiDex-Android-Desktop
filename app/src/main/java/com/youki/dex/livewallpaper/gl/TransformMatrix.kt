package com.youki.dex.livewallpaper.gl

import android.opengl.Matrix
import com.youki.dex.livewallpaper.data.WallpaperConfig

/**
 * TransformMatrix — converts [WallpaperConfig] into an MVP matrix ready for the vertex shader.
 *
 * ┌────────────────────────────────────────────────────────────────────────┐
 * │  Concept: we draw a fixed quad of size [-1, 1] × [-1, 1] (the full     │
 * │  screen area), then "distort" it with an MVP matrix to achieve:        │
 * │                                                                        │
 * │    FIT     → full original aspect ratio + empty space on the edges    │
 * │    COVER   → scaled up to fully cover the screen + crop the excess    │
 * │    STRETCH → no aspect correction at all — direct stretch (= identity scale) │
 * │    FREE    → the user controls every value manually (scale/translate/rotate) │
 * └────────────────────────────────────────────────────────────────────────┘
 */
object TransformMatrix {

    /**
     * Builds the final MVP matrix.
     *
     * @param videoWidth/videoHeight the video's original dimensions (pixels)
     * @param screenWidth/screenHeight the current screen dimensions (pixels) — updated
     *        automatically on device rotation, so there's never any forced stretching.
     */
    fun build(
        config: WallpaperConfig,
        videoWidth: Int, videoHeight: Int,
        screenWidth: Int, screenHeight: Int
    ): FloatArray {
        val mvp = FloatArray(16)
        Matrix.setIdentityM(mvp, 0)

        if (videoWidth <= 0 || videoHeight <= 0 || screenWidth <= 0 || screenHeight <= 0) {
            return mvp // avoid dividing by zero before we know the video's dimensions
        }

        val videoAspect  = videoWidth.toFloat() / videoHeight.toFloat()
        val screenAspect = screenWidth.toFloat() / screenHeight.toFloat()

        // ── FIX: which orientation do we read from? ──────────────────────────────────────────
        // Each screen orientation (portrait/landscape) has its own fully independent
        // Free Mode values — this guarantees that a user's adjustment in portrait
        // (say, moving a character to center the screen) never leaks into landscape
        // and vice versa, exactly as requested. The orientation is determined from
        // the actual screen dimensions at draw time (more accurate than any other
        // system setting, and works the same way in both the Editor and the real
        // service).
        val orientation = WallpaperConfig.orientationFor(screenWidth, screenHeight)
        val free = config.freeTransformFor(orientation)

        // ── Step 1: base aspect ratio scaling depending on the mode ───────────────────
        var scaleX = 1f
        var scaleY = 1f

        when (WallpaperConfig.ScaleMode.valueOf(config.scaleMode)) {
            WallpaperConfig.ScaleMode.STRETCH -> {
                // No correction at all — the video fills [-1,1]×[-1,1] entirely as-is
            }

            WallpaperConfig.ScaleMode.FIT -> {
                // The smaller dimension determines the scaling → the full video shows + empty space
                if (videoAspect > screenAspect) {
                    scaleY = screenAspect / videoAspect
                } else {
                    scaleX = videoAspect / screenAspect
                }
            }

            WallpaperConfig.ScaleMode.COVER -> {
                // The larger dimension determines the scaling → fully covers the screen + crops
                if (videoAspect > screenAspect) {
                    scaleX = videoAspect / screenAspect
                } else {
                    scaleY = screenAspect / videoAspect
                }
            }

            WallpaperConfig.ScaleMode.FREE -> {
                // Same logic as COVER as a base, then multiply by the user's free values on top
                if (videoAspect > screenAspect) {
                    scaleX = videoAspect / screenAspect
                } else {
                    scaleY = screenAspect / videoAspect
                }
                // FIX: scaleX/scaleY here are just the base (aspect correction)
                // free.scaleX/Y is applied exactly once further below when building the matrix
            }
        }

        // ── Step 2: horizontal/vertical flip (mainly a Free Mode feature) ──────────────
        val flipX = if (free.flipHorizontal) -1f else 1f
        val flipY = if (free.flipVertical)   -1f else 1f

        // ── Step 3: compose the matrix in the correct order ────────────────────────────
        // Full rewrite (after two earlier attempts using a chain of consecutive
        // Matrix.*M calls, which were prone to ambiguity in android.opengl.Matrix's
        // multiplication order): here we build a 4×4 matrix manually for a purely 2D
        // operation (Scale → Rotate → Translate), with no chained calls that could
        // confuse the order. This mathematically guarantees 100% that the sheet
        // rotates completely rigidly at any angle, with no shear, exactly like any
        // photo editor (Photoshop / Wallpaper Engine): rotation around a fixed
        // center + per-axis scale + translation, all computed in real screen space
        // already corrected for aspect ratio — there's no third dimension (Z) and
        // no perspective at any step.
        if (config.scaleMode == WallpaperConfig.ScaleMode.FREE.name) {
            // scaleX/scaleY from COVER are the base (aspect correction only), free.scaleX/Y is multiplied in exactly once here
            val sx = scaleX * free.scaleX * flipX
            val sy = scaleY * free.scaleY * flipY
            // Resolved issue: inverted response. free.rotationDeg is computed in the screen's
            // touch coordinates, where +Y = downward (see currentAngle() in
            // FreeGestureOverlay, where a positive angle = clockwise as the user
            // actually sees it with their own eyes). But this space here is OpenGL
            // NDC, where +Y = upward — the two spaces are vertically mirrored from
            // each other. Without inverting the angle itself, any "clockwise"
            // rotation with the user's finger used to translate visually into a
            // "counter-clockwise" rotation on the actual screen, and this exact
            // inversion is also what caused the feeling that the rotation "doesn't
            // complete smoothly" or "stutters" near 90°/270° — because the user was
            // trying to correct the direction with their finger, which generated
            // oscillation between two opposing rotations stacked on top of each
            // other. Inverting the angle's sign here (and only here) fully corrects
            // the reference for any angle, including the full 360° rotation,
            // without touching the translateX/Y coordinates (those are already
            // handled correctly in EditorActivity).
            val rad = Math.toRadians(-free.rotationDeg.toDouble())
            val cosA = Math.cos(rad).toFloat()
            val sinA = Math.sin(rad).toFloat()
            val tx = free.translateX * 2f
            // FIX 2: ty with no inversion — the inversion happens correctly in EditorActivity (dy is inverted there)
            val ty = free.translateY * 2f

            // Key correction: the NDC space here [-1,1]×[-1,1] isn't visually square
            // (the real screen is rectangular, with ratio screenAspect =
            // width/height). A normal rotation (standard sin/cos) in this space
            // produces Shear, not a rigid rotation. The correct mathematical fix:
            // convert Y into "X units" before rotating (y_eq = y * screenAspect),
            // rotate normally, then convert it back (divide by screenAspect) — when
            // this transform is algebraically expanded, it simplifies to
            // multiplying every term containing sinA by the aspect ratio exactly
            // once, in the correct direction, as below. This derivation was tested
            // manually against the 0°/90°/180° cases to confirm it gives a truly
            // rigid rotation with no shear at any angle.
            val aspect = screenAspect

            val m = mvp
            m[0]  = cosA * sx;               m[1]  = sinA * aspect * sx;        m[2]  = 0f; m[3]  = 0f
            m[4]  = -sinA / aspect * sy;      m[5]  = cosA * sy;                  m[6]  = 0f; m[7]  = 0f
            m[8]  = 0f;                       m[9]  = 0f;                          m[10] = 1f; m[11] = 0f
            m[12] = tx;                       m[13] = ty;                          m[14] = 0f; m[15] = 1f
        } else {
            Matrix.scaleM(mvp, 0, scaleX * flipX, scaleY * flipY, 1f)
        }

        return mvp
    }

    /** A simple identity matrix — used for the background quad (background color) that always covers the entire screen */
    fun identity(): FloatArray {
        val m = FloatArray(16)
        Matrix.setIdentityM(m, 0)
        return m
    }
}
