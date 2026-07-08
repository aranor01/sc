use anyhow::{bail, Result};
use std::io;
use std::os::unix::io::RawFd;

pub struct Subshell {
    pub master_fd: RawFd,
    pub child_pid: libc::pid_t,
    slave_name: String,
}

impl Subshell {
    /// Spawn a subshell attached to a new PTY.
    pub fn spawn() -> Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        unsafe {
            let mut master_fd: RawFd = -1;
            let mut slave_fd: RawFd = -1;
            let mut name = [0u8; 256];
            let mut ws: libc::winsize = std::mem::zeroed();
            libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws);
            let ret = libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                name.as_mut_ptr() as *mut libc::c_char,
                std::ptr::null_mut(),
                if ws.ws_col > 0 { &ws } else { std::ptr::null() },
            );
            if ret != 0 {
                bail!("openpty failed: {}", io::Error::last_os_error());
            }

            let child_pid = libc::fork();
            if child_pid < 0 {
                libc::close(slave_fd);
                libc::close(master_fd);
                bail!("fork failed: {}", io::Error::last_os_error());
            }

            if child_pid == 0 {
                // Child: set up the PTY as controlling terminal
                libc::close(master_fd);
                libc::setsid();
                libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
                libc::dup2(slave_fd, libc::STDIN_FILENO);
                libc::dup2(slave_fd, libc::STDOUT_FILENO);
                libc::dup2(slave_fd, libc::STDERR_FILENO);
                if slave_fd > 2 { libc::close(slave_fd); }

                // exec shell with the history settings we need
                let shell_c = std::ffi::CString::new(shell).unwrap();
                let histcontrol_cstr = std::ffi::CString::new("HISTCONTROL=ignorespace").unwrap();
                let env_c = std::ffi::CString::new("env").unwrap();
                let argv_env: Vec<*const libc::c_char> = vec![
                    env_c.as_ptr(),
                    histcontrol_cstr.as_ptr(),
                    shell_c.as_ptr(),
                    std::ptr::null(),
                ];
                libc::execvp(env_c.as_ptr(), argv_env.as_ptr());
                // If exec fails, exit child
                libc::_exit(1);
            }

            // Parent: get slave device name for set_echo before closing the fd.
            // ptsname only needs master_fd — slave_fd can already be closed.
            let slave_name = {
                let ptr = libc::ptsname(master_fd);
                if ptr.is_null() { String::new() }
                else { std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string() }
            };
            libc::close(slave_fd);

            // Wait a moment for the shell to start and emit its first prompt
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = drain_fd(master_fd);

            Ok(Subshell { master_fd, child_pid, slave_name })
        }
    }

    /// Enter a raw passthrough loop: forward stdin↔PTY master until
    /// the user presses Ctrl+O or the shell exits.
    ///
    /// This call blocks until passthrough is exited.
    pub fn start_passthrough(&self, ipc_fd: Option<RawFd>) -> Result<Vec<String>> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

        enable_raw_mode()?;

        let master = self.master_fd;
        let result = passthrough_loop(master, ipc_fd);

        disable_raw_mode()?;
        result
    }

    /// Send a line to the PTY. Use this before entering passthrough for
    /// interactive commands.
    pub fn send_line(&self, line: &str) -> Result<()> {
        write_fd(self.master_fd, format!("{line}\n").as_bytes())
    }

    pub fn send_raw(&self, bytes: &[u8]) {
        let _ = write_fd(self.master_fd, bytes);
    }

    /// Control the ECHO flag on the PTY slave's line discipline.
    /// Opens the slave device transiently so tcsetattr targets the correct termios.
    pub fn set_echo(&self, on: bool) {
        if self.slave_name.is_empty() { return; }
        let Ok(name) = std::ffi::CString::new(self.slave_name.as_str()) else { return; };
        unsafe {
            let fd = libc::open(name.as_ptr(), libc::O_RDWR | libc::O_NOCTTY);
            if fd < 0 { return; }
            let mut t: libc::termios = std::mem::zeroed();
            libc::tcgetattr(fd, &mut t);
            if on { t.c_lflag |= libc::ECHO; } else { t.c_lflag &= !libc::ECHO; }
            libc::tcsetattr(fd, libc::TCSANOW, &t);
            libc::close(fd);
        }
    }

    /// Non-blocking drain of any buffered output on the PTY master.
    /// Used to discard accumulated readline echoes before entering passthrough.
    pub fn drain(&self) {
        drain_fd(self.master_fd);
    }

    /// Update the PTY window size and notify the child so fullscreen programs
    /// (vim, mc, etc.) re-query their terminal dimensions.
    pub fn resize(&self, cols: u16, rows: u16) {
        unsafe {
            let ws = libc::winsize { ws_col: cols, ws_row: rows, ws_xpixel: 0, ws_ypixel: 0 };
            libc::ioctl(self.master_fd, libc::TIOCSWINSZ, &ws);
            libc::kill(self.child_pid, libc::SIGWINCH);
        }
    }

    /// Check if the child process is still alive.
    pub fn is_alive(&self) -> bool {
        unsafe {
            let mut status = 0;
            let r = libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);
            // r == 0  → still running
            // r > 0   → just exited (we consumed the exit status)
            // r < 0   → ECHILD: already reaped elsewhere, i.e. also dead
            r == 0
        }
    }
}

