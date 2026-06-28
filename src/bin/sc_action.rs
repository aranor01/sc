use std::io::{self, BufRead, Write};
use std::os::unix::net::UnixStream;
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 3 {
        eprintln!("usage: sc-action <token> <action> [args...|-]");
        process::exit(1);
    }

    let token = &args[1];
    let action = &args[2];

    let extra_args: Vec<String> = if args.get(3).map(|s| s.as_str()) == Some("-") {
        // Read one filename per line from stdin
        let stdin = io::stdin();
        stdin.lock().lines().filter_map(|l| l.ok()).filter(|l| !l.trim().is_empty()).collect()
    } else {
        args[3..].to_vec()
    };

    // Silent exit on failure: sc-action runs inside the PTY, so any error output
    // would appear in the terminal. Only sc starting it can fail (sc crashed).
    let Ok(mut stream) = UnixStream::connect(token) else {
        process::exit(0);
    };

    let mut payload = format!("{action}\n");
    for arg in &extra_args {
        payload.push_str(arg);
        payload.push('\n');
    }

    let _ = stream.write_all(payload.as_bytes());
}
