use anyhow::{bail, Result};
use std::io;
use std::os::unix::io::RawFd;

const SENTINEL: &str = "__SC_PROMPT_SENTINEL__";
const CTRL_O_FD: RawFd = 10; // fd passed to bash for Ctrl-O pipe signaling

pub struct Subshell {
    pub master_fd: RawFd,
    pub child_pid: libc::pid_t,
    slave_name: String,
    ctrl_o_pipe_read: RawFd, // -1 if not bash
    rl_file: String,         // empty if not bash
}

impl Subshell {
    /// Spawn a subshell attached to a new PTY. The subshell is configured with
    /// a sentinel PS1 so we can detect the prompt in output.
    ///
    /// For bash: also sets up bidirectional readline sync via a pipe and an
    /// `--init-file` that installs a Ctrl-O readline binding.
    pub fn spawn() -> Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let is_bash = std::path::Path::new(&shell)
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n == "bash")
            .unwrap_or(false);

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

            // Bash readline sync: create a pipe and init file before fork so both
            // ends are available in child and parent.
            let mut ctrl_o_pipe_read: RawFd = -1;
            let mut ctrl_o_pipe_write: RawFd = -1;
            let mut rl_file = String::new();

            if is_bash {
                let mut pfd = [-1i32; 2];
                if libc::pipe(pfd.as_mut_ptr()) == 0 {
                    ctrl_o_pipe_read = pfd[0];
                    ctrl_o_pipe_write = pfd[1];
                    rl_file = format!("/tmp/sc_rl_{}", master_fd);
                    let init_path = format!("/tmp/sc_init_{}", master_fd);
                    // The init file sources ~/.bashrc (preserving the user's PS1),
                    // appends a PROMPT_COMMAND entry that emits the sentinel inside a
                    // DCS escape (\033P...\033\) — invisible to the terminal but
                    // detectable by sc's byte-stream search — and installs the Ctrl-O
                    // binding. CTRL_O_FD is inherited by bash as fd 10; the binding
                    // writes one byte there to wake the passthrough_loop poll().
                    let init_content = format!(
                        "[ -f ~/.bashrc ] && source ~/.bashrc\n\
                         _sc_send_sentinel() {{\n\
                             printf '\\033P{sentinel}\\033\\\\'\n\
                         }}\n\
                         PROMPT_COMMAND=\"${{PROMPT_COMMAND:+${{PROMPT_COMMAND}}; }}_sc_send_sentinel\"\n\
                         _sc_ctrl_o() {{\n\
                             printf '%s' \"$READLINE_LINE\" > '{rl}'\n\
                             printf 'x' >&{fd}\n\
                         }}\n\
                         bind -x '\"\\C-o\": _sc_ctrl_o'\n",
                        sentinel = SENTINEL,
                        rl = rl_file,
                        fd = CTRL_O_FD,
                    );
                    if std::fs::write(&init_path, &init_content).is_err() {
                        libc::close(ctrl_o_pipe_read);
                        libc::close(ctrl_o_pipe_write);
                        ctrl_o_pipe_read = -1;
                        ctrl_o_pipe_write = -1;
                        rl_file = String::new();
                    }
                }
            }

            let child_pid = libc::fork();
            if child_pid < 0 {
                libc::close(slave_fd);
                libc::close(master_fd);
                if ctrl_o_pipe_read >= 0 { libc::close(ctrl_o_pipe_read); }
                if ctrl_o_pipe_write >= 0 { libc::close(ctrl_o_pipe_write); }
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

                let ps1_env = format!("PS1={SENTINEL} $ ");
                let shell_c = std::ffi::CString::new(shell).unwrap();
                let ps1_cstr = std::ffi::CString::new(ps1_env).unwrap();
                let histcontrol_cstr = std::ffi::CString::new("HISTCONTROL=ignorespace").unwrap();
                let env_c = std::ffi::CString::new("env").unwrap();

                if ctrl_o_pipe_write >= 0 {
                    // Bash mode: move pipe_write to CTRL_O_FD, close the read end.
                    if ctrl_o_pipe_write != CTRL_O_FD {
                        libc::dup2(ctrl_o_pipe_write, CTRL_O_FD);
                        libc::close(ctrl_o_pipe_write);
                    }
                    libc::close(ctrl_o_pipe_read);

                    let init_path = format!("/tmp/sc_init_{}", master_fd);
                    let init_c = std::ffi::CString::new(init_path).unwrap();
                    let flag_c = std::ffi::CString::new("--init-file").unwrap();
                    let argv: Vec<*const libc::c_char> = vec![
                        env_c.as_ptr(),
                        ps1_cstr.as_ptr(),
                        histcontrol_cstr.as_ptr(),
                        shell_c.as_ptr(),
                        flag_c.as_ptr(),
                        init_c.as_ptr(),
                        std::ptr::null(),
                    ];
                    libc::execvp(env_c.as_ptr(), argv.as_ptr());
                } else {
                    // Non-bash mode: existing behaviour
                    if ctrl_o_pipe_read >= 0 { libc::close(ctrl_o_pipe_read); }
                    let argv: Vec<*const libc::c_char> = vec![
                        env_c.as_ptr(),
                        ps1_cstr.as_ptr(),
                        histcontrol_cstr.as_ptr(),
                        shell_c.as_ptr(),
                        std::ptr::null(),
                    ];
                    libc::execvp(env_c.as_ptr(), argv.as_ptr());
                }
                libc::_exit(1);
            }

            // Parent: close the write end (bash owns it via CTRL_O_FD).
            if ctrl_o_pipe_write >= 0 {
                libc::close(ctrl_o_pipe_write);
            }

            // Parent: get slave device name for set_echo before closing the fd.
            let slave_name = {
                let ptr = libc::ptsname(master_fd);
                if ptr.is_null() { String::new() }
                else { std::ffi::CStr::from_ptr(ptr).to_str().unwrap_or("").to_string() }
            };
            libc::close(slave_fd);

            // Wait a moment for the shell to start and emit its first prompt
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = drain_fd(master_fd);

            Ok(Subshell { master_fd, child_pid, slave_name, ctrl_o_pipe_read, rl_file })
        }
    }

    /// Send a shell command (with newline) to the subshell and collect output
    /// until the next sentinel prompt appears.
    pub fn run_command(&self, cmd: &str) -> Result<Vec<u8>> {
        let line = format!("{cmd}\n");
        write_fd(self.master_fd, line.as_bytes())?;
        read_until_sentinel(self.master_fd)
    }

    /// Enter a raw passthrough loop: forward stdin↔PTY master until
    /// the user presses Ctrl+O or the shell exits.
    ///
    /// For bash (ctrl_o_pipe_read >= 0): exits when bash's Ctrl-O binding
    /// signals via the pipe. Call `clear_rl_file()` before and `take_rl_line()`
    /// after to read back the readline content.
    ///
    /// This call blocks until passthrough is exited.
    pub fn start_passthrough(&self, ipc_fd: Option<RawFd>) -> Result<()> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

        enable_raw_mode()?;

        let ctrl_o_pipe = if self.ctrl_o_pipe_read >= 0 {
            Some(self.ctrl_o_pipe_read)
        } else {
            None
        };
        let result = passthrough_loop(self.master_fd, ipc_fd, ctrl_o_pipe);

        disable_raw_mode()?;
        result
    }

    /// Send a line to the PTY without waiting for the sentinel prompt.
    /// Use this before entering passthrough for interactive commands.
    pub fn send_line(&self, line: &str) -> Result<()> {
        write_fd(self.master_fd, format!("{line}\n").as_bytes())
    }

    pub fn send_raw(&self, bytes: &[u8]) {
        let _ = write_fd(self.master_fd, bytes);
    }

    /// Remove the readline sync file so that a subsequent `take_rl_line()` only
    /// returns content written during the current passthrough session (not a
    /// leftover from a previous one).
    pub fn clear_rl_file(&self) {
        if !self.rl_file.is_empty() {
            let _ = std::fs::remove_file(&self.rl_file);
        }
    }

    /// Read the readline content written by bash's Ctrl-O binding and remove
    /// the file. Returns `None` for non-bash or when the file was not written
    /// (passthrough exited via IPC/EOF rather than Ctrl-O).
    pub fn take_rl_line(&self) -> Option<String> {
        if self.rl_file.is_empty() { return None; }
        if !std::path::Path::new(&self.rl_file).exists() { return None; }
        let content = std::fs::read_to_string(&self.rl_file).unwrap_or_default();
        let _ = std::fs::remove_file(&self.rl_file);
        Some(content)
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

    /// Like start_passthrough, but exits automatically when the sentinel
    /// prompt is detected (command finished), as well as on Ctrl-O or EOF.
    /// If `ipc_fd` is provided, also exits when a ShowPanels IPC message arrives.
    pub fn start_passthrough_until_sentinel(&self, ipc_fd: Option<RawFd>) -> Result<()> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
        enable_raw_mode()?;
        let result = passthrough_loop_until_sentinel(self.master_fd, ipc_fd);
        disable_raw_mode()?;
        result
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
        if self.ctrl_o_pipe_read >= 0 {
            unsafe { libc::close(self.ctrl_o_pipe_read); }
            let _ = std::fs::remove_file(&self.rl_file);
            // Derive the init file path from the rl file path
            if let Some(suffix) = self.rl_file.strip_prefix("/tmp/sc_rl_") {
                let _ = std::fs::remove_file(format!("/tmp/sc_init_{}", suffix));
            }
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

fn read_until_sentinel(fd: RawFd) -> Result<Vec<u8>> {
    let mut output = Vec::new();
    let mut buf = [0u8; 256];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        // Poll with a small timeout
        let mut fds = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
        let timeout_ms = deadline.saturating_duration_since(std::time::Instant::now())
            .as_millis()
            .min(200) as libc::c_int;
        let r = unsafe { libc::poll(&mut fds, 1, timeout_ms) };
        if r < 0 { bail!("poll error: {}", io::Error::last_os_error()); }
        if r == 0 {
            if std::time::Instant::now() >= deadline {
                break; // timed out
            }
            continue;
        }
        let n = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut _, buf.len()) };
        if n < 0 { bail!("read error: {}", io::Error::last_os_error()); }
        if n == 0 { break; }
        output.extend_from_slice(&buf[..n as usize]);
        // Check if we've seen the sentinel
        if output.windows(SENTINEL.len()).any(|w| w == SENTINEL.as_bytes()) {
            // Strip everything from the sentinel line onwards
            if let Some(pos) = output.windows(SENTINEL.len()).position(|w| w == SENTINEL.as_bytes()) {
                // Find the start of the sentinel line
                let line_start = output[..pos].iter().rposition(|&b| b == b'\n').map(|p| p + 1).unwrap_or(0);
                output.truncate(line_start);
            }
            break;
        }
    }
    Ok(output)
}

