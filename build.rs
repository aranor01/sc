use std::path::PathBuf;

fn main() {
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    // Navigate from OUT_DIR (.../target/{profile}/build/sc-hash/out) up 3 levels
    // to reach .../target/{profile}/
    let profile_dir = out_dir
        .parent().unwrap() // sc-hash dir
        .parent().unwrap() // build/
        .parent().unwrap(); // debug/ or release/

    let scripts_dst = profile_dir.join("scripts");
    let scripts_src = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("scripts");

    if scripts_src.exists() {
        let _ = std::fs::create_dir_all(&scripts_dst);
        if let Ok(entries) = std::fs::read_dir(&scripts_src) {
            for entry in entries.flatten() {
                let dst_file = scripts_dst.join(entry.file_name());
                let _ = std::fs::copy(entry.path(), &dst_file);
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(meta) = std::fs::metadata(&dst_file) {
                        let mut perms = meta.permissions();
                        perms.set_mode(perms.mode() | 0o111);
                        let _ = std::fs::set_permissions(&dst_file, perms);
                    }
                }
            }
        }
    }

    let install_prefix = std::env::var("SC_INSTALL_PREFIX").unwrap_or_else(|_| "/usr/local".to_string());

    println!("cargo:rerun-if-changed=scripts/");
    println!("cargo:rerun-if-env-changed=SC_INSTALL_PREFIX");
    println!("cargo:rustc-env=SC_INSTALL_PREFIX={install_prefix}");

    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let pkg_version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let version = match git_describe(&manifest_dir) {
        Some(git) => format!("{pkg_version} ({git})"),
        None => pkg_version,
    };
    println!("cargo:rustc-env=SC_VERSION={version}");
    // Not exhaustive (e.g. a new tag on the current commit won't retrigger
    // this), but covers the common cases: new commits, branch switches, and
    // dirty-worktree changes.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
}

// Falls back to `None` (dropping the git suffix from the version string)
// when `git` isn't on PATH or the source isn't a git checkout at all, e.g.
// a source tarball extracted for packaging.
fn git_describe(manifest_dir: &str) -> Option<String> {
    let output = std::process::Command::new("git")
        .args(["describe", "--tags", "--always", "--dirty"])
        .current_dir(manifest_dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let describe = String::from_utf8(output.stdout).ok()?.trim().to_string();
    if describe.is_empty() { None } else { Some(describe) }
}
