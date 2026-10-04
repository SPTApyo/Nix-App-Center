//! Compile les catalogues `po/<lang>.po` en `.mo` dans `OUT_DIR/locale/…` pour que
//! `cargo run` / `cargo test` trouvent les traductions sans installation système, et
//! expose ce dossier via `ONYX_DEV_LOCALEDIR`. Le paquet Nix, lui, installe les `.mo`
//! dans `share/locale` et pointe `ONYX_LOCALEDIR` dessus (le runtime préfère ce dernier).

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
            let dest = dest_dir.join("onyx.mo");
            match Command::new("msgfmt").arg(&po).arg("-o").arg(&dest).status() {
                Ok(s) if s.success() => {}
                _ => println!("cargo:warning=msgfmt a échoué pour {}", po.display()),
            }
        }
    }

    println!("cargo:rustc-env=ONYX_DEV_LOCALEDIR={}", locale_root.display());
}
