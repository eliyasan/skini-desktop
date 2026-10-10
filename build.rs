// Embeds the Windows icon + version info (name, description) into skini.exe.
use std::{env, path::PathBuf, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=skini.rc");
    println!("cargo:rerun-if-changed=skini.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let out = PathBuf::from(env::var("OUT_DIR").unwrap()).join("skini_res.o");
    let ok = Command::new("windres").args(["skini.rc", "-O", "coff", "-o"]).arg(&out).status().map(|s| s.success()).unwrap_or(false);
    if ok {
        println!("cargo:rustc-link-arg-bins={}", out.display());
    } else {
        println!("cargo:warning=windres not found: building without icon/version info");
    }
}
