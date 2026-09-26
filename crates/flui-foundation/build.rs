//! The build script that `links = "flui_train"` requires (ADR-0088 §5).
//!
//! The key is the train guard: Cargo admits one package per `links` value in a
//! dependency graph, so two FLUI trains never meet in one build. Cargo refuses
//! a `links` key without a build script, and this one does nothing else; it
//! reruns only when it changes itself.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
}