/// Passthrough loop that exits automatically when the sentinel prompt appears
/// (command finished), on Ctrl+O / EOF, or when a ShowPanels IPC message arrives.
fn passthrough_loop_until_sentinel(master: RawFd, ipc_fd: Option<RawFd>) -> Result<()> {
    let stdin_fd  = libc::STDIN_FILENO;
    let stdout_fd = libc::STDOUT_FILENO;
    let mut buf  = [0u8; 4096];
    let mut tail: Vec<u8> = Vec::new();

    loop {
        let mut fds = [
            libc::pollfd { fd: stdin_fd,                    events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: master,                      events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: ipc_fd.unwrap_or(-1),        events: libc::POLLIN, revents: 0 },
        ];
        let nfds = if ipc_fd.is_some() { 3 } else { 2 };
        if unsafe { libc::poll(fds.as_mut_ptr(), nfds, -1) } < 0 { break; }

        if fds[0].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let data = &buf[..n as usize];
            if data.contains(&0x0F) { break; } // Ctrl-O: manual exit
            unsafe { libc::write(master, data.as_ptr() as *const _, data.len()); }
        }

        if fds[1].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(master, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let data = &buf[..n as usize];
            unsafe { libc::write(stdout_fd, data.as_ptr() as *const _, data.len()); }
            tail.extend_from_slice(data);
            if tail.windows(SENTINEL.len()).any(|w| w == SENTINEL.as_bytes()) { break; }
            let trim = tail.len().saturating_sub(SENTINEL.len() * 2);
            tail.drain(..trim);
        }

        // IPC: accept a connection and check for HideShell
        if fds[2].revents & libc::POLLIN != 0 {
            if let Some(fd) = ipc_fd {
                if ipc_accept_shows_panels(fd) {
                    break;
                }
            }
        }
    }
    Ok(())
}

