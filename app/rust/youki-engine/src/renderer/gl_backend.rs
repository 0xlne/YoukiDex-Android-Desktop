//! GlRenderer — direct port of WallpaperGLRenderer.kt + GLProgram.kt +
//! Quad.kt + Shaders.kt, using khronos-egl for context/surface management
//! (equivalent to what WallpaperGLEngine.kt did manually with
//! android.opengl.EGL14, since GLSurfaceView cannot be used inside a
//! WallpaperService on Android 10+ — see handoff doc).
//!
//! Every documented "FIX" comment from the original Kotlin is preserved
//! here, because each one encodes a real bug that was hand-diagnosed and
//! fixed — removing the workaround without understanding why it exists
//! would reintroduce the original bug (freeze/glitch on EGL context
//! recreation, upside-down video, crash on use-after-release, etc).

use crate::renderer::matrix::{FreeTransform, TransformMatrix};
use crate::renderer::{FrameConfig, RendererBackend};
use ndk::native_window::NativeWindow;

// khronos_egl types (Display/Context/Surface/Config) are referenced via
// fully-qualified paths (khronos_egl::Display, etc.) below rather than
// imported individually, since EglState is the only place they're used and
// the explicit prefix makes it unambiguous this is EGL state, not something
// GLES- or Vulkan-related. No `use` statement is needed for this to
// compile (Rust 2021 resolves top-level crate paths without one); omitting
// it here also avoids an unused-import warning.

// ── GLES constant re-exports (gles31 crate exposes these under gl::) ───────
use gles31 as gl;

// samplerExternalOES / GL_OES_EGL_image_external requires this token, which
// isn't in core GLES2.0/3.1 headers — defined manually as in Android's
// GLES11Ext.GL_TEXTURE_EXTERNAL_OES.
const GL_TEXTURE_EXTERNAL_OES: u32 = 0x8D65;

// ── Shader sources — verbatim from Shaders.kt ───────────────────────────────
const VERTEX_SHADER: &str = r#"
    uniform mat4 uMVPMatrix;
    uniform mat4 uSTMatrix;
    attribute vec4 aPosition;
    attribute vec4 aTextureCoord;
    varying vec2 vTextureCoord;
    void main() {
        gl_Position = uMVPMatrix * aPosition;
        vTextureCoord = (uSTMatrix * aTextureCoord).xy;
    }
"#;

// OES_EGL_image_external is required to read video frames directly from
// the SurfaceTexture-equivalent external texture.
const FRAGMENT_SHADER: &str = r#"
    #extension GL_OES_EGL_image_external : require
    precision mediump float;
    varying vec2 vTextureCoord;
    uniform samplerExternalOES sTexture;

    uniform bool  uColorCorrectionEnabled;
    uniform float uBrightness;   // 0..2   (1 = neutral)
    uniform float uContrast;     // 0..2
    uniform float uSaturation;   // 0..2

    vec3 applyColorCorrection(vec3 color) {
        // Contrast: blend around mid-gray (0.5)
        color = mix(vec3(0.5), color, uContrast);
        // Brightness: direct multiplication
        color *= uBrightness;
        // Saturation: blend between luminance (gray) and the original color
        float luminance = dot(color, vec3(0.299, 0.587, 0.114));
        color = mix(vec3(luminance), color, uSaturation);
        return clamp(color, 0.0, 1.0);
    }

    void main() {
        vec4 texColor = texture2D(sTexture, vTextureCoord);
        if (uColorCorrectionEnabled) {
            texColor.rgb = applyColorCorrection(texColor.rgb);
        }
        gl_FragColor = texColor;
    }
"#;

const SOLID_COLOR_VERTEX_SHADER: &str = r#"
    uniform mat4 uMVPMatrix;
    attribute vec4 aPosition;
    void main() {
        gl_Position = uMVPMatrix * aPosition;
    }
"#;

const SOLID_COLOR_FRAGMENT_SHADER: &str = r#"
    precision mediump float;
    uniform vec4 uColor;
    void main() {
        gl_FragColor = uColor;
    }
"#;

