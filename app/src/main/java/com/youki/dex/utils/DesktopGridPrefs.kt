package com.youki.dex.utils

import android.content.Context
import androidx.preference.PreferenceManager

/**
 * Single source of truth for the desktop grid dimensions.
 * Users can now freely type any width × height (e.g. "10x15" for a large
 * screen or "5x2" for a small one). Hard limits are kept only as a safety
 * floor/ceiling so the grid never becomes unusable:
 *   - Minimum 1 column or row (can't have 0).
 *   - Maximum 30 per dimension (sanity cap; beyond this icons become tiny).
 *
 * All grid math (collision, pixel↔cell, push-to-resolve) lives in Rust —
 * see desktop_grid.rs and DesktopGridManager.
 */
object DesktopGridPrefs {
    const val KEY_COLUMNS = "desktop_grid_columns"
    const val KEY_ROWS    = "desktop_grid_rows"
    const val DEFAULT_COLUMNS = 5
    const val DEFAULT_ROWS    = 7
    // Safety limits only — the UI now lets users type freely.
    const val MIN_DIM = 1
    const val MAX_DIM = 30

    fun getColumns(context: Context): Int =
        PreferenceManager.getDefaultSharedPreferences(context)
            .getInt(KEY_COLUMNS, DEFAULT_COLUMNS).coerceIn(MIN_DIM, MAX_DIM)

    fun getRows(context: Context): Int =
        PreferenceManager.getDefaultSharedPreferences(context)
            .getInt(KEY_ROWS, DEFAULT_ROWS).coerceIn(MIN_DIM, MAX_DIM)

    fun setGridSize(context: Context, columns: Int, rows: Int) {
        PreferenceManager.getDefaultSharedPreferences(context).edit()
            .putInt(KEY_COLUMNS, columns.coerceIn(MIN_DIM, MAX_DIM))
            .putInt(KEY_ROWS,    rows.coerceIn(MIN_DIM, MAX_DIM))
            .apply()
    }

    /** Widget corner style: rounded vs sharp corners. */
    const val KEY_WIDGET_CORNER_STYLE   = "widget_corner_style"
    const val CORNER_STYLE_ROUNDED      = "rounded"
    const val CORNER_STYLE_SHARP        = "sharp"

    fun getWidgetCornerStyle(context: Context): String =
        PreferenceManager.getDefaultSharedPreferences(context)
            .getString(KEY_WIDGET_CORNER_STYLE, CORNER_STYLE_ROUNDED) ?: CORNER_STYLE_ROUNDED

    fun setWidgetCornerStyle(context: Context, style: String) {
        PreferenceManager.getDefaultSharedPreferences(context).edit()
            .putString(KEY_WIDGET_CORNER_STYLE, style).apply()
    }
}
