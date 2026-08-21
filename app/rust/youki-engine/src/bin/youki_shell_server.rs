//! youki_shell_server — standalone root/shell TCP socket server.
//!
//! Ported 1:1 from ShellServer.kt (see git history / handoff doc), with one
//! architectural change: this is now a true native executable, launched
//! directly (`su -c <path-to-binary>` or via Shizuku's runShell), rather
//! than a Kotlin `fun main()` bootstrapped through `app_process` as a JVM
//! process. Same protocol, same behavior, same security properties —
//! listens on 127.0.0.1 only (never 0.0.0.0), requires a random per-run
//! session token before accepting any command, reads stdout/stderr in
//! parallel to avoid the classic pipe-buffer deadlock, and refuses to run
//! unless started as uid 0 (root) or 2000 (shell).
//!
//! Protocol (unchanged from the Kotlin original — ShellManager.kt, the
//! client side, does not need to change to talk to this binary):
//!   server -> client: "YOUKI_SHELL_V1"
//!   client -> server: "TOKEN:<token>"
//!   server -> client: "TOKEN_OK" | "ERR:Invalid token"
//!   client -> server: "PING" | "UID" | "EXIT" | "CMD:<command>"
//!   server -> client: "PONG" | "uid=<n>" | "BYE" | <output lines>, "__DONE__"
//!
//! Startup: broadcasts `com.youki.dex.SHELL_SERVER_READY` with the session
//! token via `am broadcast`, exactly as the original did — ShellManager.kt
//! listens for this to learn the token and know the server is up.

use std::io::{BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddrV4, TcpListener, TcpStream};
use std::process::Command;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

const PORT: u16 = 7171;
const MAGIC: &str = "YOUKI_SHELL_V1";
const CMD_TIMEOUT: Duration = Duration::from_secs(10);

fn main() {
    let uid = getuid();
    println!("[{MAGIC}] ShellServer starting... uid={uid}");

    if uid != 0 && uid != 2000 {
        eprintln!("[{MAGIC}] ERROR: Must run as root (0) or shell (2000), got uid={uid}");
        std::process::exit(1);
    }

    println!("[{MAGIC}] Running as uid={uid}");
    println!("[{MAGIC}] Listening on localhost:{PORT}...");

    let session_token = generate_session_token();

    // Bind to localhost only — mirrors the Kotlin original's explicit
    // InetAddress.getByName("127.0.0.1") (never 0.0.0.0).
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, PORT);
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[{MAGIC}] Fatal: {e}");
            std::process::exit(1);
        }
    };
    println!("[{MAGIC}] Ready!");

    // Broadcast the token so ShellManager (Kotlin, running inside the app
    // process) learns it and can start connecting.
    let _ = Command::new("am")
        .args([
            "broadcast",
            "-a",
            "com.youki.dex.SHELL_SERVER_READY",
            "--es",
            "token",
            &session_token,
            "--receiver-include-background",
        ])
        .status();

    for incoming in listener.incoming() {
        match incoming {
            Ok(stream) => {
                let token = session_token.clone();
                thread::spawn(move || handle_client(stream, &token));
            }
            Err(e) => {
                eprintln!("[{MAGIC}] Accept error: {e}");
            }
        }
    }
}

fn handle_client(stream: TcpStream, session_token: &str) {
    // Mirrors Kotlin's `try { ... } finally { socket.close() }` — TcpStream
    // already closes its socket on Drop, so no explicit finally-equivalent
    // is needed; early returns below are sufficient.
    let mut writer = match stream.try_clone() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("[{MAGIC}] Client error: {e}");
            return;
        }
    };
    let mut reader = BufReader::new(stream);

    if writeln_flush(&mut writer, MAGIC).is_err() {
        return;
    }

    // Verify the session token before any command.
    let mut token_line = String::new();
    if reader.read_line(&mut token_line).unwrap_or(0) == 0 {
        return;
    }
    let token_line = token_line.trim_end_matches(['\r', '\n']);
    let supplied = token_line.strip_prefix("TOKEN:");
    if supplied != Some(session_token) {
        let _ = writeln_flush(&mut writer, "ERR:Invalid token");
        return;
    }
    if writeln_flush(&mut writer, "TOKEN_OK").is_err() {
        return;
    }

    loop {
        let mut line = String::new();
        let n = match reader.read_line(&mut line) {
            Ok(n) => n,
            Err(_) => break,
        };
        if n == 0 {
            break; // EOF
        }
        let line = line.trim_end_matches(['\r', '\n']);

        if line == "PING" {
            if writeln_flush(&mut writer, "PONG").is_err() {
                break;
            }
        } else if line == "UID" {
            if writeln_flush(&mut writer, &format!("uid={}", getuid())).is_err() {
                break;
            }
        } else if line == "EXIT" {
            let _ = writeln_flush(&mut writer, "BYE");
            break;
        } else if let Some(cmd) = line.strip_prefix("CMD:") {
            let result = run_command(cmd);
            if writeln_flush(&mut writer, &result).is_err() {
                break;
            }
            if writeln_flush(&mut writer, "__DONE__").is_err() {
                break;
            }
        } else {
            if writeln_flush(&mut writer, "ERR:Unknown command").is_err() {
                break;
            }
        }
    }
}

