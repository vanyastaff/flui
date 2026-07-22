//! Build script for `flui-platform`.
//!
//! Declares the `cargo-clippy` feature value for the `unexpected_cfgs` lint:
//! objc 0.2 macros (macOS backend) expand `cfg(feature = "cargo-clippy")`
//! probes, which rustc would otherwise flag. This used to live as a local
//! `[lints.rust]` table in Cargo.toml, but Cargo forbids mixing local lint
//! keys with `[lints] workspace = true`, so the declaration moved here when
//! the crate switched to workspace lints.

fn main() {
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"cargo-clippy\"))");
}
