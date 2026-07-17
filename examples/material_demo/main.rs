//! Material sample app — mounts [`tree::MaterialDemoApp`] (a themed
//! `Scaffold` + `AppBar` + `FloatingActionButton` + a `ListView` of `Card`s +
//! a `Dialog`, all through the `flui` facade's `flui::material` surface)
//! through the real platform pipeline: View → Element → RenderObject →
//! layout/paint → `LayerTree` → `wgpu`.
//!
//! `tree.rs` is `#[path]`-included unchanged by `tests/material_demo.rs` at
//! the workspace root, so the acceptance test exercises the exact tree this
//! binary runs.
//!
//! Run with: cargo run --example material_demo
//!
//! No local `tracing_subscriber` init here, matching every other
//! `run_app`-based example (`vertical_slice_demo`, `colored_box_app`,
//! `animated_box_app`, …): `run_app` installs the process-global subscriber
//! itself before its event loop starts, and a second `set_global_default`
//! call panics.

#[path = "tree.rs"]
mod tree;

use flui::run_app;

fn main() {
    run_app(tree::MaterialDemoApp);
}
