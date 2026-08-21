//! TransformMatrix — direct port of the Kotlin TransformMatrix.kt.
//!
//! IMPORTANT: this is a deliberately literal 1:1 port, not a "cleaned up"
//! rewrite. The original file encodes several manually-derived and
//! hand-tested fixes (see comments below, carried over verbatim in spirit)
//! for real bugs: shear-free rotation on a non-square NDC space, and a
//! touch-space vs NDC-space Y-axis inversion for rotation direction. Do not
//! "simplify" the FREE-mode branch without re-testing at 0°/90°/180°/270°
//! against the original Kotlin behavior side by side.

use crate::renderer::{FrameConfig, ScaleMode};

/// Per-orientation free-transform values, mirroring WallpaperConfig's
/// freeTransformFor(orientation) — Kotlin still owns reading/writing these
/// from Room; this struct is just what crosses the JNI boundary for a given
/// draw call already resolved to the current orientation.
#[derive(Clone, Copy, Debug, Default)]
pub struct FreeTransform {
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_deg: f32,
    pub translate_x: f32,
    pub translate_y: f32,
    pub flip_horizontal: bool,
    pub flip_vertical: bool,
}

pub struct TransformMatrix;

impl TransformMatrix {
    /// Builds the final MVP matrix (column-major, matching android.opengl.Matrix's
    /// layout so the existing GLSL vertex shader's `uMVP` uniform upload is
    /// unchanged — see gl_backend.rs's shader source).
    ///
    /// video_width/video_height: the video's original dimensions (pixels).
    /// screen_width/screen_height: current screen dimensions (pixels), updated
    /// on rotation so there is never any forced stretching.
    pub fn build(
        config: &FrameConfig,
        free: FreeTransform,
        video_width: i32,
        video_height: i32,
        screen_width: i32,
        screen_height: i32,
    ) -> [f32; 16] {
        let mut mvp = identity();

        if video_width <= 0 || video_height <= 0 || screen_width <= 0 || screen_height <= 0 {
            // Avoid dividing by zero before we know the video's dimensions.
            return mvp;
        }

        let video_aspect = video_width as f32 / video_height as f32;
        let screen_aspect = screen_width as f32 / screen_height as f32;

        // ── Step 1: base aspect ratio scaling depending on the mode ────────
        let mut scale_x = 1f32;
        let mut scale_y = 1f32;

        match config.scale_mode {
            ScaleMode::Stretch => {
                // No correction at all — video fills [-1,1]x[-1,1] entirely as-is.
            }
            ScaleMode::Fit => {
                // The smaller dimension determines the scaling -> full video
                // shows + empty space on the edges.
                if video_aspect > screen_aspect {
                    scale_y = screen_aspect / video_aspect;
                } else {
                    scale_x = video_aspect / screen_aspect;
                }
            }
            ScaleMode::Cover => {
                // The larger dimension determines the scaling -> fully
                // covers the screen + crops the excess.
                if video_aspect > screen_aspect {
                    scale_x = video_aspect / screen_aspect;
                } else {
                    scale_y = screen_aspect / video_aspect;
                }
            }
            ScaleMode::Free => {
                // Same base logic as Cover, then the user's free values are
                // multiplied on top exactly once, below.
                if video_aspect > screen_aspect {
                    scale_x = video_aspect / screen_aspect;
                } else {
                    scale_y = screen_aspect / video_aspect;
                }
            }
        }

        // ── Step 2: horizontal/vertical flip (mainly a Free Mode feature) ──
        let flip_x = if free.flip_horizontal { -1f32 } else { 1f32 };
        let flip_y = if free.flip_vertical { -1f32 } else { 1f32 };

        // ── Step 3: compose the matrix in the correct order ────────────────
        // A manually-built 4x4 matrix for a purely 2D operation
        // (Scale -> Rotate -> Translate), with no chained multiply calls
        // that could reintroduce multiplication-order ambiguity. This
        // guarantees rigid rotation at any angle, with no shear.
        if config.scale_mode == ScaleMode::Free {
            // scale_x/scale_y from the COVER-equivalent base above are the
            // aspect correction only; free.scale_x/y is multiplied in
            // exactly once here.
            let sx = scale_x * free.scale_x * flip_x;
            let sy = scale_y * free.scale_y * flip_y;

            // FIX (inverted response, carried over from Kotlin):
            // free.rotation_deg is computed in screen touch coordinates,
            // where +Y = downward (see the equivalent of
            // FreeGestureOverlay.currentAngle() — positive angle = clockwise
            // as the user visually sees it). This space here is OpenGL NDC,
            // where +Y = upward — the two spaces are vertically mirrored.
            // Without inverting the angle itself, a "clockwise" finger
            // rotation would render as counter-clockwise on screen, and this
            // exact inversion is also what prevents stutter/oscillation near
            // 90°/270° (the user fighting the direction with their finger).
            // Inverting the angle's sign here — and only here — fully
            // corrects the reference for any angle, without touching
            // translate_x/y (already handled correctly upstream, mirroring
            // EditorActivity's dy inversion for translateY).
            let rad = (-free.rotation_deg as f64).to_radians();
            let cos_a = rad.cos() as f32;
            let sin_a = rad.sin() as f32;
            let tx = free.translate_x * 2f32;
            let ty = free.translate_y * 2f32; // no inversion here — handled upstream

            // Key correction: NDC space [-1,1]x[-1,1] isn't visually square
            // (the real screen is rectangular, ratio = screen_aspect =
            // width/height). A naive rotation (plain sin/cos) in this space
            // produces shear, not a rigid rotation. Correct fix: convert Y
            // into "X units" before rotating (y_eq = y * screen_aspect),
            // rotate normally, then convert back (divide by screen_aspect).
            // Algebraically expanded, this simplifies to multiplying every
            // term containing sin_a by the aspect ratio exactly once, in the
            // direction below. Verified manually against 0°/90°/180°.
            let aspect = screen_aspect;

            let m = &mut mvp;
            m[0] = cos_a * sx;
            m[1] = sin_a * aspect * sx;
            m[2] = 0f32;
            m[3] = 0f32;
            m[4] = -sin_a / aspect * sy;
            m[5] = cos_a * sy;
            m[6] = 0f32;
            m[7] = 0f32;
            m[8] = 0f32;
            m[9] = 0f32;
            m[10] = 1f32;
            m[11] = 0f32;
            m[12] = tx;
            m[13] = ty;
            m[14] = 0f32;
            m[15] = 1f32;
        } else {
            scale_m(&mut mvp, scale_x * flip_x, scale_y * flip_y, 1f32);
        }

        mvp
    }

    /// A simple identity matrix — used for the background quad (background
    /// color) that always covers the entire screen.
    pub fn identity_matrix() -> [f32; 16] {
        identity()
    }
}

fn identity() -> [f32; 16] {
    let mut m = [0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

/// Equivalent of android.opengl.Matrix.scaleM(m, 0, x, y, z) — scales
/// in-place, column-major layout.
fn scale_m(m: &mut [f32; 16], x: f32, y: f32, z: f32) {
    m[0] *= x;
    m[1] *= x;
    m[2] *= x;
    m[3] *= x;
    m[4] *= y;
    m[5] *= y;
    m[6] *= y;
    m[7] *= y;
    m[8] *= z;
    m[9] *= z;
    m[10] *= z;
    m[11] *= z;
}