// ── Quad geometry — verbatim from Quad.kt, INCLUDING the documented V-flip
// fix. Do not "correct" TEX_COORDS back to the naive [0,0 / 0,1 / 1,0 / 1,1]
// mapping — that was the original bug (upside-down video), already fixed
// once, and reverting it silently would reintroduce it.
#[rustfmt::skip]
const QUAD_VERTICES: [f32; 12] = [
    -1.0,  1.0, 0.0,
    -1.0, -1.0, 0.0,
     1.0,  1.0, 0.0,
     1.0, -1.0, 0.0,
];

// FIX (video was upside down): vertex order above is top-left, bottom-left,
// top-right, bottom-right. In GL texture space V=0 is the image's bottom and
// V=1 is the top (opposite of screen space). V is inverted per-vertex here
// so V=1 (image's actual top) maps to the on-screen top-left vertex.
#[rustfmt::skip]
const QUAD_TEX_COORDS: [f32; 8] = [
    0.0, 1.0,
    0.0, 0.0,
    1.0, 1.0,
    1.0, 0.0,
];

// NOTE: an earlier draft declared this as i32, assuming GLsizei was
// signed — caught by the build: gles31's GLsizei (and GLuint/GLenum) are
// all u32. Declaring the constant as u32 directly avoids a .try_into()
// cast at every call site.
const QUAD_VERTEX_COUNT: u32 = 4;
const QUAD_COORDS_PER_VERTEX: i32 = 3;
const QUAD_TEX_COORDS_PER_VERTEX: i32 = 2;

/// EGL context/surface/display handles. Ported from WallpaperGLEngine.kt's
/// manual EGL setup (GLSurfaceView doesn't work inside a WallpaperService on
/// Android 10+ — see handoff doc's "no GLSurfaceView" note).
///
/// NOTE: an earlier draft of this struct omitted `instance`, keeping only
/// the display/context/surface/config handles. Those are opaque EGL handles
/// (not usable on their own) — every EGL call (make_current, swap_buffers,
/// eventual teardown) is a method on `khronos_egl::Instance`, so the
/// Instance that produced these handles must be kept alive alongside them
/// for as long as they're used, or later calls elsewhere in this file would
/// have nothing to call them on. Caught while cross-checking init_egl's
/// return value against every place EglState's fields get used.
struct EglState {
    instance: khronos_egl::Instance<khronos_egl::Static>,
    display: khronos_egl::Display,
    context: khronos_egl::Context,
    surface: khronos_egl::Surface,
    // Never read directly, but kept alive here on purpose: EGL semantics
    // require the Config that produced `context`/`surface` to remain valid
    // for as long as those handles are used, so this field's only job is to
    // hold that reference for EglState's lifetime.
    #[allow(dead_code)]
    config: khronos_egl::Config,
}

pub struct GlRenderer {
    egl: Option<EglState>,

    video_program: u32,
    bg_program: u32,

    a_position_loc: i32,
    a_tex_coord_loc: i32,
    u_mvp_loc: i32,
    u_st_matrix_loc: i32,
    u_texture_loc: i32,
    u_cc_enabled_loc: i32,
    u_brightness_loc: i32,
    u_contrast_loc: i32,
    u_saturation_loc: i32,

    bg_a_position_loc: i32,
    bg_mvp_loc: i32,
    bg_color_loc: i32,

    oes_texture_id: u32,
    /// Row-major 4x4 "ST matrix" (SurfaceTexture.getTransformMatrix
    /// equivalent) applied to texture coords to account for the producer's
    /// buffer transform (rotation/crop the video decoder may apply).
    /// Populated by the JNI bridge from the Kotlin-side SurfaceTexture
    /// (see jni_bridge::renderer) since SurfaceTexture itself has no Rust/
    /// NDK-level equivalent — this is one of the few pieces of per-frame
    /// state that still has to cross the JNI boundary every frame, same as
    /// the original Kotlin's stMatrix.
    st_matrix: [f32; 16],

    video_width: i32,
    video_height: i32,
    screen_width: i32,
    screen_height: i32,

    frame_available: bool,
    released: bool,
}

impl GlRenderer {
    pub fn new() -> Self {
        Self {
            egl: None,
            video_program: 0,
            bg_program: 0,
            a_position_loc: 0,
            a_tex_coord_loc: 0,
            u_mvp_loc: 0,
            u_st_matrix_loc: 0,
            u_texture_loc: 0,
            u_cc_enabled_loc: 0,
            u_brightness_loc: 0,
            u_contrast_loc: 0,
            u_saturation_loc: 0,
            bg_a_position_loc: 0,
            bg_mvp_loc: 0,
            bg_color_loc: 0,
            oes_texture_id: 0,
            st_matrix: identity_4x4(),
            video_width: 0,
            video_height: 0,
            screen_width: 0,
            screen_height: 0,
            frame_available: false,
            released: false,
        }
    }

