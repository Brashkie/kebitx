//! With the `native` feature: builds `native/zig/kebitx-native` with Zig, for
//! the target Cargo builds for, and links it statically. Without it: nothing, and no Zig is needed.

use std::path::PathBuf;
use std::process::Command;

/// Cargo's target triple -> Zig's. Only targets the release ships for.
fn zig_target(target: &str) -> Option<&'static str> {
    Some(match target {
        "x86_64-pc-windows-msvc" => "x86_64-windows-msvc",
        "aarch64-pc-windows-msvc" => "aarch64-windows-msvc",
        "x86_64-pc-windows-gnu" => "x86_64-windows-gnu",
        "x86_64-unknown-linux-gnu" => "x86_64-linux-gnu",
        "aarch64-unknown-linux-gnu" => "aarch64-linux-gnu",
        "x86_64-unknown-linux-musl" => "x86_64-linux-musl",
        "aarch64-unknown-linux-musl" => "aarch64-linux-musl",
        "x86_64-apple-darwin" => "x86_64-macos",
        "aarch64-apple-darwin" => "aarch64-macos",
        _ => return None,
    })
}

fn main() {
    println!("cargo:rerun-if-env-changed=ZIG");
    if std::env::var_os("CARGO_FEATURE_NATIVE").is_none() {
        return;
    }
    let manifest =
        PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let native = manifest.join("../../native");
    for dir in ["zig/kebitx-native/src", "zig/kebitx-native/build.zig"] {
        println!("cargo:rerun-if-changed={}", native.join(dir).display());
    }
    let target = std::env::var("TARGET").expect("TARGET");
    let Some(zig_target) = zig_target(&target) else {
        panic!(
            "kebitx-native: no Zig target known for {target}; build without the `native` feature"
        );
    };
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR"));
    let zig = std::env::var_os("ZIG").unwrap_or_else(|| "zig".into());
    let status = Command::new(&zig)
        .current_dir(native.join("zig/kebitx-native"))
        .args([
            "build",
            "--release=fast",
            &format!("-Dtarget={zig_target}"),
            "--prefix",
        ])
        .arg(&out)
        .status()
        .unwrap_or_else(|e| {
            panic!("kebitx-native: running Zig ({zig:?}) failed: {e}; install Zig 0.14 or build without the `native` feature")
        });
    assert!(
        status.success(),
        "kebitx-native: `zig build` failed for {zig_target}"
    );
    println!(
        "cargo:rustc-link-search=native={}",
        out.join("lib").display()
    );
    println!("cargo:rustc-link-lib=static=kebitx_native");
}
