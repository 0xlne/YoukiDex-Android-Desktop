package com.youki.dex.onboarding

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.view.View
import androidx.fragment.app.Fragment
import com.youki.dex.R

/** Step 5 — confirmation screen. OnboardingActivity's Next button becomes "Finish" here.
 *
 * Also introduces three support channels right as the user finishes setup:
 * GitHub star, Discord community, and reporting a bug/idea via GitHub Issues.
 * Same URLs and open-in-browser approach as row_github/row_discord in
 * HelpAboutPreferences (and the same Issues URL used by DebugActivity's
 * crash dialog), kept in sync manually since these are separate screens. */
class OnboardingDoneFragment : Fragment(R.layout.fragment_onboarding_done) {

    override fun onViewCreated(view: View, savedInstanceState: Bundle?) {
        super.onViewCreated(view, savedInstanceState)
        view.findViewById<View>(R.id.row_onboarding_github).setOnClickListener {
            openUrl("https://github.com/mrYouki/YoukiDex-Android-Desktop")
        }
        view.findViewById<View>(R.id.row_onboarding_discord).setOnClickListener {
            openUrl("https://discord.gg/mKkaMxd5M2")
        }
        view.findViewById<View>(R.id.row_onboarding_report_issue).setOnClickListener {
            openUrl("https://github.com/mrYouki/YoukiDex-Android-Desktop/issues")
        }
    }

    private fun openUrl(url: String) =
        startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
}