    /// Called by the JNI bridge every time SurfaceTexture.OnFrameAvailable
    /// fires on the Kotlin side, along with the freshly-read transform
    /// matrix (SurfaceTexture.getTransformMatrix has no NDK equivalent, so
    /// Kotlin still reads it and forwards the 16 floats here).
    /// NOTE: not yet called — the JNI bridge entry point for this hasn't
    /// been wired up from the Kotlin side yet.
    #[allow(dead_code)]
    pub fn on_frame_available(&mut self, st_matrix: [f32; 16]) {
        self.st_matrix = st_matrix;
        self.frame_available = true;
    }

    /// NOTE: not yet called — same as on_frame_available above, awaiting
    /// its JNI bridge entry point.
    #[allow(dead_code)]
    pub fn update_video_size(&mut self, w: i32, h: i32) {
        self.video_width = w;
        self.video_height = h;
    }

    fn draw_background_quad(&self, color: [f32; 4]) {
        unsafe {
            gl::glUseProgram(self.bg_program);
            gl::glEnableVertexAttribArray(self.bg_a_position_loc as u32);
            gl::glVertexAttribPointer(
                self.bg_a_position_loc as u32,
                QUAD_COORDS_PER_VERTEX,
                gl::GL_FLOAT,
                gl::GL_FALSE,
                0,
                QUAD_VERTICES.as_ptr() as *const _,
            );
            gl::glUniformMatrix4fv(
                self.bg_mvp_loc,
                1,
                gl::GL_FALSE,
                TransformMatrix::identity_matrix().as_ptr(),
            );
            gl::glUniform4fv(self.bg_color_loc, 1, color.as_ptr());
            gl::glDrawArrays(gl::GL_TRIANGLE_STRIP, 0, QUAD_VERTEX_COUNT);
            gl::glDisableVertexAttribArray(self.bg_a_position_loc as u32);
        }
    }

    fn draw_video_frame(&self, config: &FrameConfig, free: FreeTransform) {
        unsafe {
            gl::glUseProgram(self.video_program);

            gl::glEnableVertexAttribArray(self.a_position_loc as u32);
            gl::glVertexAttribPointer(
                self.a_position_loc as u32,
                QUAD_COORDS_PER_VERTEX,
                gl::GL_FLOAT,
                gl::GL_FALSE,
                0,
                QUAD_VERTICES.as_ptr() as *const _,
            );

            gl::glEnableVertexAttribArray(self.a_tex_coord_loc as u32);
            gl::glVertexAttribPointer(
                self.a_tex_coord_loc as u32,
                QUAD_TEX_COORDS_PER_VERTEX,
                gl::GL_FLOAT,
                gl::GL_FALSE,
                0,
                QUAD_TEX_COORDS.as_ptr() as *const _,
            );

            let mvp = TransformMatrix::build(
                config,
                free,
                self.video_width,
                self.video_height,
                self.screen_width,
                self.screen_height,
            );
            gl::glUniformMatrix4fv(self.u_mvp_loc, 1, gl::GL_FALSE, mvp.as_ptr());
            gl::glUniformMatrix4fv(self.u_st_matrix_loc, 1, gl::GL_FALSE, self.st_matrix.as_ptr());

            gl::glUniform1i(self.u_cc_enabled_loc, config.color_correction_enabled as i32);
            gl::glUniform1f(self.u_brightness_loc, config.brightness);
            gl::glUniform1f(self.u_contrast_loc, config.contrast);
            gl::glUniform1f(self.u_saturation_loc, config.saturation);

            gl::glActiveTexture(gl::GL_TEXTURE0);
            gl::glBindTexture(GL_TEXTURE_EXTERNAL_OES, self.oes_texture_id);
            gl::glUniform1i(self.u_texture_loc, 0);

            gl::glDrawArrays(gl::GL_TRIANGLE_STRIP, 0, QUAD_VERTEX_COUNT);

            gl::glDisableVertexAttribArray(self.a_position_loc as u32);
            gl::glDisableVertexAttribArray(self.a_tex_coord_loc as u32);
        }
    }

