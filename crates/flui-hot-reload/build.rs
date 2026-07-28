//! Captures the exact compiler identity for the plugin ABI handshake.
//!
//! `Scene` is `repr(Rust)`: its layout is a private contract between one
//! compiler invocation and itself. The host and a plugin are separate
//! compilations, so before any `Scene` crosses the dlopen boundary both sides
//! must prove they were produced by the same compiler from the same crate
//! revision — see `abi::abi_token`, which folds this string in.

use std::process::Command;

fn main() {
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".to_string());
    // A silent fallback here would be unsafe in the wrong direction: if both
    // sides failed to capture the compiler identity, two different toolchains
    // could produce EQUAL tokens. Fail the build instead.
    let output = Command::new(&rustc)
        .arg("-vV")
        .output()
        .unwrap_or_else(|e| panic!("failed to run `{rustc} -vV` for the ABI handshake: {e}"));
    assert!(
        output.status.success(),
        "`{rustc} -vV` exited with {}: the ABI handshake needs the compiler identity",
        output.status
    );
    let verbose_version = String::from_utf8_lossy(&output.stdout).into_owned();

    // Single line so the env var survives cargo's metadata handling.
    let compact = verbose_version
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    println!("cargo:rustc-env=FLUI_HOT_RELOAD_RUSTC_VERSION={compact}");
    println!("cargo:rerun-if-env-changed=RUSTC");
}
