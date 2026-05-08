use std::fs;
use std::path::Path;

fn main() {
    // Tauri's bundle.resources is verified up front by tauri-build's build
    // script and fails when ANY entry — even one a given platform will
    // never load — is missing. wintun-amd64.dll is Windows-only; on
    // Linux / macOS dev hosts the fetch script may have skipped it.
    // Drop a 0-byte placeholder before tauri_build::build() runs so the
    // glob in bundle.resources matches at least one file. Real Windows
    // builds overwrite this when scripts/fetch-singbox.mjs runs.
    let dll = Path::new("binaries").join("wintun-amd64.dll");
    if !dll.exists() {
        if let Some(parent) = dll.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&dll, []);
    }
    println!("cargo:rerun-if-changed=binaries/wintun-amd64.dll");

    tauri_build::build();
}