/// Passthrough loop: copies stdin→master and master→stdout until Ctrl+O (or the
/// bash pipe signals), EOF, or a ShowPanels IPC message arrives on `ipc_fd`.
///
/// `ctrl_o_pipe`: when `Some`, bash signals Ctrl-O via this fd (pipe read end)
/// and we exit on that event instead of intercepting the raw 0x0F byte.
/// When `None` (non-bash), we break on the raw Ctrl+O byte as before.
fn passthrough_loop(master: RawFd, ipc_fd: Option<RawFd>, ctrl_o_pipe: Option<RawFd>) -> Result<()> {
    let stdin_fd = libc::STDIN_FILENO;
    let stdout_fd = libc::STDOUT_FILENO;
    let mut buf = [0u8; 4096];
    let use_pipe = ctrl_o_pipe.is_some();

    loop {
        // poll() ignores entries with fd < 0 (sets revents = 0), so we can
        // always pass 4 slots regardless of which optional fds are active.
        let mut fds = [
            libc::pollfd { fd: stdin_fd,                      events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: master,                        events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: ipc_fd.unwrap_or(-1),          events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: ctrl_o_pipe.unwrap_or(-1),     events: libc::POLLIN, revents: 0 },
        ];
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 4, -1) };
        if r < 0 { break; }

        if fds[0].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let data = &buf[..n as usize];
            // In bash mode Ctrl-O reaches bash's readline (bind -x fires, then
            // signals us via the pipe). In non-bash mode intercept it directly.
            if !use_pipe && data.contains(&0x0F) { break; }
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
                if ipc_accept_shows_panels(fd) {
                    break;
                }
            }
        }

        // Bash Ctrl-O pipe: bash's bind -x wrote the readline content to rl_file
        // then wrote one byte here to wake us up.
        if fds[3].revents & libc::POLLIN != 0 {
            if let Some(pipe_fd) = ctrl_o_pipe {
                let mut tmp = [0u8; 64];
                unsafe { libc::read(pipe_fd, tmp.as_mut_ptr() as *mut _, tmp.len()); }
            }
            break;
        }
    }
    Ok(())
}

/// Non-blocking accept on the IPC listener fd. Returns true if the message is "ShowPanels".
/// Silently ignores accept errors and unrecognized messages.
pub fn ipc_accept_shows_panels(listener_fd: RawFd) -> bool {
    use std::os::unix::net::UnixListener;
    use std::os::unix::io::FromRawFd;
    use std::io::Read;

    // Safety: we borrow the fd temporarily; ManuallyDrop prevents double-close.
    let listener = std::mem::ManuallyDrop::new(unsafe { UnixListener::from_raw_fd(listener_fd) });
    let _ = listener.set_nonblocking(true);
    if let Ok((mut stream, _)) = listener.accept() {
        let _ = stream.set_nonblocking(false);
        let mut buf = String::new();
        let _ = stream.read_to_string(&mut buf);
        return buf.lines().next().map(|l| l.trim()) == Some("ShowPanels");
    }
    false
}