/// Runs `sh -c <cmd>`, reading stdout and stderr in parallel (via two
/// threads + a channel) to avoid the classic deadlock where a child fills
/// one pipe's OS buffer while nobody is draining it — the same fix the
/// Kotlin original applied (Gap 73), ported to Rust's process API, which
/// has the identical hazard for the identical reason (two separate OS
/// pipes, no built-in concurrent draining).
fn run_command(cmd: &str) -> String {
    let mut child = match Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return format!("ERROR: {e}"),
    };

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    let (stdout_tx, stdout_rx) = mpsc::channel();
    let (stderr_tx, stderr_rx) = mpsc::channel();

    thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut p) = stdout_pipe {
            let _ = p.read_to_string(&mut buf);
        }
        let _ = stdout_tx.send(buf);
    });
    thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut p) = stderr_pipe {
            let _ = p.read_to_string(&mut buf);
        }
        let _ = stderr_tx.send(buf);
    });

    // wait_timeout isn't in std; approximate the Kotlin original's
    // `process.waitFor(10, TimeUnit.SECONDS)` by polling try_wait, which
    // has the same externally-observable timeout behavior without an
    // extra dependency for this one call site.
    let deadline = std::time::Instant::now() + CMD_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }

    // IMPORTANT: do not call stdout_thread.join() / stderr_thread.join()
    // here — JoinHandle::join() blocks until the thread actually returns,
    // with no timeout parameter (unlike Java's Thread.join(2000), which the
    // Kotlin original relied on for its own 2-second cutoff). If the
    // process didn't fully die from kill() above (rare but possible for a
    // wedged child), the reader thread's read_to_string() would still be
    // blocked on the pipe, and join() would then hang this handler thread
    // forever. Using recv_timeout directly on the channel achieves the
    // same "give it 2 more seconds, then give up" behavior without ever
    // blocking indefinitely: if the reader thread hasn't sent by the
    // timeout, we just stop waiting and leave it to finish (or stay
    // blocked) on its own — it holds no resources this function needs.
    let stdout = stdout_rx.recv_timeout(Duration::from_millis(2000)).unwrap_or_default();
    let stderr = stderr_rx.recv_timeout(Duration::from_millis(2000)).unwrap_or_default();

    let mut out = String::new();
    let stdout_trim = stdout.trim();
    let stderr_trim = stderr.trim();
    if !stdout_trim.is_empty() {
        out.push_str(stdout_trim);
    }
    if !stderr_trim.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(stderr_trim);
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }
    out
}

/// Direct getuid() syscall — replaces the Kotlin original's
/// `Runtime.exec(arrayOf("id", "-u"))` subprocess spawn. Cannot fail in
/// the way the subprocess version could (e.g. `id` binary missing from
/// PATH), so there's no -1 fallback case to preserve; getuid(2) always
/// succeeds.
fn getuid() -> u32 {
    unsafe { libc::getuid() }
}

/// 16 random bytes, hex-encoded — same shape as the Kotlin original's
/// `SecureRandom().nextBytes(16)` + hex-join. Uses /dev/urandom directly
/// (readily available on Android, no extra crate needed) rather than
/// pulling in a full CSPRNG dependency for one-time startup token
/// generation.
fn generate_session_token() -> String {
    use std::io::Read;
    let mut bytes = [0u8; 16];
    match std::fs::File::open("/dev/urandom") {
        Ok(mut f) => {
            let _ = f.read_exact(&mut bytes);
        }
        Err(_) => {
            // /dev/urandom is present on every real Android device; this
            // fallback only matters for hypothetical restricted
            // environments and intentionally trades cryptographic
            // strength for availability rather than crashing the server.
            let seed = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            for (i, b) in bytes.iter_mut().enumerate() {
                *b = ((seed >> (i % 8 * 8)) & 0xff) as u8;
            }
        }
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn writeln_flush(w: &mut impl Write, s: &str) -> std::io::Result<()> {
    writeln!(w, "{s}")?;
    w.flush()
}
