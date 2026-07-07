//! Single-binary consolidation of flui-layer's root integration tests.
//!
//! Each former standalone test target linked the full dependency stack
//! separately; compiling them as modules of one `layer_it` binary cuts
//! link time and `target/` disk. Source files stay in place (see
//! `autotests = false` + `[[test]]` in `Cargo.toml`).
//!
//! Convention (mirrors `flui-view/tests/main.rs`): tests that WRITE
//! process-global state get their own [[test]] target instead. None of
//! flui-layer's integration tests do — the crate's only module-scope
//! static is the `LeaderLayer` `NEXT_ID` monotonic counter (benign;
//! no test asserts absolute ID values).

#[path = "damage_tracking.rs"]
mod damage_tracking;
#[path = "layer_tree.rs"]
mod layer_tree;
#[path = "scene_builder.rs"]
mod scene_builder;
