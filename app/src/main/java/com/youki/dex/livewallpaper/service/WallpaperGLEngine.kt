package com.youki.dex.livewallpaper.service

import android.opengl.EGL14
import android.opengl.EGLConfig
import android.opengl.EGLContext
import android.opengl.EGLDisplay
import android.opengl.EGLSurface
import android.opengl.GLSurfaceView
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.view.SurfaceHolder

/**
 * WallpaperGLEngine — a [GLSurfaceView] replacement built for live wallpaper
 * services, with no dependency on a real View/Window.
 *
 * ┌────────────────────────────────────────────────────────────────────────┐
 * │  Why can't we use GLSurfaceView here at all (discovered from Crash #2)? │
 * │  SurfaceView on modern Android (10+) needs a real ViewRootImpl (via an  │
 * │  actual WindowManager.addView) to register its SurfaceChangedCallback.  │
 * │  A live wallpaper service has no real View hierarchy — it only gives us │
 * │  a raw SurfaceHolder. Manually calling onAttachedToWindow() (an old     │
 * │  trick from ~2010) used to fool GLSurfaceView, but now it crashes       │
 * │  because getViewRootImpl() genuinely returns null — there's no real     │
 * │  window for it to attach to.                                            │
 * │                                                                          │
 * │  The fix: a simple manual EGL thread — the same idea GLSurfaceView      │
 * │  implements internally (EGLDisplay/EGLContext/EGLSurface + a draw loop  │
 * │  on a dedicated thread), but tied directly to the raw holder.surface,   │
 * │  with no View/Window involvement at all. [WallpaperGLRenderer] stays    │
 * │  exactly as it is (a GLSurfaceView.Renderer) — we call its three        │
 * │  methods (onSurfaceCreated/onSurfaceChanged/onDrawFrame) manually from  │
 * │  this thread instead of GLSurfaceView doing it for us.                 │
 * └────────────────────────────────────────────────────────────────────────┘
 */
