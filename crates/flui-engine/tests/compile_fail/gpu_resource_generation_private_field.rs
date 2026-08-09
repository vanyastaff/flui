//! Pins that `flui_engine::GpuResourceGeneration` (ADR-0045 decisions 2 and
//! 4) has no public constructor: the tuple field is private, so the only
//! way to obtain one from outside `flui-engine` is a real `GpuServices`
//! construction (or, since decision 4 promoted the definition to
//! `flui_foundation::epoch`, that crate's own public `mint()`) — never a
//! literal.
//!
//! Moved here from a `compile_fail` doctest on `GpuResourceGeneration`
//! itself: a doctest only proves "does not compile", while this harness
//! (same one `raw_handle_field_breaks_send_derivation.rs` and
//! `raster_backend_requires_send.rs` use) pins the actual diagnostic via a
//! checked-in `.stderr`, so a future change that accidentally makes the
//! field `pub` — which would still "not compile" for some unrelated reason
//! only by coincidence — is caught precisely rather than passing on a
//! vacuous rename of the compiler error.
//!
//! The pinned `.stderr`'s `note:` line points at
//! `crates/flui-foundation/src/epoch.rs` now, not this crate's own
//! `wgpu/gpu_services.rs` — decision 4 moved the type's definition there
//! (see that module's own doc for why), and `flui_engine::GpuResourceGeneration`
//! is now a re-export. Re-verified by extraction (`TRYBUILD=overwrite`)
//! against this branch's source rather than assumed from the diff shape.

fn main() {
    let _ = flui_engine::GpuResourceGeneration(5);
}
