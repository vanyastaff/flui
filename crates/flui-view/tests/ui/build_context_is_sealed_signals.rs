//! Same guard as `build_context_is_sealed.rs`, compiled with the `signals`
//! feature: the trait then has `reactive()`, which changes the E0046 list in
//! the snapshot (issue #1269).
//!
//! `BuildContext` is sealed: a downstream context cannot forward the untyped
//! field set to a different provider `TypeId`.
struct Mine;

impl flui_view::BuildContext for Mine {}

fn main() {}