class WallpaperGLEngine(
    private val renderer: GLSurfaceView.Renderer
) {
    companion object { private const val TAG = "WallpaperGLEngine" }

    private var thread: HandlerThread? = null
    private var handler: Handler? = null

    private var eglDisplay: EGLDisplay? = null
    private var eglContext: EGLContext? = null
    private var eglSurface: EGLSurface? = null
    private var eglConfig: EGLConfig? = null

    @Volatile private var surfaceReady = false
    @Volatile private var paused = false
    /** Prevents multiple overlapping render requests from stacking up in the Handler's queue (see the requestRender explanation below) */
    @Volatile private var renderPending = false

    fun surfaceCreated(holder: SurfaceHolder) {
        // FIX (resource leak): if the system called surfaceCreated() twice
        // without a surfaceDestroyed() in between (this can happen quickly on
        // the preview screen when navigating between pages), we used to
        // directly replace the old thread/handler without closing them first —
        // leaving the old HandlerThread running forever, with the
        // EGLContext/EGLSurface tied to a dead surface staying reserved in
        // memory. Now we clean up the old one first.
        if (thread != null) {
            surfaceDestroyed()
        }
        val t = HandlerThread("WallpaperGLThread").apply { start() }
        thread = t
        val h = Handler(t.looper)
        handler = h
        h.post {
            try {
                initEgl(holder)
                renderer.onSurfaceCreated(null, null)
                surfaceReady = true
                Log.d(TAG, "EGL surface created")
            } catch (e: Exception) {
                Log.e(TAG, "Failed to create EGL surface", e)
            }
        }
    }

    fun surfaceChanged(width: Int, height: Int) {
        handler?.post {
            if (!surfaceReady) return@post
            renderer.onSurfaceChanged(null, width, height)
            drawFrame()
        }
    }

    fun surfaceDestroyed() {
        // FIX (Crash — Race Condition): the old code used to call
        // thread?.quitSafely() directly right after handler?.post {
        // releaseEgl() } — but quitSafely() runs immediately on the Main
        // Thread, ending the HandlerThread before the post { releaseEgl() } on
        // the EGL thread gets to run. Result: eglDestroyContext gets called on
        // a dead thread, or never gets called at all → the EGLContext leaks,
        // and any later GL call from the wrong thread = crash.
        //
        // The fix: a CountDownLatch guarantees releaseEgl() finishes before the thread is stopped.
        surfaceReady = false
        val latch = java.util.concurrent.CountDownLatch(1)
        val h = handler
        if (h != null) {
            h.post {
                try { releaseEgl() } finally { latch.countDown() }
            }
            latch.await(2, java.util.concurrent.TimeUnit.SECONDS)
        }
        thread?.quitSafely()
        thread = null
        handler = null
    }

    /**
     * Requests a new frame to be drawn — equivalent to GLSurfaceView.requestRender().
     *
     * FIX (dropped frames and gradually worsening performance on long-running
     * wallpapers): before, every call added a new `post {}` message to the
     * [HandlerThread]'s queue with no check at all — and two independent
     * sources were calling it in parallel (every video frame from
     * SurfaceTexture + the service's periodic FpsLimiter loop), so if the
     * drawing thread lagged even slightly (a slow GPU, device heat, etc.),
     * messages piled up in the queue faster than they were processed — a
     * negative feedback loop: a bigger queue leads to more delay, which leads
     * to a bigger queue, getting worse the longer the wallpaper ran. Every old
     * queued message eventually got drawn anyway even though it no longer
     * represented the current state = wasted frames and accumulating time drift.
     *
     * The fix: a simple [renderPending] flag — we don't add a new render
     * request to the queue if one is already pending and hasn't run yet. This
     * "merges" any fast consecutive requests into just one, so the queue never
     * builds up no matter how long the wallpaper has been running.
     */
    fun requestRender() {
        if (paused) return
        if (renderPending) return
        renderPending = true
        handler?.post {
            renderPending = false
            if (surfaceReady) drawFrame()
        }
    }

    fun onPause() { paused = true }
    fun onResume() { paused = false; requestRender() }

    fun release() {
        surfaceDestroyed()
    }

    private fun drawFrame() {
        try {
            renderer.onDrawFrame(null)
            EGL14.eglSwapBuffers(eglDisplay, eglSurface)
        } catch (e: Exception) {
            Log.e(TAG, "Failed to draw frame", e)
        }
    }

    private fun initEgl(holder: SurfaceHolder) {
        val display = EGL14.eglGetDisplay(EGL14.EGL_DEFAULT_DISPLAY)
        if (display == EGL14.EGL_NO_DISPLAY) error("eglGetDisplay failed")
        eglDisplay = display

        val version = IntArray(2)
        if (!EGL14.eglInitialize(display, version, 0, version, 1)) {
            error("eglInitialize failed")
        }

        val attribList = intArrayOf(
            EGL14.EGL_RENDERABLE_TYPE, EGL14.EGL_OPENGL_ES2_BIT,
            EGL14.EGL_SURFACE_TYPE, EGL14.EGL_WINDOW_BIT,
            EGL14.EGL_RED_SIZE, 8,
            EGL14.EGL_GREEN_SIZE, 8,
            EGL14.EGL_BLUE_SIZE, 8,
            EGL14.EGL_ALPHA_SIZE, 8,
            // FIX (hardening): the renderer clears GL_DEPTH_BUFFER_BIT, but we
            // weren't actually requesting a depth buffer from EGL at all — on
            // most chipsets this silently passes, but some GPUs (especially
            // older Mali ones) give a warning or unexpected behavior. We
            // request it explicitly, matching GLSurfaceView's default (16-bit
            // is enough, we don't actually use depth testing).
            EGL14.EGL_DEPTH_SIZE, 16,
            EGL14.EGL_NONE
        )
        val configs = arrayOfNulls<EGLConfig>(1)
        val numConfigs = IntArray(1)
        EGL14.eglChooseConfig(display, attribList, 0, configs, 0, 1, numConfigs, 0)
        if (numConfigs[0] <= 0 || configs[0] == null) error("No suitable EGLConfig found")
        val config = configs[0]!!
        eglConfig = config

        val contextAttribs = intArrayOf(EGL14.EGL_CONTEXT_CLIENT_VERSION, 2, EGL14.EGL_NONE)
        val context = EGL14.eglCreateContext(display, config, EGL14.EGL_NO_CONTEXT, contextAttribs, 0)
        if (context == EGL14.EGL_NO_CONTEXT) error("eglCreateContext failed")
        eglContext = context

        val surfaceAttribs = intArrayOf(EGL14.EGL_NONE)
        val surface = EGL14.eglCreateWindowSurface(display, config, holder.surface, surfaceAttribs, 0)
        if (surface == EGL14.EGL_NO_SURFACE) error("eglCreateWindowSurface failed")
        eglSurface = surface

        if (!EGL14.eglMakeCurrent(display, surface, surface, context)) {
            error("eglMakeCurrent failed")
        }
    }

    private fun releaseEgl() {
        val display = eglDisplay
        if (display != null) {
            EGL14.eglMakeCurrent(display, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_SURFACE, EGL14.EGL_NO_CONTEXT)
            eglSurface?.let { EGL14.eglDestroySurface(display, it) }
            eglContext?.let { EGL14.eglDestroyContext(display, it) }
            EGL14.eglTerminate(display)
        }
        eglDisplay = null
        eglSurface = null
        eglContext = null
        eglConfig = null
    }
}