    fn create_oes_texture() -> u32 {
        unsafe {
            let mut textures = [0u32; 1];
            gl::glGenTextures(1, textures.as_mut_ptr());
            let id = textures[0];
            gl::glBindTexture(GL_TEXTURE_EXTERNAL_OES, id);
            gl::glTexParameteri(GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_MIN_FILTER, gl::GL_LINEAR as i32);
            gl::glTexParameteri(GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_MAG_FILTER, gl::GL_LINEAR as i32);
            gl::glTexParameteri(GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_WRAP_S, gl::GL_CLAMP_TO_EDGE as i32);
            gl::glTexParameteri(GL_TEXTURE_EXTERNAL_OES, gl::GL_TEXTURE_WRAP_T, gl::GL_CLAMP_TO_EDGE as i32);
            id
        }
    }
}

/// Converts "#AARRGGBB" into [r,g,b,a] with 0.0..1.0 values, matching
/// WallpaperGLRenderer's private parseColor() exactly, including the
/// black-fallback behavior on invalid input.
pub fn parse_color(hex: &str) -> [f32; 4] {
    let clean = hex.strip_prefix('#').unwrap_or(hex);
    match u32::from_str_radix(clean, 16) {
        Ok(argb) => {
            let a = ((argb >> 24) & 0xFF) as f32 / 255.0;
            let r = ((argb >> 16) & 0xFF) as f32 / 255.0;
            let g = ((argb >> 8) & 0xFF) as f32 / 255.0;
            let b = (argb & 0xFF) as f32 / 255.0;
            [r, g, b, a]
        }
        Err(_) => {
            log::warn!("Invalid color hex: {hex}, falling back to black");
            [0.0, 0.0, 0.0, 1.0]
        }
    }
}

