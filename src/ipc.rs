use std::io::Read;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::Duration;

pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
}

// --- IPC hardening constants -----------------------------------------------
//
// The IPC socket is reachable by anything that can open a path on the local
// filesystem (see `socket_path` below), which is a much bigger set of
// potential callers than "the child processes sc itself launched." The two
// constants and the `read_authenticated_message` function further down are
// what stand between "any process that can open this path" and "full control
// over this sc instance" — see the doc comment on `read_authenticated_message`
// for the full reasoning.

/// Hard ceiling on a single IPC message's size, in bytes. The underlying read
/// has no built-in limit, so without this a peer could stream an unbounded
/// amount of data into memory before we ever get a chance to parse (and
/// likely reject) it. Most legitimate actions — a handful of filenames, a
/// filter pattern, some command-line text — are a few dozen bytes. The one
/// documented use case that can legitimately get large is `TagOnly`/`Tag`
/// fed from stdin (see docs/IpcActions.md's `git diff --name-only | sc-action
/// ... TagOnly -` example): at roughly 40-60 bytes per path including the
/// newline, 8 MiB comfortably covers well over 100,000 changed files, which
/// is generous even for a large monorepo diff. If a payload somehow did
/// exceed the cap anyway, it's truncated rather than erroring, which will
/// typically just make `parse_message` fail (or, worse, silently succeed
/// with a partial list) rather than doing anything harmful — the cap exists
/// to bound memory, not to validate input, so callers of actions that could
/// plausibly produce a very large payload should keep it well under this.
const MAX_MESSAGE_BYTES: u64 = 8 * 1024 * 1024; // 8 MiB

/// Total wall-clock time we're willing to spend reading one peer's message,
/// from the moment we start reading. A legitimate `sc-action` client does a
/// single `write_all` and then exits (which closes its end of the socket and
/// delivers EOF) in well under a millisecond, so half a second is enormous
/// headroom for a same-host Unix socket under any realistic system load. Its
/// real purpose is to bound the alternative: without a deadline, a peer that
/// connects and simply never sends anything (or never closes) would block the
/// accepting thread forever. Both places that accept IPC connections do so on
/// their one and only thread — the main TUI render loop, and the Ctrl+O
/// subshell's PTY-shuffling loop — so "blocks forever" would mean "sc, or the
/// subshell session, freezes solid until the peer goes away."
///
/// This is deliberately enforced as a wall-clock deadline in
/// `read_authenticated_message`'s manual read loop, not as a naive
/// `set_read_timeout` on a single `read_to_string` call. `SO_RCVTIMEO` (what
/// `set_read_timeout` configures) resets on every individual `read()`
/// syscall rather than counting down cumulatively, so a peer trickling in a
/// few bytes at a time — each arriving just under the timeout — could keep a
/// naive implementation blocked far longer than this constant would suggest.
/// Recomputing the remaining budget before each `read()` closes that gap:
/// the total time spent in the read loop is capped at `IPC_READ_TIMEOUT`
/// regardless of how the peer paces its sends.
const IPC_READ_TIMEOUT: Duration = Duration::from_millis(500);

/// How many rejected or timed-out connections `IpcServer::try_recv` will
/// skip past, within a single call, while looking for one real message.
/// This exists in tension with `IPC_READ_TIMEOUT`: `try_recv` needs to look
/// past a bad connection rather than stopping at it (see the comment in
/// `try_recv` itself for why), but each connection it looks past can cost up
/// to `IPC_READ_TIMEOUT`. Without a cap here, a same-uid peer that floods the
/// accept backlog with connections that immediately stall (still possible
/// even after the SO_PEERCRED check — that check narrows the attacker to
/// "already running code as the same OS user," it doesn't rate-limit them)
/// could make a *single* `try_recv()` call block for
/// `IPC_READ_TIMEOUT * backlog_size`, which is far worse than the bound this
/// whole module exists to establish. Eight is generous headroom for the
/// realistic non-adversarial case (a stray probe or a slow legitimate sender
/// mixed in with real messages) while keeping the worst case for one call at
/// a known, small multiple of `IPC_READ_TIMEOUT` (currently 4 seconds) rather
/// than unbounded.
const MAX_BAD_CONNECTIONS_PER_CALL: u32 = 8;

#[derive(Debug)]
pub enum IpcMessage {
    Tag(Vec<String>),
    Untag(Vec<String>),
    TagOnly(Vec<String>),
    SelectGroup(String),
    UnselectGroup(String),
    InjectToCommandLine(String, CmdlineInjectMode),
    Filter(String),
    ToggleShell,
    RefreshPanel,
    ShowPanels(Option<String>),
}

