package com.youki.dex.livewallpaper.data

import android.content.ContentResolver
import android.content.Context
import android.net.Uri
import android.os.Environment
import java.io.File

/**
 * VideoFileStore — reads videos from two folders:
 *   1) YoukiDEX_Wallpapers/   ← wallpapers the user picked manually
 *   2) YoukiDex/Wallpapers/   ← files downloaded from WorkshopFragment
 *
 * Imports always go to YoukiDEX_Wallpapers (the user's folder).
 */
object VideoFileStore {

    // The user's wallpapers folder (the original)
    const val FOLDER_NAME = "YoukiDEX_Wallpapers"

    /** Strips characters unsafe for filesystem filenames and caps the length. */
    private fun sanitizeVideoFilename(name: String): String {
        val cleaned = name.trim().replace(Regex("""[/\\:*?"<>|\x00-\x1F]"""), "_")
        return cleaned.take(200).ifBlank { "video_${System.currentTimeMillis()}" }
    }


    // The downloads folder from WorkshopFragment
    private const val DOWNLOADS_ROOT      = "YoukiDex"
    private const val DOWNLOADS_WALLPAPERS = "Wallpapers"

    /** The user's wallpapers folder */
    fun folder(): File {
        val dir = File(
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
            FOLDER_NAME
        )
        if (!dir.exists()) dir.mkdirs()
        return dir
    }

    /** The WorkshopFragment downloads folder */
    private fun downloadsFolder(): File =
        File(
            Environment.getExternalStoragePublicDirectory(Environment.DIRECTORY_DOWNLOADS),
            "$DOWNLOADS_ROOT/$DOWNLOADS_WALLPAPERS"
        )

    /** Combines the videos from both folders together, sorted newest-first */
    fun listVideos(): List<VideoFile> {
        val exts = setOf("mp4", "mkv", "webm", "3gp", "mov")

        val fromUser = folder()
            .takeIf { it.exists() }
            ?.listFiles { f -> f.isFile && f.extension.lowercase() in exts }
            ?.toList() ?: emptyList()

        val fromDownloads = downloadsFolder()
            .takeIf { it.exists() }
            ?.listFiles { f -> f.isFile && f.extension.lowercase() in exts }
            ?.toList() ?: emptyList()

        return (fromUser + fromDownloads)
            .sortedByDescending { it.lastModified() }
            .map { VideoFile.fromFile(it) }
    }

    /**
     * Copies a video picked via SAF into the user's YoukiDEX_Wallpapers folder.
     */
    fun importVideo(context: Context, sourceUri: Uri, suggestedName: String?): VideoFile? {
        return try {
            val resolver: ContentResolver = context.contentResolver
            val safeName = (suggestedName?.takeIf { it.isNotBlank() }
                ?: "video_${System.currentTimeMillis()}")
                .let { sanitizeVideoFilename(it) }
            val destFile = File(folder(), safeName)

            resolver.openInputStream(sourceUri)?.use { input ->
                destFile.outputStream().use { output -> input.copyTo(output) }
            } ?: return null

            VideoFile.fromFile(destFile)
        } catch (e: Exception) {
            null
        }
    }

    /**
     * Adds a video that already exists on disk (without copying) — used
     * when the user picks a file from within the app's own folders via InternalFilePicker.
     */
    fun addExistingFile(file: File, displayName: String): VideoFile? {
        return try {
            if (!file.exists() || !file.isFile) return null
            VideoFile.fromFile(file)
        } catch (e: Exception) {
            null
        }
    }

    fun delete(videoUri: String): Boolean {
        return try {
            File(Uri.parse(videoUri).path ?: return false).delete()
        } catch (e: Exception) {
            false
        }
    }
}