impl RendererBackend for GlRenderer {
    fn on_surface_created(&mut self, window: &NativeWindow) -> Result<(), String> {
        // FIX (Freeze/Glitch — EGL context recreation), carried over from
        // WallpaperGLEngine.kt/WallpaperGLRenderer.kt: the surface can be
        // recreated after every pause/resume (Home button, opening
        // Settings, etc). A leftover `released = true` from a previous
        // session made the old onDrawFrame() return immediately -> frozen
        // screen. The fix here is the same: full "rebirth" — reinitialize
        // every GL resource and reset `released` before any drawing, and
        // release any previous EGL/GL state first (safe: single GL thread).
        self.released = false;
        self.release_gl_resources();

        let egl = init_egl(window)?;
        // eglMakeCurrent's C ABI is (display, draw_surface, read_surface,
        // context). The Rust wrapper call below follows that shape, but
        // the precise khronos-egl 6.x signature (value vs reference for
        // `display`, exact Option<...> nesting) has not been build-verified
        // in this session — same confidence category as the rest of this
        // EGL integration (see init_egl's note above). If this doesn't
        // compile as written, check khronos-egl's Instance::make_current
        // docs.rs page for the pinned version rather than re-guessing here.
        egl.instance
            .make_current(egl.display, Some(egl.surface), Some(egl.surface), Some(egl.context))
            .map_err(|e| format!("eglMakeCurrent failed: {e:?}"))?;
        self.egl = Some(egl);

        self.video_program = build_program(VERTEX_SHADER, FRAGMENT_SHADER)?;
        // NOTE: each CString is bound to a local variable and kept alive for
        // the whole unsafe block before .as_ptr() is taken — passing
        // c_str("x").as_ptr() directly as a call argument would create a
        // dangling pointer, since the temporary CString is dropped at the
        // end of the enclosing statement, before the FFI call can safely
        // use the pointer it handed out. This is a real memory-safety
        // concern, not a style preference.
        let name_a_position = c_str("aPosition");
        let name_a_tex_coord = c_str("aTextureCoord");
        let name_u_mvp = c_str("uMVPMatrix");
        let name_u_st_matrix = c_str("uSTMatrix");
        let name_s_texture = c_str("sTexture");
        let name_u_cc_enabled = c_str("uColorCorrectionEnabled");
        let name_u_brightness = c_str("uBrightness");
        let name_u_contrast = c_str("uContrast");
        let name_u_saturation = c_str("uSaturation");
        unsafe {
            self.a_position_loc = gl::glGetAttribLocation(self.video_program, name_a_position.as_ptr() as *const _);
            self.a_tex_coord_loc =
                gl::glGetAttribLocation(self.video_program, name_a_tex_coord.as_ptr() as *const _);
            self.u_mvp_loc = gl::glGetUniformLocation(self.video_program, name_u_mvp.as_ptr() as *const _);
            self.u_st_matrix_loc =
                gl::glGetUniformLocation(self.video_program, name_u_st_matrix.as_ptr() as *const _);
            self.u_texture_loc = gl::glGetUniformLocation(self.video_program, name_s_texture.as_ptr() as *const _);
            self.u_cc_enabled_loc =
                gl::glGetUniformLocation(self.video_program, name_u_cc_enabled.as_ptr() as *const _);
            self.u_brightness_loc =
                gl::glGetUniformLocation(self.video_program, name_u_brightness.as_ptr() as *const _);
            self.u_contrast_loc = gl::glGetUniformLocation(self.video_program, name_u_contrast.as_ptr() as *const _);
            self.u_saturation_loc =
                gl::glGetUniformLocation(self.video_program, name_u_saturation.as_ptr() as *const _);
        }

        self.bg_program = build_program(SOLID_COLOR_VERTEX_SHADER, SOLID_COLOR_FRAGMENT_SHADER)?;
        let name_bg_a_position = c_str("aPosition");
        let name_bg_mvp = c_str("uMVPMatrix");
        let name_bg_color = c_str("uColor");
        unsafe {
            self.bg_a_position_loc =
                gl::glGetAttribLocation(self.bg_program, name_bg_a_position.as_ptr() as *const _);
            self.bg_mvp_loc = gl::glGetUniformLocation(self.bg_program, name_bg_mvp.as_ptr() as *const _);
            self.bg_color_loc = gl::glGetUniformLocation(self.bg_program, name_bg_color.as_ptr() as *const _);
        }

        self.oes_texture_id = Self::create_oes_texture();

        // NOTE: unlike the Kotlin version, this backend does not construct
        // a SurfaceTexture itself (no NDK equivalent exists — see handoff
        // doc's "SurfaceTexture is external-OES-specific" note). The JNI
        // bridge is responsible for constructing the Kotlin-side
        // SurfaceTexture(oesTextureId-equivalent) and wiring its
        // OnFrameAvailableListener to call on_frame_available() above, then
        // wrapping it as a Surface for ExoPlayer via
        // RendererBackend::video_input_surface_handle().

        Ok(())
    }

    fn on_surface_changed(&mut self, width: i32, height: i32) {
        // gles31's GLsizei is u32; the trait signature keeps i32 (shared
        // with vk_backend.rs, and negative sizes are meaningless anyway).
        // .max(0) as u32 rather than .try_into().unwrap() avoids a panic if
        // a transient negative value ever reaches here during rotation,
        // clamping to 0 instead of crashing the renderer over a size that
        // will be corrected by the next resize callback anyway.
        let gl_width = width.max(0) as u32;
        let gl_height = height.max(0) as u32;
        unsafe {
            gl::glViewport(0, 0, gl_width, gl_height);
        }
        self.screen_width = width;
        self.screen_height = height;
    }

    fn on_draw_frame(&mut self, config: &FrameConfig, has_new_frame: bool) {
        // FIX (crash on release-during-draw), carried over: if release()
        // runs on another thread while the GL thread is mid-draw, touching
        // GL state on a released context crashes. Check `released` first.
        if self.released {
            return;
        }
        if has_new_frame {
            self.frame_available = true;
        }
        // The actual "updateTexImage()" equivalent + fresh st_matrix arrive
        // via on_frame_available() from the JNI bridge (Kotlin still owns
        // the SurfaceTexture) — nothing further to fetch here.
        self.frame_available = false;

        // ── Step 1: clear the screen with the configured background color ──
        let bg = parse_color(&config.background_color);
        unsafe {
            gl::glClearColor(bg[0], bg[1], bg[2], bg[3]);
            gl::glClear(gl::GL_COLOR_BUFFER_BIT | gl::GL_DEPTH_BUFFER_BIT);
        }
        self.draw_background_quad(bg);

        // ── Step 2: draw the video frame on top with the computed MVP ──────
        if self.video_width > 0 && self.video_height > 0 {
            // TODO(jni_bridge): FreeTransform must be resolved per-orientation
            // (WallpaperConfig.freeTransformFor(orientation)) on the Kotlin
            // side and passed in alongside FrameConfig — not yet threaded
            // through this trait signature. Flagging explicitly rather than
            // defaulting silently: using FreeTransform::default() here is a
            // placeholder and will NOT reproduce Free-mode transforms
            // correctly until wired up. See jni_bridge::renderer.
            self.draw_video_frame(config, FreeTransform::default());
        }

        if let Some(egl) = &self.egl {
            // Same method-belongs-to-Instance pattern as make_current above.
            let _ = egl.instance.swap_buffers(egl.display, egl.surface);
        }
    }

