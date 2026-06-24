use anyhow::{bail, Result};
use std::io;
use std::os::unix::io::RawFd;
use std::path::Path;

const SENTINEL: &str = "__SC_PROMPT_SENTINEL__";

pub struct Subshell {
    pub master_fd: RawFd,
    pub child_pid: libc::pid_t,
}

impl Subshell {
    /// Spawn a subshell attached to a new PTY. The subshell is configured with
    /// a sentinel PS1 so we can detect the prompt in output.
    pub fn spawn() -> Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        unsafe {
            let mut master_fd: RawFd = -1;
            let mut slave_fd: RawFd = -1;
            let mut name = [0u8; 256];
            let ret = libc::openpty(
                &mut master_fd,
                &mut slave_fd,
                name.as_mut_ptr() as *mut libc::c_char,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
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

                let ps1_env = format!("PS1={SENTINEL} $ \0");
                // exec shell with customized PS1
                let shell_c = std::ffi::CString::new(shell).unwrap();
                // argv is built but exec uses argv_env; kept for reference
                let _argv: Vec<*const libc::c_char> = vec![
                    shell_c.as_ptr(),
                    std::ptr::null(),
                ];
                let ps1_cstr = std::ffi::CString::new(ps1_env.trim_end_matches('\0')).unwrap();
                let env_c = std::ffi::CString::new("env").unwrap();
                let argv_env: Vec<*const libc::c_char> = vec![
                    env_c.as_ptr(),
                    ps1_cstr.as_ptr(),
                    shell_c.as_ptr(),
                    std::ptr::null(),
                ];
                libc::execvp(env_c.as_ptr(), argv_env.as_ptr());
                // If exec fails, exit child
                libc::_exit(1);
            }

            // Parent
            libc::close(slave_fd);

            // Wait a moment for the shell to start and emit its first prompt
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = drain_fd(master_fd);

            Ok(Subshell { master_fd, child_pid })
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
    /// This call blocks until passthrough is exited.
    pub fn start_passthrough(&self) -> Result<()> {
        use crossterm::terminal::{disable_raw_mode, enable_raw_mode};

        enable_raw_mode()?;

        let master = self.master_fd;
        let result = passthrough_loop(master);

        disable_raw_mode()?;
        result
    }

    /// Send `cd <path>` to the subshell (best-effort; ignore errors).
    pub fn sync_cwd(&self, path: &Path) {
        let cmd = format!("cd {}", shell_escape(path.to_string_lossy().as_ref()));
        let _ = self.run_command(&cmd);
    }

    /// Check if the child process is still alive.
    pub fn is_alive(&self) -> bool {
        unsafe {
            let mut status = 0;
            let r = libc::waitpid(self.child_pid, &mut status, libc::WNOHANG);
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

/// Passthrough loop: copies stdin→master and master→stdout until Ctrl+O or EOF.
fn passthrough_loop(master: RawFd) -> Result<()> {
    let stdin_fd = libc::STDIN_FILENO;
    let stdout_fd = libc::STDOUT_FILENO;
    let mut buf = [0u8; 4096];

    loop {
        let mut fds = [
            libc::pollfd { fd: stdin_fd, events: libc::POLLIN, revents: 0 },
            libc::pollfd { fd: master,   events: libc::POLLIN, revents: 0 },
        ];
        let r = unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) };
        if r < 0 { break; }

        // stdin → master
        if fds[0].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let data = &buf[..n as usize];
            // Ctrl+O = 0x0F
            if data.contains(&0x0F) { break; }
            let _ = unsafe { libc::write(master, data.as_ptr() as *const _, data.len()) };
        }

        // master → stdout
        if fds[1].revents & libc::POLLIN != 0 {
            let n = unsafe { libc::read(master, buf.as_mut_ptr() as *mut _, buf.len()) };
            if n <= 0 { break; }
            let _ = unsafe { libc::write(stdout_fd, buf.as_ptr() as *const _, n as usize) };
        }
    }
    Ok(())
}

fn shell_escape(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
