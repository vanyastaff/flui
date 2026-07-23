//! Cupertino sample app — mounts [`tree::CupertinoDemoApp`] (a two-tab
//! `CupertinoTabScaffold` with a `CupertinoNavigationBar`ed Home tab that
//! pushes a `cupertino_page_route`, and a Settings tab proving tab-switch
//! state retention) through the real platform pipeline: View → Element →
//! RenderObject → layout/paint → `LayerTree` → `wgpu`.
//!
//! `tree.rs` is `#[path]`-included unchanged by `tests/cupertino_demo.rs` at
//! the workspace root, so the acceptance test exercises the exact tree this
//! binary runs.
//!
//! Run with: cargo run --example cupertino_demo
//!
//! No local `tracing_subscriber` init here, matching every other
//! `run_app`-based example (`material_demo`, `vertical_slice_demo`, …):
//! `run_app` installs the process-global subscriber itself before its event
//! loop starts, and a second `set_global_default` call panics.

#[path = "tree.rs"]
mod tree;

use flui_app::run_app;

fn main() {
    run_app(tree::CupertinoDemoApp);
}