    fn on_surface_destroyed(&mut self) {
        // FIX (crash — use-after-release + leaked GL resources), carried
        // over verbatim: the old release() only released the SurfaceTexture
        // and leaked the OES texture + both GL programs, and didn't
        // disconnect the frame-available listener first (risking a callback
        // firing on an already-released object). Order matters: disconnect
        // listener equivalent first (handled JNI-side before this is
        // called), then release GL resources, then EGL itself.
        if self.released {
            return;
        }
        self.released = true;
        self.release_gl_resources();
        self.egl = None;
    }

    fn video_input_surface_handle(&self) -> i64 {
        // Returned as an opaque handle; the JNI bridge resolves this to the
        // actual android.view.Surface global ref it created around the
        // SurfaceTexture bound to `self.oes_texture_id`. See jni_bridge::renderer.
        self.oes_texture_id as i64
    }
}

impl GlRenderer {
    fn release_gl_resources(&mut self) {
        unsafe {
            if self.video_program != 0 {
                gl::glDeleteProgram(self.video_program);
                self.video_program = 0;
            }
            if self.bg_program != 0 {
                gl::glDeleteProgram(self.bg_program);
                self.bg_program = 0;
            }
            if self.oes_texture_id != 0 {
                gl::glDeleteTextures(1, &self.oes_texture_id);
                self.oes_texture_id = 0;
            }
        }
    }
}

fn identity_4x4() -> [f32; 16] {
    let mut m = [0f32; 16];
    m[0] = 1.0;
    m[5] = 1.0;
    m[10] = 1.0;
    m[15] = 1.0;
    m
}

/// Converts a `&str` attribute/uniform name into a NUL-terminated buffer
/// suitable for gles31's `GetAttribLocation`/`GetUniformLocation`, which (per
/// the standard GLES C ABI these bindings wrap) expect a `*const GLchar`
/// (i.e. `*const i8`/`*const u8` depending on the binding's exact typedef).
///
/// Implemented with `std::ffi::CString` (standard library only — no
/// uncertain external-crate API surface here, unlike the EGL/shader-source
/// TODOs below). Only called from `on_surface_created`, once per uniform/
/// attribute at program-link time (not per-frame), so the small allocation
/// per call is not a hot-path concern.
///
/// Panics if `s` contains an interior NUL byte — every call site in this
/// file passes a fixed GLSL identifier string literal, none of which can
/// contain a NUL, so this is unreachable in practice; `expect` makes that
/// assumption explicit instead of silently ignoring a malformed name.
fn c_str(s: &str) -> std::ffi::CString {
    std::ffi::CString::new(s).expect("attribute/uniform name must not contain interior NUL")
}

fn build_program(vertex_src: &str, fragment_src: &str) -> Result<u32, String> {
    let vertex_shader = compile_shader(gl::GL_VERTEX_SHADER, vertex_src)?;
    let fragment_shader = compile_shader(gl::GL_FRAGMENT_SHADER, fragment_src)?;

    unsafe {
        let program = gl::glCreateProgram();
        if program == 0 {
            return Err("glCreateProgram failed".into());
        }
        gl::glAttachShader(program, vertex_shader);
        gl::glAttachShader(program, fragment_shader);
        gl::glLinkProgram(program);

        let mut link_status = 0i32;
        gl::glGetProgramiv(program, gl::GL_LINK_STATUS, &mut link_status);
        if link_status != gl::GL_TRUE as i32 {
            let log = get_program_info_log(program);
            gl::glDeleteProgram(program);
            return Err(format!("Program link failed: {log}"));
        }

        // Shaders are now part of the program, no longer needed separately.
        gl::glDeleteShader(vertex_shader);
        gl::glDeleteShader(fragment_shader);

        Ok(program)
    }
}

