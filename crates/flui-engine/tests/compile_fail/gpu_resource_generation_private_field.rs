//! Pins that `flui_engine::GpuResourceGeneration` (ADR-0045 decision 2) has
//! no public constructor: the tuple field is private, so the only way to
//! obtain one from outside `flui-engine` is a real `GpuServices`
//! construction — never a literal.
//!
//! Moved here from a `compile_fail` doctest on `GpuResourceGeneration`
//! itself: a doctest only proves "does not compile", while this harness
//! (same one `raw_handle_field_breaks_send_derivation.rs` and
//! `raster_backend_requires_send.rs` use) pins the actual diagnostic via a
//! checked-in `.stderr`, so a future change that accidentally makes the
//! field `pub` — which would still "not compile" for some unrelated reason
//! only by coincidence — is caught precisely rather than passing on a
//! vacuous rename of the compiler error.

fn main() {
    let _ = flui_engine::GpuResourceGeneration(5);
}
