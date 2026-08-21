//! shell_client — Rust port of ShellManager.kt.
//!
//! TCP client for the `youki_shell_server` socket on 127.0.0.1:7171.
//! Protocol (unchanged from the original Kotlin implementation):
//!   1. Server sends a line: MAGIC ("YOUKI_SHELL_V1")
//!   2. Client sends:        "TOKEN:<token>"
//!      Server replies:      "TOKEN_OK"
//!   3. Client sends:        "CMD:<command>"
//!      Server streams output lines, terminated by a "__DONE__" sentinel line
//!   4. Client sends:        "EXIT"
//!
//! State ownership: `sessionToken` / `isConnected` in the Kotlin original
//! were plain `@Volatile` object fields. Here they live behind a `Mutex`
//! inside a `once_cell::sync::Lazy` static, since JNI calls can arrive on
//! different JVM-attached threads concurrently (the original relied on a
//! single-threaded CoroutineScope(Dispatchers.IO) queue to avoid races;
//! Rust has no equivalent implicit queue, so explicit locking replaces it).
//!
//! What did NOT move here: the BroadcastReceiver that listens for
//! `com.youki.dex.SHELL_SERVER_READY` and the Shizuku/Root process-launch
//! calls (`ShizukoManager` / `RootManager` are Binder-bound, framework-only
//! APIs — per this project's own root.rs precedent, those stay in Kotlin).
//! Kotlin receives the broadcast token and passes it in via
//! `nativeSetSessionToken`; Rust owns everything from there down.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::sync::Mutex;
use std::time::Duration;

use once_cell::sync::Lazy;

const HOST: &str = "127.0.0.1";
const PORT: u16 = 7171;
const MAGIC: &str = "YOUKI_SHELL_V1";

const EXEC_TIMEOUT: Duration = Duration::from_secs(10);
const PING_TIMEOUT: Duration = Duration::from_secs(3);

struct ShellState {
    session_token: Option<String>,
    is_connected: bool,
}

static STATE: Lazy<Mutex<ShellState>> = Lazy::new(|| {
    Mutex::new(ShellState {
        session_token: None,
        is_connected: false,
    })
});

/// Called when the `SHELL_SERVER_READY` broadcast is received in Kotlin.
/// Mirrors the original's `sessionToken = token; isConnected = false`.
pub fn set_session_token(token: String) {
    let mut state = STATE.lock().unwrap();
    state.session_token = Some(token);
    state.is_connected = false; // force reconnect with the new token
}

/// Clears session state — Rust equivalent of the token-related half of
/// `ShellManager.destroy()`. (The BroadcastReceiver unregistration itself
/// stays in Kotlin, since Rust has no Context/Receiver to hold.)
pub fn clear_session(){
    let mut state = STATE.lock().unwrap();
    state.session_token = None;
    state.is_connected = false;
}

/// Mirrors `ShellManager.isAvailable`.
pub fn is_available() -> bool {
    let state = STATE.lock().unwrap();
    state.session_token.is_some() && state.is_connected
}

fn current_token() -> Option<String> {
    STATE.lock().unwrap().session_token.clone()
}

fn set_connected(connected: bool) {
    STATE.lock().unwrap().is_connected = connected;
}

/// One round trip over a fresh TCP connection: handshake, send `line`,
/// then read+return every line up to (not including) `__DONE__`.
/// Shared by `exec_sync` and `ping` — the only difference between them is
/// what gets sent after the handshake and how the reply is interpreted.
fn handshake(stream: &mut TcpStream, token: &str) -> std::io::Result<BufReader<TcpStream>> {
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut magic_line = String::new();
    reader.read_line(&mut magic_line)?;
    if magic_line.trim_end_matches(['\r', '\n']) != MAGIC {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "handshake: unexpected magic line",
        ));
    }

    writeln!(stream, "TOKEN:{}", token)?;
    let mut ack_line = String::new();
    reader.read_line(&mut ack_line)?;
    if ack_line.trim_end_matches(['\r', '\n']) != "TOKEN_OK" {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "handshake: token rejected",
        ));
    }

    Ok(reader)
}

/// Rust port of `ShellManager.execSync()`. Blocking — call from a
/// background thread on the Kotlin side, same calling convention as the
/// original (it was documented "call from IO thread only").
pub fn exec_sync(cmd: &str) -> Option<String> {
    let token = current_token()?;

    let result = (|| -> std::io::Result<String> {
        let mut stream = TcpStream::connect((HOST, PORT))?;
        stream.set_read_timeout(Some(EXEC_TIMEOUT))?;
        stream.set_write_timeout(Some(EXEC_TIMEOUT))?;

        let mut reader = handshake(&mut stream, &token)?;
        set_connected(true);

        writeln!(stream, "CMD:{}", cmd)?;

        let mut out = String::new();
        loop {
            let mut line = String::new();
            let n = reader.read_line(&mut line)?;
            if n == 0 {
                break; // peer closed the connection
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line == "__DONE__" {
                break;
            }
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(line);
        }

        let _ = writeln!(stream, "EXIT");
        Ok(out.trim().to_string())
    })();

    match result {
        Ok(s) => Some(s),
        Err(_) => {
            set_connected(false);
            None
        }
    }
}

/// Rust port of `ShellManager.ping()` (private in the original; exposed
/// here since Kotlin's `init()` triggers a ping immediately on receiving
/// a fresh token, and that call site now needs to reach it from the JNI
/// side rather than an internal coroutine).
pub fn ping() -> bool {
    let token = match current_token() {
        Some(t) => t,
        None => return false,
    };

    let result = (|| -> std::io::Result<bool> {
        let mut stream = TcpStream::connect((HOST, PORT))?;
        stream.set_read_timeout(Some(PING_TIMEOUT))?;
        stream.set_write_timeout(Some(PING_TIMEOUT))?;

        let mut reader = handshake(&mut stream, &token)?;

        writeln!(stream, "PING")?;
        let mut pong_line = String::new();
        reader.read_line(&mut pong_line)?;
        let _ = writeln!(stream, "EXIT");

        Ok(pong_line.trim_end_matches(['\r', '\n']) == "PONG")
    })();

    match result {
        Ok(ok) => {
            set_connected(ok);
            ok
        }
        Err(_) => {
            set_connected(false);
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Pure-logic tests only — no real socket, since that needs a running
    // youki_shell_server. Covers the state machine (token/connected flags),
    // which is the part most likely to regress silently across edits.

    #[test]
    fn no_token_means_unavailable_and_exec_short_circuits() {
        clear_session();
        assert!(!is_available());
        assert_eq!(exec_sync("echo hi"), None);
        assert!(!ping());
    }

    #[test]
    fn setting_token_marks_disconnected_until_pinged() {
        clear_session();
        set_session_token("abc123".to_string());
        // Token present but not yet connected -> not available yet,
        // matching `isConnected = false // force reconnect` in the original.
        assert!(!is_available());
    }

    #[test]
    fn clear_session_resets_everything() {
        set_session_token("abc123".to_string());
        clear_session();
        assert!(!is_available());
        assert_eq!(current_token(), None);
    }
}