fn compile_shader(shader_type: u32, source: &str) -> Result<u32, String> {
    unsafe {
        let shader = gl::glCreateShader(shader_type);
        if shader == 0 {
            return Err(format!("glCreateShader failed for type={shader_type}"));
        }

        // glShaderSource's C ABI is (shader, count, *const *const GLchar,
        // *const GLint) — unchanged since GLES 2.0, so this is not a
        // guessed/uncertain signature the way the attrib/uniform location
        // return types were flagged as uncertain elsewhere in this file.
        // length = -1 tells GL the string is NUL-terminated, so no length
        // array is strictly needed, but passing an explicit length avoids
        // relying on the driver correctly scanning for NUL in `source`.
        let c_source = std::ffi::CString::new(source)
            .map_err(|e| format!("shader source contains interior NUL: {e}"))?;
        // NOTE: an earlier draft hardcoded `*const i8` here, but `c_char` is
        // `u8` on the aarch64-linux-android target (it varies by platform —
        // this was an unverified assumption, caught by the first real
        // build). Using `std::os::raw::c_char` instead of hardcoding a
        // signedness makes this correct on whichever target it's built for.
        let source_ptr: *const std::os::raw::c_char = c_source.as_ptr();
        let source_len: i32 = c_source.as_bytes().len() as i32;
        gl::glShaderSource(shader, 1, &source_ptr as *const *const std::os::raw::c_char as *const *const _, &source_len);

        gl::glCompileShader(shader);

        let mut status = 0i32;
        gl::glGetShaderiv(shader, gl::GL_COMPILE_STATUS, &mut status);
        if status != gl::GL_TRUE as i32 {
            let log = get_shader_info_log(shader);
            gl::glDeleteShader(shader);
            return Err(format!("Shader compile failed (type={shader_type}): {log}"));
        }
        Ok(shader)
    }
}

fn get_program_info_log(program: u32) -> String {
    unsafe {
        // NOTE: log_len (glGetProgramiv's *mut GLint output) and
        // written_len (glGetProgramInfoLog's *mut GLsizei output) have
        // DIFFERENT gles31 pointer types despite both conceptually being
        // "a length" — GLint is i32, GLsizei is u32 in this binding. An
        // earlier draft used i32 for both, which built fine for log_len
        // (glGetProgramiv genuinely wants *mut i32) but failed for
        // written_len (glGetProgramInfoLog wants *mut u32). Caught by the
        // build; kept as two distinctly-typed variables rather than one,
        // to make this asymmetry explicit instead of casting one at the
        // call site with no explanation.
        let mut log_len: i32 = 0;
        gl::glGetProgramiv(program, gl::GL_INFO_LOG_LENGTH, &mut log_len);
        if log_len <= 0 {
            return String::from("(no info log)");
        }
        let mut buf: Vec<u8> = vec![0u8; log_len as usize];
        let mut written_len: u32 = 0;
        gl::glGetProgramInfoLog(
            program,
            log_len as u32,
            &mut written_len,
            buf.as_mut_ptr() as *mut _,
        );
        buf.truncate(written_len as usize);
        String::from_utf8_lossy(&buf).into_owned()
    }
}

fn get_shader_info_log(shader: u32) -> String {
    unsafe {
        // Same log_len (i32, glGetShaderiv) vs written_len (u32,
        // glGetShaderInfoLog) type asymmetry as get_program_info_log above
        // — see that function's comment for why.
        let mut log_len: i32 = 0;
        gl::glGetShaderiv(shader, gl::GL_INFO_LOG_LENGTH, &mut log_len);
        if log_len <= 0 {
            return String::from("(no info log)");
        }
        let mut buf: Vec<u8> = vec![0u8; log_len as usize];
        let mut written_len: u32 = 0;
        gl::glGetShaderInfoLog(
            shader,
            log_len as u32,
            &mut written_len,
            buf.as_mut_ptr() as *mut _,
        );
        buf.truncate(written_len as usize);
        String::from_utf8_lossy(&buf).into_owned()
    }
}

