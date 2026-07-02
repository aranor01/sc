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

/// Hard ceiling on a single IPC message's size, in bytes. `read_to_string` has
/// no built-in limit, so without this a peer could stream an unbounded amount
/// of data into a `String` before we ever get a chance to parse (and likely
/// reject) it, growing our memory usage without bound. No legitimate action —
/// a handful of filenames, a filter pattern, some command-line text — comes
/// anywhere close to this size. If a real payload somehow did exceed it, it
/// would simply be truncated, which will typically just make `parse_message`
/// fail rather than doing anything harmful.
const MAX_MESSAGE_BYTES: u64 = 1024 * 1024; // 1 MiB

/// How long we're willing to block waiting for a connected peer to finish
/// sending its message. A legitimate `sc-action` client does a single
/// `write_all` and then exits (which closes its end of the socket and
/// delivers EOF) in well under a millisecond, so half a second is enormous
/// headroom for a same-host Unix socket under any realistic system load. Its
/// real purpose is to bound the alternative: without a read timeout, a peer
/// that connects and simply never sends anything (or never closes) would
/// block the accepting thread's `read_to_string` forever. Both places that
/// accept IPC connections do so on their one and only thread — the main TUI
/// render loop, and the Ctrl+O subshell's PTY-shuffling loop — so "blocks
/// forever" would mean "sc, or the subshell session, freezes solid until the
/// peer goes away." A bounded timeout turns that into "this one message is
/// dropped after at most half a second," which is a much smaller blast radius
/// for the same trigger (just opening a connection and doing nothing).
const IPC_READ_TIMEOUT: Duration = Duration::from_millis(500);

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
        let (stream, _) = self.listener.accept().ok()?;
        let buf = read_authenticated_message(stream)?;
        parse_message(&buf)
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
/// 2. **Read timeout.** See `IPC_READ_TIMEOUT`'s doc comment: bounds how long
///    a stalled or silent peer can block the calling thread.
///
/// 3. **Size cap.** See `MAX_MESSAGE_BYTES`'s doc comment: bounds how much
///    memory a single message can consume.
pub(crate) fn read_authenticated_message(stream: UnixStream) -> Option<String> {
    // SAFETY: getuid(2) has no failure mode — it always succeeds and simply
    // returns the calling process's real user ID. This asks "who are we?" so
    // we can compare it below against "who is the peer?"
    let our_uid = unsafe { libc::getuid() };

    if peer_uid(&stream)? != our_uid {
        return None;
    }

    // The listener is non-blocking so `accept()` never stalls the event loop,
    // but accepted connections don't reliably inherit that flag, so we set it
    // explicitly here before arming the read timeout below (a timeout on a
    // socket already in non-blocking mode wouldn't do anything useful, since
    // reads would already return immediately with WouldBlock).
    stream.set_nonblocking(false).ok()?;
    stream.set_read_timeout(Some(IPC_READ_TIMEOUT)).ok()?;

    // `Read::take` caps how many bytes `read_to_string` is willing to pull
    // off the stream; it stops yielding bytes at the limit rather than
    // erroring, so an oversized payload is silently truncated instead of
    // exhausting memory.
    let mut buf = String::new();
    stream.take(MAX_MESSAGE_BYTES).read_to_string(&mut buf).ok()?;
    Some(buf)
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
