//! iOS demo — the Material sample app on a real iOS simulator.
//!
//! This is the iOS half of the "a real app renders on a real native platform"
//! proof. It reuses [`examples/material_demo/tree.rs`]'s tree *unchanged* (the
//! `#[path]` inclusion the root acceptance test already uses), so the pixels
//! this renders are the same tree the workspace's `material_demo` acceptance
//! test drives headlessly — an iOS pass means the widget pipeline, layout,
//! paint and present all ran on UIKit with no tree-specific plumbing.
//!
//! It is built for the simulator target (`aarch64-apple-ios-sim`), staged into
//! a minimal `.app`, and launched by `just ios-sim`, which captures the app's
//! console and asserts on the engine's own frame evidence (`surface frame
//! submitted and presented`) — the same instrument ADR-0029's pacing
//! measurement used on both platforms.
//!
//! No `tracing_subscriber` init here: `run_app` installs the process-wide
//! subscriber itself (a second `set_global_default` panics), matching every
//! other `run_app`-based example.
//!
//! On every non-iOS target this is a compile-time no-op `main`.

// Only iOS uses the tree, so the `#[path]` inclusion is gated with it: an
// unconditional include would compile the whole Material tree on every host
// and then fail `-D warnings` for `never used`, since the non-iOS `main` is a
// no-op.
#[cfg(target_os = "ios")]
#[path = "material_demo/tree.rs"]
mod tree;

#[cfg(target_os = "ios")]
fn main() {
    flui::run_app(tree::MaterialDemoApp);
}

#[cfg(not(target_os = "ios"))]
fn main() {
    eprintln!("ios_demo is iOS-only; run it via `just ios-sim`");
}
