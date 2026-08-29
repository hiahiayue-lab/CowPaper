fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        // Tauri's own test-manifest workaround is only enabled inside its
        // workspace.  `rustc-link-arg` applies to every final artifact,
        // including Rust's `app_lib-*.exe` test harnesses (unlike `-bins`).
        let manifest = std::path::Path::new("windows-app-manifest.xml")
            .canonicalize()
            .expect("Windows application manifest must exist");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
        println!("cargo:rustc-link-arg=/WX");

        let attributes = tauri_build::Attributes::new().windows_attributes(
            tauri_build::WindowsAttributes::new_without_app_manifest(),
        );
        tauri_build::try_build(attributes).expect("failed to run Tauri build script");
    } else {
        tauri_build::build();
    }
}