#[derive(Debug, Clone, Copy)]
pub enum CmdlineInjectMode {
    Insert,
    Append,
    Replace,
}

impl IpcServer {
    pub fn new() -> Option<Self> {
        let pid = std::process::id();
        let path = socket_path(pid);
        let listener = UnixListener::bind(&path).ok()?;

        // `bind()` creates the socket's filesystem entry the same way `open()`
        // or `mkdir()` would: with mode `0o777 & !umask`. That is NOT private
        // by default — if sc happens to start under a permissive umask (0, or
        // 002, both genuinely used as defaults in some containers/CI images,
        // and occasionally set deliberately in a user's own shell for
        // shared-directory reasons), the socket file would be left group- or
        // world-writable. Unix domain sockets enforce the same permission bits
        // on connect() that regular files enforce on open(), so a writable
        // socket file means any other local user's process could connect and
        // drive this sc instance — including, if this happens to be a `sudo
        // sc`, at root's privilege level. `socket_path` below also means this
        // matters more than it might seem: whenever `$XDG_RUNTIME_DIR` isn't
        // set (which is the common case for a plain `sudo sc`, since sudo's
        // default `env_reset` strips it and root rarely has a `/run/user/0`
        // from a one-off `sudo` invocation), the socket falls back to a
        // world-traversable directory such as `/tmp`.
        //
        // We don't want the socket's privacy to be an accident of whatever
        // umask happened to be active, so we pin the mode to owner-only
        // explicitly, regardless of umask. This is defense in depth on top of
        // the SO_PEERCRED check in `read_authenticated_message` below, which
        // is the check that actually matters for security — this one just
        // makes sure the socket's on-disk permissions never end up more
        // permissive than intended. If we can't confirm the mode, we fail
        // closed (no IPC this session) rather than run with an unknown-privacy
        // socket.
        //
        // This has to be a path-based `chmod(2)` (`std::fs::set_permissions`)
        // rather than `fchmod` on the listener's own fd, even though `fchmod`
        // looks like the more obvious, TOCTOU-proof choice at first: on
        // Linux, `fchmod` on a bound AF_UNIX socket's fd does not change the
        // permission bits visible at its path, and does not error either —
        // it silently no-ops (verified empirically; this is a genuine, easy
        // trap, not a hypothetical one). Only `chmod(path)` actually reaches
        // the directory entry the kernel checks on `connect()`, so
        // path-based `set_permissions` is the only thing that works here,
        // despite re-resolving `path` and thus (in principle) being
        // susceptible to a TOCTOU race if something else could unlink and
        // replace whatever's at `path` between `bind()` and this call. In
        // practice that window isn't exploitable for the two directories
        // `socket_path` actually uses: `$XDG_RUNTIME_DIR` is mode 0700 and
        // owned by us, and the `/tmp` fallback has the sticky bit set, so in
        // both cases no other user can unlink our file out from under us to
        // begin with — the SO_PEERCRED check in `read_authenticated_message`
        // remains the real boundary regardless.
        use std::os::unix::fs::PermissionsExt;
        if std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).is_err() {
            let _ = std::fs::remove_file(&path);
            return None;
        }

        listener.set_nonblocking(true).ok()?;
        unsafe {
            std::env::set_var("SC_TOKEN", path.to_str().unwrap_or(""));
        }
        Some(IpcServer { listener, path })
    }

    pub fn raw_fd(&self) -> std::os::unix::io::RawFd {
        use std::os::unix::io::AsRawFd;
        self.listener.as_raw_fd()
    }

    pub fn try_recv(&self) -> Option<IpcMessage> {
        // A rejected (wrong uid) or timed-out connection must NOT make this
        // function return `None` the same way "the accept backlog is empty"
        // does: callers (see app.rs's post-command drain, which loops via
        // `std::iter::from_fn(|| ipc.try_recv())` until it sees a `None`)
        // treat `None` as "nothing left to process." If one bad connection
        // could produce that same `None`, it would prematurely end the drain
        // and strand any legitimate messages still queued behind it in the
        // backlog. So we skip past bad connections internally and keep
        // trying, and only bottom out via `accept().ok()?` once the backlog
        // is genuinely empty.
        //
        // That skipping is itself bounded (`MAX_BAD_CONNECTIONS_PER_CALL`)
        // rather than unconditional, so a same-uid peer that floods the
        // backlog with stalling connections can't turn a single `try_recv()`
        // call into an unbounded block by chaining `IPC_READ_TIMEOUT`s
        // end-to-end — see that constant's doc comment for why "same-uid
        // peer floods connections" is a real, if narrower, residual risk
        // this bound is meant to contain rather than fully eliminate.
        for _ in 0..MAX_BAD_CONNECTIONS_PER_CALL {
            let (stream, _) = self.listener.accept().ok()?;
            if let Some(buf) = read_authenticated_message(stream) {
                return parse_message(&buf);
            }
        }
        None
    }
}

