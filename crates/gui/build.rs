//! Compiles `po/*.po` for `cargo run` and `cargo test`, and points
//! `NIX_SOFTWARE_DEV_LOCALEDIR` at them.

use std::path::Path;
use std::process::Command;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let po_dir = Path::new(&manifest).join("../../po");
    let out = std::env::var("OUT_DIR").unwrap();
    let locale_root = Path::new(&out).join("locale");

    println!("cargo:rerun-if-changed={}", po_dir.display());

    if let Ok(entries) = std::fs::read_dir(&po_dir) {
        for entry in entries.flatten() {
            let po = entry.path();
            if po.extension().and_then(|s| s.to_str()) != Some("po") {
                continue;
            }
            println!("cargo:rerun-if-changed={}", po.display());
            let lang = po.file_stem().unwrap().to_string_lossy().into_owned();
            let dest_dir = locale_root.join(&lang).join("LC_MESSAGES");
            std::fs::create_dir_all(&dest_dir).unwrap();
            let dest = dest_dir.join("nix-software.mo");
            match Command::new("msgfmt")
                .arg(&po)
                .arg("-o")
                .arg(&dest)
                .status()
            {
                Ok(s) if s.success() => {}
                _ => println!("cargo:warning=msgfmt failed for {}", po.display()),
            }
        }
    }

    println!(
        "cargo:rustc-env=NIX_SOFTWARE_DEV_LOCALEDIR={}",
        locale_root.display()
    );
}
