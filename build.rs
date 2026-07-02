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
}
