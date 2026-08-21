package com.youki.dex.bridge

/**
 * Message codes exchanged between Youki DEX and MaterialFiles via Messenger/Handler.
 * Use Message.what to determine the event type, and Message.data (Bundle) for the payload.
 *
 * This file exists on both sides (Workshop and MaterialFiles) with the exact
 * same values (they don't need to be the same package — just the same
 * What/Keys values, since the data is passed as a raw Bundle).
 */
object DexBridgeProtocol {
    // ---- What arrives at Workshop (from MaterialFiles) ----
    const val WHAT_ADD_TO_DESKTOP = 1001

    // ---- What arrives at MaterialFiles (from Workshop) — used only as a unified key reference ----
    const val ACTION_OPEN_FOLDER = "com.youki.dex.action.OPEN_FOLDER"

    // Shared Bundle keys
    const val KEY_FILE_PATH = "filePath"
    const val KEY_FILE_NAME = "fileName"
    const val KEY_IS_DIRECTORY = "isDirectory"

    // Info about both sides — used by both when building an Intent/ComponentName
    const val WORKSHOP_PACKAGE = "com.youki.dex"
    const val WORKSHOP_SERVICE_CLASS = "com.youki.dex.services.DexRemoteService"
    const val BRIDGE_PERMISSION = "com.youki.dex.permission.DEX_BRIDGE"

    const val MATERIAL_FILES_PACKAGE = "me.zhanghai.android.files"
    const val MATERIAL_FILES_ACTIVITY = "me.zhanghai.android.files.filelist.FileListActivity"
    // A real Action documented in MaterialFiles' own manifest (OpenFileActivity) — used
    // to open a single file. Confirmed from the official source: github.com/zhanghai/MaterialFiles
    const val MATERIAL_FILES_OPEN_FILE_ACTION = "me.zhanghai.android.files.intent.action.OPEN_FILE"
}