impl Drop for IpcServer {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn socket_path(pid: u32) -> PathBuf {
    let name = format!("sc-{pid}.sock");
    if let Ok(run) = std::env::var("XDG_RUNTIME_DIR") {
        return PathBuf::from(run).join(name);
    }
    std::env::temp_dir().join(name)
}

/// Validates an already-`accept()`ed connection and reads its raw message
/// payload. This is the single choke point both IPC read paths funnel
/// through — `IpcServer::try_recv` above (used from the main TUI loop) and
/// `subshell::ipc_accept_message` (used from the Ctrl+O passthrough loop) —
/// so the hardening described below only has to be written, and reasoned
/// about, once.
///
/// Returns `None` if the peer isn't us, if the read failed or timed out, or
/// (implicitly, via `parse_message` at the call site) if the payload wasn't
/// something we understand. In every case the caller just drops the message
/// and moves on — same "fail silently" philosophy the `sc-action` client uses
/// (see its own doc comment), since there's no good place to surface an error
/// for a background IPC listener.
///
/// Three things happen here, in order:
///
/// 1. **Peer identity check via `SO_PEERCRED`.** We reject any connection
///    that isn't from a process running as our own Unix user. This is the
///    *actual* trust boundary for the whole IPC mechanism. `$SC_TOKEN` (the
///    socket path) is meant to be handed only to our own child processes via
///    environment inheritance, but the socket file itself lives at a
///    predictable, sometimes world-traversable path — see the comment in
///    `IpcServer::new` about the `/tmp` fallback. Filesystem permissions on
///    that path are a second line of defense, but they depend on whatever
///    umask happened to be active when the socket was created, which isn't
///    something callers control or should have to trust blindly. Checking the
///    kernel's own record of the connecting process's credentials (via
///    `getsockopt(SO_PEERCRED)`, see `peer_uid` below) holds regardless of
///    umask, regardless of where the socket file ends up, and regardless of
///    its file mode — it's what actually closes the gap where, say, `sc` is
///    running as root under `sudo` and an unrelated, unprivileged local
///    process tries to connect and drive it.
///
/// 2. **Wall-clock read deadline.** See `IPC_READ_TIMEOUT`'s doc comment:
///    bounds the *total* time a stalled, silent, or slow-trickling peer can
///    block the calling thread — not just the time between individual reads.
///
/// 3. **Size cap.** See `MAX_MESSAGE_BYTES`'s doc comment: bounds how much
///    memory a single message can consume.
pub(crate) fn read_authenticated_message(mut stream: UnixStream) -> Option<String> {
    // SAFETY: getuid(2) has no failure mode — it always succeeds and simply
    // returns the calling process's real user ID. This asks "who are we?" so
    // we can compare it below against "who is the peer?"
    let our_uid = unsafe { libc::getuid() };

    if peer_uid(&stream)? != our_uid {
        return None;
    }

    // The listener is non-blocking so `accept()` never stalls the event loop,
    // but accepted connections don't reliably inherit that flag, so we set it
    // explicitly here before we start reading below.
    stream.set_nonblocking(false).ok()?;

    // Read in a loop, bounded by both a total-size cap and a wall-clock
    // deadline, rather than a single `read_to_string` call. This is *not*
    // just `stream.set_read_timeout(...)` followed by `read_to_string`: that
    // would set `SO_RCVTIMEO`, which the kernel resets on every individual
    // `read()` syscall rather than treating as a cumulative budget. A peer
    // that trickles in a few bytes at a time, each arriving just under the
    // timeout, would keep that naive version blocked far longer than
    // `IPC_READ_TIMEOUT` — silently reintroducing the same "freezes the UI"
    // failure mode this whole function exists to prevent, just stretched out
    // instead of eliminated. Recomputing the remaining time budget before
    // every `read()` call here closes that gap: no matter how the peer paces
    // its sends, the total time spent in this loop cannot exceed
    // `IPC_READ_TIMEOUT`.
    let deadline = std::time::Instant::now() + IPC_READ_TIMEOUT;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        stream.set_read_timeout(Some(remaining)).ok()?;

        match stream.read(&mut chunk) {
            Ok(0) => break, // peer closed its end: message complete
            Ok(n) => {
                bytes.extend_from_slice(&chunk[..n]);
                if bytes.len() as u64 >= MAX_MESSAGE_BYTES {
                    break;
                }
            }
            // WouldBlock/TimedOut here means our own deadline elapsed
            // mid-read; treat that the same as "ran out of time" rather than
            // as a hard failure, so a peer that sent a complete, small
            // message right up against the deadline still gets processed
            // with whatever arrived instead of being discarded outright.
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => break,
            Err(_) => return None,
        }
    }

