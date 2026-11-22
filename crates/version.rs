mod winres;

use std::path::Path;
use std::process::Command;

const MAIN_STACK: u32 = 8 * 1024 * 1024;

fn main() {
    let git = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.git");
    ["HEAD", "refs", "packed-refs"]
        .iter()
        .map(|p| git.join(p))
        .filter(|p| p.exists())
        .for_each(|p| println!("cargo:rerun-if-changed={}", p.display()));
    println!("cargo:rerun-if-env-changed=KLAD_BUILD");
    let described = std::env::var("KLAD_BUILD").ok().or_else(|| {
        Command::new("git")
            .args(["describe", "--tags", "--always"])
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    });
    println!("cargo:rustc-env=KLAD_BUILD={}", described.unwrap_or_default());
    main_stack();
    icon();
}

fn main_stack() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    if !windows || env!("CARGO_PKG_NAME") != "klad-cli" {
        return;
    }
    match std::env::var("CARGO_CFG_TARGET_ENV").as_deref() {
        Ok("msvc") => println!("cargo:rustc-link-arg-bins=/STACK:{MAIN_STACK}"),
        _ => println!("cargo:rustc-link-arg-bins=-Wl,--stack,{MAIN_STACK}"),
    }
}

fn icon() {
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if !windows || !msvc || env!("CARGO_PKG_NAME") != "klad-cli" {
        return;
    }
    let ico = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/icon/klad.ico");
    println!("cargo:rerun-if-changed={}", ico.display());
    let bytes = std::fs::read(&ico).unwrap_or_else(|e| panic!("reading {}: {e}", ico.display()));
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("cargo sets OUT_DIR"));
    let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_default();
    let editor = std::env::var_os("CARGO_FEATURE_EDITOR").is_some();
    [("klad", "klad command line", true), ("klad-editor", "klad", editor)]
        .into_iter()
        .filter(|(_, _, built)| *built)
        .for_each(|(bin, description, _)| {
            let about = winres::About {
                version: &version,
                description,
                file: &format!("{bin}.exe"),
            };
            let res = winres::resources(&bytes, &about).unwrap_or_else(|e| panic!("{}: {e}", ico.display()));
            let path = out.join(format!("{bin}.res"));
            std::fs::write(&path, res).unwrap_or_else(|e| panic!("writing {}: {e}", path.display()));
            println!("cargo:rustc-link-arg-bin={bin}={}", path.display());
        });
}
