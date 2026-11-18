//! Generates the Swift bindings for the mm-ffi bridge into
//! `generated/` (checked in, so the SideStore build needs no Rust
//! toolchain — same arrangement as the legacy crate).

use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=./build.rs");
    println!("cargo:rerun-if-changed=./src/lib.rs");

    const OUT_DIR: &str = "./generated/";
    const NAME: &str = "minimuxer_rust";

    let bridges: Vec<PathBuf> = vec![PathBuf::from("./src/lib.rs")];
    swift_bridge_build::parse_bridges(bridges).write_all_concatenated(OUT_DIR, NAME);

    // write_all_concatenated nests output under OUT_DIR/NAME/; flatten it.
    let nested = format!("{OUT_DIR}{NAME}");
    if std::path::Path::new(&nested).is_dir() {
        for entry in std::fs::read_dir(&nested).unwrap() {
            let entry = entry.unwrap();
            let file_name = entry.file_name().to_string_lossy().to_string();
            std::fs::rename(
                entry.path(),
                format!("{OUT_DIR}{file_name}"),
            )
            .unwrap();
        }
        std::fs::remove_dir(&nested).unwrap();
    }
}
