//! Declares the internal `__flui_legacy_downcast_internal` cfg name
//! so Rust 1.80+'s `unexpected_cfgs` lint does not warn at default
//! build sites.
//!
//! Plan §U8 / KTD-4: the cfg is set ONLY by the per-crate benchmark
//! via `[[bench]] rustflags = ["--cfg=__flui_legacy_downcast_internal"]`
//! (Phase 0 §U2 S1 bench path) — no other consumer is expected to
//! enable it. Declaring it here keeps the lint quiet for the
//! 99.99% of builds where the cfg is absent.
//!
//! `println!("cargo::rustc-check-cfg=...")` is the modern syntax
//! (Cargo 1.80+); falls back gracefully if the cfg is unknown to
//! older Cargo versions (just emits an unrecognized-instruction
//! warning, no error).

fn main() {
    println!("cargo::rustc-check-cfg=cfg(__flui_legacy_downcast_internal)");
}
