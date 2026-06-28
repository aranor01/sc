use std::io::Read;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;

pub struct IpcServer {
    listener: UnixListener,
    path: PathBuf,
}

#[derive(Debug)]
pub enum IpcMessage {
    Tag(Vec<String>),
    Untag(Vec<String>),
    TagOnly(Vec<String>),
    SelectGroup(String),
    UnselectGroup(String),
    InjectToCommandLine(String),
    Filter(String),
    ToggleShell,
    RefreshPanel,
    ShowPanels(Option<String>),
}

impl IpcServer {
    pub fn new() -> Option<Self> {
        let pid = std::process::id();
        let path = socket_path(pid);
        let listener = UnixListener::bind(&path).ok()?;
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
        let (mut stream, _) = self.listener.accept().ok()?;
        stream.set_nonblocking(false).ok();
        let mut buf = String::new();
        stream.read_to_string(&mut buf).ok();
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

fn parse_message(raw: &str) -> Option<IpcMessage> {
    let mut lines = raw.lines();
    let action = lines.next()?.trim();
    let args: Vec<String> = lines.map(|l| l.trim().to_string()).filter(|s| !s.is_empty()).collect();

    match action {
        "Tag"                 => Some(IpcMessage::Tag(args)),
        "Untag"               => Some(IpcMessage::Untag(args)),
        "TagOnly"             => Some(IpcMessage::TagOnly(args)),
        "SelectGroup"         => Some(IpcMessage::SelectGroup(args.into_iter().next()?)),
        "UnselectGroup"       => Some(IpcMessage::UnselectGroup(args.into_iter().next()?)),
        "InjectToCommandLine" => Some(IpcMessage::InjectToCommandLine(args.join(" "))),
        "Filter"              => Some(IpcMessage::Filter(args.into_iter().next().unwrap_or_default())),
        "ToggleShell"         => Some(IpcMessage::ToggleShell),
        "RefreshPanel"        => Some(IpcMessage::RefreshPanel),
        "ShowPanels"          => Some(IpcMessage::ShowPanels(args.into_iter().next())),
        _                     => None,
    }
}
