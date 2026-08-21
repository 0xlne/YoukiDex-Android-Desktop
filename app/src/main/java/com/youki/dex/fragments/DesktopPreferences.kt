package com.youki.dex.fragments

import android.os.Bundle
import android.view.LayoutInflater
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.TextView
import androidx.appcompat.app.AlertDialog
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import com.youki.dex.R
import com.youki.dex.utils.DesktopGridPrefs

/**
 * Desktop settings fragment.
 * Grid size is now a free-text input — the user types "columns x rows"
 * (e.g. "10x15" or "5x2") instead of choosing from a fixed list.
 */
class DesktopPreferences : PreferenceFragmentCompat() {

    override fun onCreatePreferences(arg0: Bundle?, arg1: String?) {
        setPreferencesFromResource(R.xml.preferences_desktop, arg1)

        findPreference<Preference>("desktop_grid_custom")?.let { pref ->
            val cols = DesktopGridPrefs.getColumns(requireContext())
            val rows = DesktopGridPrefs.getRows(requireContext())
            pref.summary = "${cols} × ${rows}"

            pref.setOnPreferenceClickListener {
                showGridInputDialog()
                true
            }
        }
    }

    private fun showGridInputDialog() {
        val ctx = requireContext()
        val cols = DesktopGridPrefs.getColumns(ctx)
        val rows = DesktopGridPrefs.getRows(ctx)

        // Build dialog layout programmatically so we don't need a new XML file.
        val layout = LinearLayout(ctx).apply {
            orientation = LinearLayout.VERTICAL
            val pad = (20 * resources.displayMetrics.density).toInt()
            setPadding(pad, pad, pad, 0)
        }

        val labelCols = TextView(ctx).apply { text = getString(R.string.grid_columns_label) }
        val etCols = EditText(ctx).apply {
            inputType = android.text.InputType.TYPE_CLASS_NUMBER
            setText(cols.toString())
            hint = "5"
        }
        val labelRows = TextView(ctx).apply {
            text = getString(R.string.grid_rows_label)
            val marginDp = (8 * resources.displayMetrics.density).toInt()
            val lp = LinearLayout.LayoutParams(
                LinearLayout.LayoutParams.WRAP_CONTENT,
                LinearLayout.LayoutParams.WRAP_CONTENT
            )
            lp.topMargin = marginDp
            layoutParams = lp
        }
        val etRows = EditText(ctx).apply {
            inputType = android.text.InputType.TYPE_CLASS_NUMBER
            setText(rows.toString())
            hint = "7"
        }

        layout.addView(labelCols)
        layout.addView(etCols)
        layout.addView(labelRows)
        layout.addView(etRows)

        AlertDialog.Builder(ctx)
            .setTitle(getString(R.string.desktop_grid_size_title))
            .setView(layout)
            .setPositiveButton(android.R.string.ok) { _, _ ->
                val newCols = etCols.text.toString().toIntOrNull()
                    ?.coerceIn(DesktopGridPrefs.MIN_DIM, DesktopGridPrefs.MAX_DIM)
                    ?: cols
                val newRows = etRows.text.toString().toIntOrNull()
                    ?.coerceIn(DesktopGridPrefs.MIN_DIM, DesktopGridPrefs.MAX_DIM)
                    ?: rows
                DesktopGridPrefs.setGridSize(ctx, newCols, newRows)
                // Update summary shown on the preference row.
                findPreference<Preference>("desktop_grid_custom")?.summary =
                    "${newCols} × ${newRows}"
                // Rebuild desktop icons with the new grid size.
                (activity as? com.youki.dex.activities.LauncherActivity)?.loadDesktopApps()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }
}