impl Drop for Subshell {
    fn drop(&mut self) {
        unsafe {
            libc::kill(self.child_pid, libc::SIGTERM);
            libc::close(self.master_fd);
        }
    }
}

// ── PTY capture (stateless command execution) ─────────────────────────────────

/// Run `cmd` (via `sh -c`) in a fresh PTY, forwarding the user's terminal
/// bidirectionally so both interactive and non-interactive programs work.
/// Returns all raw bytes that came out of the PTY (may include ANSI sequences).
/// The caller must have already left the alternate screen and disabled raw mode.
pub fn run_with_pty_capture(cmd: &str, cwd: &str) -> Vec<u8> {
    use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

    unsafe {
        let mut master_fd: RawFd = -1;
        let mut slave_fd: RawFd = -1;

        // Inherit the current terminal dimensions for the new PTY.
        let mut ws: libc::winsize = std::mem::zeroed();
        libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws);

        let ret = libc::openpty(
            &mut master_fd,
            &mut slave_fd,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            if ws.ws_col > 0 { &ws } else { std::ptr::null() },
        );
        if ret != 0 {
            return Vec::new();
        }

        let child_pid = libc::fork();
        if child_pid < 0 {
            libc::close(slave_fd);
            libc::close(master_fd);
            return Vec::new();
        }

        if child_pid == 0 {
            // Child: set the PTY slave as controlling terminal, then exec.
            libc::close(master_fd);
            libc::setsid();
            libc::ioctl(slave_fd, libc::TIOCSCTTY, 0);
            libc::dup2(slave_fd, libc::STDIN_FILENO);
            libc::dup2(slave_fd, libc::STDOUT_FILENO);
            libc::dup2(slave_fd, libc::STDERR_FILENO);
            if slave_fd > 2 {
                libc::close(slave_fd);
            }
            if let Ok(c) = std::ffi::CString::new(cwd) {
                libc::chdir(c.as_ptr());
            }
            if let (Ok(sh), Ok(flag), Ok(cmd_c)) = (
                std::ffi::CString::new("sh"),
                std::ffi::CString::new("-c"),
                std::ffi::CString::new(cmd),
            ) {
                let argv: [*const libc::c_char; 4] =
                    [sh.as_ptr(), flag.as_ptr(), cmd_c.as_ptr(), std::ptr::null()];
                libc::execvp(sh.as_ptr(), argv.as_ptr());
            }
            libc::_exit(1);
        }

        // Parent: close slave, then run passthrough + capture.
        libc::close(slave_fd);
        let _ = enable_raw_mode();

        let mut capture: Vec<u8> = Vec::new();
        let mut buf = [0u8; 4096];

        loop {
            let mut fds = [
                libc::pollfd { fd: libc::STDIN_FILENO, events: libc::POLLIN, revents: 0 },
                libc::pollfd { fd: master_fd,          events: libc::POLLIN, revents: 0 },
            ];
            if libc::poll(fds.as_mut_ptr(), 2, -1) < 0 {
                break;
            }

            if fds[0].revents & libc::POLLIN != 0 {
                let n = libc::read(libc::STDIN_FILENO, buf.as_mut_ptr() as *mut _, buf.len());
                if n <= 0 { break; }
                libc::write(master_fd, buf.as_ptr() as *const _, n as usize);
            }

            if fds[1].revents & libc::POLLIN != 0 {
                let n = libc::read(master_fd, buf.as_mut_ptr() as *mut _, buf.len());
                if n <= 0 { break; } // EIO when child exits and slave closes
                libc::write(libc::STDOUT_FILENO, buf.as_ptr() as *const _, n as usize);
                capture.extend_from_slice(&buf[..n as usize]);
            } else if fds[1].revents & (libc::POLLHUP | libc::POLLERR) != 0 {
                break;
            }
        }

        let _ = disable_raw_mode();
        libc::waitpid(child_pid, std::ptr::null_mut(), 0);
        libc::close(master_fd);

        capture
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn write_fd(fd: RawFd, data: &[u8]) -> Result<()> {
    let mut written = 0;
    while written < data.len() {
        let n = unsafe { libc::write(fd, data[written..].as_ptr() as *const _, data.len() - written) };
        if n < 0 {
            bail!("write to PTY: {}", io::Error::last_os_error());
        }
        written += n as usize;
    }
    Ok(())
}

/// Non-blocking drain of whatever is in the fd buffer. Returns discarded bytes.
fn drain_fd(fd: RawFd) -> Vec<u8> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) };
    let mut buf = [0u8; 4096];
    let mut out = Vec::new();
    loop {
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n <= 0 { break; }
        out.extend_from_slice(&buf[..n as usize]);
    }
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    out
}

