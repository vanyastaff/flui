//! The facade's integration tests, through its public API only.
//!
//! One binary for the package (`facade_it`): each suite is a module of this file, so the
//! dependency stack links once. `thread_boundary_ui` (trybuild, a member of nextest's `trybuild`
//! group) and `wasm_consumer` (compiled for wasm32 by CI's `wasm-check` job) are targets of
//! their own.

// The counter template's tree: `flui::testing` exists only with `testing`, and the tree
// includes an `ElevatedButton`, which needs `material`.
#[cfg(all(feature = "testing", feature = "material"))]
mod agent_workflow;

// The GPU readback of update-only composited-layer commits; `#[ignore]`d without an adapter.
mod composited_layer_update_readback;

mod facade_consumer;

// `flui/serde` is what gives each owner crate's value types their `Serialize` impls.
#[cfg(feature = "serde")]
mod facade_serde;

mod facade_smoke;

mod surface;
