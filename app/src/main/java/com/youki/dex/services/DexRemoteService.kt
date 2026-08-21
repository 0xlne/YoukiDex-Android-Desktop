package com.youki.dex.services

import android.app.Service
import android.content.Intent
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import com.youki.dex.bridge.DexBridgeProtocol
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.launch

/**
 * Represents the "add file/folder to desktop" event after unpacking the Bundle.
 */
data class DesktopFileRequest(
    val filePath: String,
    val fileName: String,
    val isDirectory: Boolean
)

/**
 * An internal broadcast (only within the Workshop process) that delivers the
 * event from the service to LauncherActivity. This isn't a replacement for
 * IPC — it's a normal, correct use of SharedFlow between two classes in the
 * same app and same process, after DexRemoteService has already received the
 * message from the external process.
 */
object DesktopBus {
    private val _events = MutableSharedFlow<DesktopFileRequest>(replay = 0)
    val events = _events.asSharedFlow()

    // FIX (Unmanaged CoroutineScope + Crash Risk): the old code used to run
    // CoroutineScope(Dispatchers.Main).launch { } fresh on every call — every
    // call created a new Job with no SupervisorJob and no exception handler.
    // If an exception occurred during emit() (rare but possible), it would
    // propagate as an uncaught exception in an unmanaged scope = the entire
    // app crashes, not just this operation failing. Also, every new
    // CoroutineScope() reserves its own Job unnecessarily instead of using a
    // single fixed scope.
    // Fix: SupervisorJob + CoroutineExceptionHandler confines any failure to
    // this scope only, without taking down the rest of the app.
    private val busScope = CoroutineScope(
        Dispatchers.Main + kotlinx.coroutines.SupervisorJob() +
            kotlinx.coroutines.CoroutineExceptionHandler { _, e ->
                android.util.Log.e("DesktopBus", "emit failed: ${e.message}", e)
            }
    )

    fun emitFromService(request: DesktopFileRequest) {
        busScope.launch {
            _events.emit(request)
        }
    }
}

/**
 * The Messenger service that MaterialFiles binds to from its own process to
 * send an "add this file to the desktop" request. Protected by a signature
 * permission in AndroidManifest, so only an app signed with the same
 * Workshop key can connect to it.
 */
class DexRemoteService : Service() {

    private val incomingHandler = object : Handler(Looper.getMainLooper()) {
        override fun handleMessage(msg: Message) {
            when (msg.what) {
                DexBridgeProtocol.WHAT_ADD_TO_DESKTOP -> {
                    val data: Bundle = msg.data
                    val path = data.getString(DexBridgeProtocol.KEY_FILE_PATH) ?: return
                    val name = data.getString(DexBridgeProtocol.KEY_FILE_NAME) ?: path.substringAfterLast('/')
                    val isDir = data.getBoolean(DexBridgeProtocol.KEY_IS_DIRECTORY, false)

                    DesktopBus.emitFromService(
                        DesktopFileRequest(filePath = path, fileName = name, isDirectory = isDir)
                    )
                }
                else -> super.handleMessage(msg)
            }
        }
    }

    private val messenger = Messenger(incomingHandler)

    override fun onBind(intent: Intent?): IBinder = messenger.binder
}