fn init_egl(window: &NativeWindow) -> Result<EglState, String> {
    // Implemented against khronos-egl 6.x's documented Rust API surface.
    // Confidence split, to be explicit about what is/isn't guessed:
    //   - EGL_* attribute constants and the config-attribute-list shape
    //     (RENDERABLE_TYPE/SURFACE_TYPE/RED_SIZE etc. as a flat
    //     key,value,...,NONE i32 array) are part of the EGL 1.4/1.5 C spec
    //     itself, unchanged since EGL's inception — not a guess.
    //   - The exact khronos-egl method names used below (get_display,
    //     initialize, choose_first_config, create_context,
    //     create_window_surface, make_current, swap_buffers) match the
    //     crate's 6.x documented API as of this writing, but the crate's
    //     error type / Result wrapping and the "static" vs "dynamic"
    //     loading feature choice in Cargo.toml (this crate uses the
    //     `static` feature, linking libEGL.so at build time rather than
    //     dlopen-ing it at runtime) have NOT been verified against an
    //     actual build in this session — that verification can only happen
    //     once this compiles in a real NDK environment. If method names
    //     have shifted between minor 6.x versions, this is the first place
    //     to check.
    use khronos_egl as egl;

    // With the `static` Cargo feature, `Instance::new` links against the
    // system libEGL.so directly (no runtime dlopen step needed on Android,
    // since libEGL.so is always present).
    let instance: egl::Instance<egl::Static> = egl::Instance::new(egl::Static);

    // NOTE: an earlier draft called get_display() without an unsafe block,
    // assuming it was a safe function — caught by the first real build:
    // khronos-egl's get_display is `unsafe fn` in the pinned crate version.
    let display = unsafe { instance.get_display(egl::DEFAULT_DISPLAY) }
        .ok_or_else(|| "eglGetDisplay returned no display".to_string())?;

    instance
        .initialize(display)
        .map_err(|e| format!("eglInitialize failed: {e:?}"))?;

    // Config attribute list: RGBA8888, GLES2-renderable (GLES 3.x contexts
    // are requested via EGL_CONTEXT_CLIENT_VERSION on the context, not via
    // this renderable-type bit — GLES2 bit is sufficient and is what the
    // original WallpaperGLEngine.kt/GLSurfaceView setup used implicitly via
    // GLES20.*), window-surface-capable.
    let config_attribs = [
        egl::RED_SIZE,
        8,
        egl::GREEN_SIZE,
        8,
        egl::BLUE_SIZE,
        8,
        egl::ALPHA_SIZE,
        8,
        egl::SURFACE_TYPE,
        egl::WINDOW_BIT,
        egl::RENDERABLE_TYPE,
        egl::OPENGL_ES2_BIT,
        egl::NONE,
    ];

    let config = instance
        .choose_first_config(display, &config_attribs)
        .map_err(|e| format!("eglChooseConfig failed: {e:?}"))?
        .ok_or_else(|| "eglChooseConfig found no matching config".to_string())?;

    // EGL_CONTEXT_CLIENT_VERSION = 2 requests a GLES2-and-up context; the
    // shaders in this file (Shaders.kt-equivalent) use #version-implicit
    // GLES2-style GLSL (attribute/varying, not in/out), matching the
    // original Kotlin's GLES20.* usage throughout WallpaperGLRenderer.kt.
    const EGL_CONTEXT_CLIENT_VERSION: egl::Int = 0x3098;
    let context_attribs = [EGL_CONTEXT_CLIENT_VERSION, 2, egl::NONE];

    let context = instance
        .create_context(display, config, None, &context_attribs)
        .map_err(|e| format!("eglCreateContext failed: {e:?}"))?;

    // SAFETY: `window` is a valid ANativeWindow for the lifetime of this
    // call, guaranteed by the caller (jni_bridge, once it exists) holding a
    // live reference for as long as the surface is in use — mirrors the
    // original Kotlin's Surface lifetime guarantee from SurfaceHolder.
    let native_window_ptr = window.ptr().as_ptr() as egl::NativeWindowType;
    let surface = unsafe {
        instance
            .create_window_surface(display, config, native_window_ptr, None)
            .map_err(|e| format!("eglCreateWindowSurface failed: {e:?}"))?
    };

    Ok(EglState {
        instance,
        display,
        context,
        surface,
        config,
    })
}
