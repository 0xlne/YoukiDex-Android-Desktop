package com.youki.dex.fragments

import android.content.Context
import android.os.Bundle
import android.text.InputType
import android.view.inputmethod.EditorInfo
import androidx.preference.EditTextPreference
import androidx.preference.Preference
import androidx.preference.PreferenceFragmentCompat
import androidx.preference.PreferenceManager
import com.google.android.material.dialog.MaterialAlertDialogBuilder
import com.google.android.material.materialswitch.MaterialSwitch
import com.google.android.material.slider.LabelFormatter
import com.youki.dex.R
import com.youki.dex.preferences.SliderPreference
import androidx.core.content.edit

class DockPreferences : PreferenceFragmentCompat() {
    override fun onCreatePreferences(arg0: Bundle?, arg1: String?) {
        setPreferencesFromResource(R.xml.preferences_dock, arg1)

        // Dock height slider
        val dockHeight = findPreference<SliderPreference>("dock_height")!!
        dockHeight.setOnDialogShownListener(object : SliderPreference.OnDialogShownListener {
            override fun onDialogShown() {
                val slider = dockHeight.slider ?: return
                slider.isTickVisible = false
                slider.labelBehavior = LabelFormatter.LABEL_GONE
                slider.stepSize = 1f
                slider.value = dockHeight.sharedPreferences?.getString("dock_height", "48")?.toFloatOrNull() ?: 48f
                slider.valueFrom = 44f
                slider.valueTo = 80f
                slider.addOnChangeListener { _, value, _ ->
                    dockHeight.sharedPreferences?.edit { putString("dock_height", value.toInt().toString()) }
                }
            }
        })

        // QS button size slider
        val qsBtnSize = findPreference<SliderPreference>("qs_btn_size")
        qsBtnSize?.setOnDialogShownListener(object : SliderPreference.OnDialogShownListener {
            override fun onDialogShown() {
                val slider = qsBtnSize.slider ?: return
                slider.isTickVisible = false
                slider.labelBehavior = LabelFormatter.LABEL_GONE
                slider.stepSize = 1f
                slider.value = qsBtnSize.sharedPreferences?.getString("qs_btn_size", "38")?.toFloatOrNull() ?: 38f
                slider.valueFrom = 28f
                slider.valueTo = 52f
                slider.addOnChangeListener { _, value, _ ->
                    qsBtnSize.sharedPreferences?.edit { putString("qs_btn_size", value.toInt().toString()) }
                }
            }
        })

        findPreference<Preference>("auto_pin")!!.setOnPreferenceClickListener {
            showAutopinDialog(requireContext())
            false
        }
        // Swipe activation mode removed: hiding the dock left a small
        // (dock_activation_area) overlay window still holding its bounds,
        // which stole touches from whatever was underneath even though
        // nothing was visible there. Handle mode is the only mode now, so
        // "activation_method" and "dock_activation_area" are no longer
        // user-facing — force the pref to "handle" and hide both rows.
        findPreference<Preference>("activation_method")?.sharedPreferences?.edit {
            putString("activation_method", "handle")
        }
        findPreference<Preference>("activation_method")?.isVisible = false
        val activationArea = findPreference<SliderPreference>("dock_activation_area")
        activationArea?.isVisible = false

        val handleOpacity = findPreference<SliderPreference>("handle_opacity")
        handleOpacity!!.isVisible = true
        handleOpacity.setOnDialogShownListener(object : SliderPreference.OnDialogShownListener {
            override fun onDialogShown() {
                val slider = handleOpacity.slider
                slider.isTickVisible = false
                slider.labelBehavior = LabelFormatter.LABEL_GONE
                slider.stepSize = 0.1f
                slider.value =
                    handleOpacity.sharedPreferences?.getString(handleOpacity.key, "0.5f")
                        ?.toFloatOrNull() ?: 0.5f
                slider.valueFrom = 0.2f
                slider.valueTo = 1f
                slider.addOnChangeListener { _, value, _ ->
                    handleOpacity.sharedPreferences!!.edit {
                        putString(handleOpacity.key, value.toString())
                    }
                }
            }
        })
        val handlePosition = findPreference<Preference>("handle_position")
        handlePosition!!.isVisible = true

        val maxRunningApps: EditTextPreference = findPreference("max_running_apps")!!
        maxRunningApps.setOnBindEditTextListener { editText ->
            editText.inputType = InputType.TYPE_CLASS_NUMBER
            editText.imeOptions = EditorInfo.IME_ACTION_GO
        }
        maxRunningApps.setOnPreferenceChangeListener { _, newValue ->
            val value = newValue as String
            value.isNotEmpty() && value.toInt() < 50
        }
    }

    private fun showAutopinDialog(context: Context) {
        val sharedPreferences = PreferenceManager.getDefaultSharedPreferences(context)
        val editor = sharedPreferences.edit()
        val dialogBuilder = MaterialAlertDialogBuilder(context)
        dialogBuilder.setTitle(R.string.auto_pin_summary)
        val view = layoutInflater.inflate(R.layout.dialog_auto_pin, null)
        val startupSwitch = view.findViewById<MaterialSwitch>(R.id.pin_startup_switch)
        val windowedSwitch = view.findViewById<MaterialSwitch>(R.id.pin_window_switch)
        val fullscreenSwitch = view.findViewById<MaterialSwitch>(R.id.unpin_fullscreen_switch)
        startupSwitch.isChecked = sharedPreferences.getBoolean("pin_dock", true)
        windowedSwitch.isChecked = sharedPreferences.getBoolean("auto_pin", true)
        fullscreenSwitch.isChecked = sharedPreferences.getBoolean("auto_unpin", true)
        startupSwitch.setOnCheckedChangeListener { _, checked ->
            editor.putBoolean(
                "pin_dock",
                checked
            ).apply()
        }
        windowedSwitch.setOnCheckedChangeListener { _, checked ->
            editor.putBoolean(
                "auto_pin",
                checked
            ).commit()
        }
        fullscreenSwitch.setOnCheckedChangeListener { _, checked ->
            editor.putBoolean(
                "auto_unpin",
                checked
            ).commit()
        }
        dialogBuilder.setView(view)
        dialogBuilder.setPositiveButton(R.string.ok, null)
        dialogBuilder.show()
    }
}