/// Passthrough loop: copies stdin→master and master→stdout until Ctrl+O, EOF,
/// or a ShowPanels IPC message arrives on `ipc_fd`. Any other IPC message received
/// while looping is queued and returned so the caller can process it once passthrough
/// exits, instead of being silently discarded.
fn passthrough_loop(master: RawFd, ipc_fd: Option<RawFd>) -> Result<Vec<String>> {
    let stdin_fd = libc::STDIN_FILENO;
    let stdout_fd = libc::STDOUT_FILENO;
    let mut buf = [0u8; 4096];
    let mut pending: Vec<String> = Vec::new();

    loop {
        let mut fds = [
            libc::pollfd { fd: stdin_fd,                 events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: master,                   events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: ipc_fd.unwrap_or(-1),     events: libc::POLLIN, revents: 0 },
        ];
        let nfds = if ipc_fd.is_some() { 3 } else { 2 };
        let r = unsafe { libc::poll(fds.as_mut_ptr(), nfds, -1) };
        if r < 0 { break; }

        if fds[0].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let data = &buf[..n as usize];
            if data.contains(&0x0F) { break; } // Ctrl+O
            let _ = unsafe { libc::write(master, data.as_ptr() as *const _, data.len()) };
        }

        if fds[1].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(master, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let data = &buf[..n as usize];
            let _ = unsafe { libc::write(stdout_fd, data.as_ptr() as *const _, data.len()) };
        }

        if fds[2].revents & libc::POLLIN != 0 {
            if let Some(fd) = ipc_fd {
                if let Some(raw) = ipc_accept_message(fd) {
                    if raw.lines().next().map(|l| l.trim()) == Some("ShowPanels") {
                        break;
                    }
                    pending.push(raw);
                }
            }
        }
    }
    Ok(pending)
}

/// Non-blocking accept on the IPC listener fd. Returns the raw message payload if a
/// connection was accepted and passed authentication, or `None` if there was nothing
/// to accept, the peer wasn't us, or the read failed/timed out.
///
/// All the actual hardening (peer-credential check, read timeout, size cap) lives in
/// `crate::ipc::read_authenticated_message`, shared with the main TUI loop's IPC path
/// so it's implemented — and reasoned about — in exactly one place. See that
/// function's doc comment for the full explanation of what it protects against.
pub fn ipc_accept_message(listener_fd: RawFd) -> Option<String> {
    use std::os::unix::net::UnixListener;
    use std::os::unix::io::FromRawFd;

    // Safety: we borrow the fd temporarily; ManuallyDrop prevents double-close.
    let listener = std::mem::ManuallyDrop::new(unsafe { UnixListener::from_raw_fd(listener_fd) });
    let _ = listener.set_nonblocking(true);
    let (stream, _) = listener.accept().ok()?;
    crate::ipc::read_authenticated_message(stream)
}
