package com.youki.dex.livewallpaper.gl

import android.opengl.GLES20
import android.util.Log

/**
 * GLProgram — compiles and links a Vertex+Fragment shader into a single ready-to-use program.
 * Prints compilation errors clearly instead of the usual silent black screen in OpenGL.
 */
object GLProgram {

    private const val TAG = "GLProgram"

    fun build(vertexSrc: String, fragmentSrc: String): Int {
        val vertexShader   = compile(GLES20.GL_VERTEX_SHADER, vertexSrc)
        val fragmentShader = compile(GLES20.GL_FRAGMENT_SHADER, fragmentSrc)

        val program = GLES20.glCreateProgram()
        if (program == 0) error("glCreateProgram failed")

        GLES20.glAttachShader(program, vertexShader)
        GLES20.glAttachShader(program, fragmentShader)
        GLES20.glLinkProgram(program)

        val linkStatus = IntArray(1)
        GLES20.glGetProgramiv(program, GLES20.GL_LINK_STATUS, linkStatus, 0)
        if (linkStatus[0] != GLES20.GL_TRUE) {
            val log = GLES20.glGetProgramInfoLog(program)
            GLES20.glDeleteProgram(program)
            Log.e(TAG, "Link failed: $log")
            error("Program link failed: $log")
        }

        // The shaders are now part of the program, we no longer need them separately
        GLES20.glDeleteShader(vertexShader)
        GLES20.glDeleteShader(fragmentShader)

        return program
    }

    private fun compile(type: Int, source: String): Int {
        val shader = GLES20.glCreateShader(type)
        if (shader == 0) error("glCreateShader failed for type=$type")

        GLES20.glShaderSource(shader, source)
        GLES20.glCompileShader(shader)

        val status = IntArray(1)
        GLES20.glGetShaderiv(shader, GLES20.GL_COMPILE_STATUS, status, 0)
        if (status[0] != GLES20.GL_TRUE) {
            val log = GLES20.glGetShaderInfoLog(shader)
            GLES20.glDeleteShader(shader)
            Log.e(TAG, "Compile failed (type=$type): $log")
            error("Shader compile failed: $log")
        }
        return shader
    }
}