    // `MAX_MESSAGE_BYTES` is meant as a hard ceiling, but the loop above can
    // overshoot it by up to one 4 KiB chunk before noticing; trim back to
    // the exact cap so callers never see more than they were promised.
    bytes.truncate(MAX_MESSAGE_BYTES as usize);
    String::from_utf8(bytes).ok()
}

/// Asks the kernel for the real user ID of whoever is on the other end of
/// `stream`, via `getsockopt(SO_PEERCRED)`.
///
/// `std::os::unix::net::UnixStream::peer_cred` would do exactly this safely,
/// but it's still gated behind the unstable `peer_credentials_unix_socket`
/// feature on this toolchain, so we go straight to the syscall it wraps. This
/// mirrors how the rest of the codebase already talks to the kernel directly
/// via `libc` for things `std` doesn't expose (see subshell.rs's use of
/// `libc::poll`/`libc::read`/`libc::write` for the PTY passthrough loop) —
/// `SO_PEERCRED` is Linux-specific, which is fine here since this project
/// already assumes Linux elsewhere (e.g. reading `/proc/<pid>/cwd`).
///
/// Returns `None` if the kernel call itself fails, which in practice should
/// only happen if `stream` somehow isn't a genuine, still-open Unix domain
/// socket — a case we'd want to treat as "untrusted" anyway.
#[cfg(target_os = "linux")]
fn peer_uid(stream: &UnixStream) -> Option<libc::uid_t> {
    use std::os::unix::io::AsRawFd;

    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;

    // SAFETY: `cred` and `len` are a correctly-sized, correctly-typed
    // out-parameter pair for SO_PEERCRED as documented in unix(7), and
    // `stream.as_raw_fd()` is a live fd owned by `stream` for the duration of
    // this call (we only borrow it, we don't take ownership away from
    // `stream`).
    let ret = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };

    if ret == 0 { Some(cred.uid) } else { None }
}

/// macOS equivalent of the Linux `peer_uid` above: there's no `SO_PEERCRED`,
/// the analogous call is `getsockopt(LOCAL_PEERCRED)` at the `SOL_LOCAL`
/// level, filling in an `xucred` rather than a `ucred`.
#[cfg(target_os = "macos")]
fn peer_uid(stream: &UnixStream) -> Option<libc::uid_t> {
    use std::os::unix::io::AsRawFd;

    let mut cred: libc::xucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::xucred>() as libc::socklen_t;

    // SAFETY: same reasoning as the Linux version above, using the macOS
    // LOCAL_PEERCRED/xucred pair instead of SO_PEERCRED/ucred.
    let ret = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_LOCAL,
            libc::LOCAL_PEERCRED,
            &mut cred as *mut libc::xucred as *mut libc::c_void,
            &mut len,
        )
    };

    if ret == 0 { Some(cred.cr_uid) } else { None }
}

pub(crate) fn parse_message(raw: &str) -> Option<IpcMessage> {
    let mut lines = raw.lines();
    let action = lines.next()?.trim();
    let args: Vec<String> = lines.map(|l| l.trim().to_string()).filter(|s| !s.is_empty()).collect();

    match action {
        "Tag"                 => Some(IpcMessage::Tag(args)),
        "Untag"               => Some(IpcMessage::Untag(args)),
        "TagOnly"             => Some(IpcMessage::TagOnly(args)),
        "SelectGroup"         => Some(IpcMessage::SelectGroup(args.into_iter().next()?)),
        "UnselectGroup"       => Some(IpcMessage::UnselectGroup(args.into_iter().next()?)),
        "InjectToCommandLine" => Some(match args.split_first() {
            Some((m, rest)) if m == "Insert"  => IpcMessage::InjectToCommandLine(rest.join(" "), CmdlineInjectMode::Insert),
            Some((m, rest)) if m == "Append"  => IpcMessage::InjectToCommandLine(rest.join(" "), CmdlineInjectMode::Append),
            Some((m, rest)) if m == "Replace" => IpcMessage::InjectToCommandLine(rest.join(" "), CmdlineInjectMode::Replace),
            _                                  => IpcMessage::InjectToCommandLine(args.join(" "), CmdlineInjectMode::Insert),
        }),
        "Filter"              => Some(IpcMessage::Filter(args.into_iter().next().unwrap_or_default())),
        "ToggleShell"         => Some(IpcMessage::ToggleShell),
        "RefreshPanel"        => Some(IpcMessage::RefreshPanel),
        "ShowPanels"          => Some(IpcMessage::ShowPanels(args.into_iter().next())),
        _                     => None,
    }
}
