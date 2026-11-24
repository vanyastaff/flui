//! Host/plugin ABI-compatibility handshake.
//!
//! `Scene` is `repr(Rust)` and crosses the dlopen boundary by raw pointer, so
//! the ONLY thing that makes the transfer sound is that the host and the
//! plugin are the same compiler's view of the same source. That premise used
//! to be assumed; now it is checked: every plugin macro exports an
//! `flui_*_abi_token` symbol returning [`abi_token`] as computed inside the
//! plugin's compilation, and each loader compares it against the host's own
//! value before accepting the library.
//!
//! The token folds in:
//!
//! - the exact `rustc -vV` identity (captured by this crate's build script) —
//!   `repr(Rust)` layout may legally change between compiler versions;
//! - this crate's version and `SYMBOL_REVISION` — the plugin ABI itself
//!   (symbol set, signatures, calling conventions) changes between releases
//!   too, and an image built before such a change must be refused rather
//!   than called with arguments it does not take;
//! - `size_of`/`align_of` of [`Scene`] — a direct tripwire for layout drift
//!   of the payload type actually transferred (a plugin's hand-built tree is
//!   made a version inside the plugin and never crosses).
//!
//! Equal tokens do not *prove* layout equality (same size/align can hide a
//! field reorder within one compiler version if the sources diverged), but
//! with the toolchain pinned and both artifacts built from one worktree —
//! the only supported dev-loop — a mismatch reliably means "rebuild the other
//! half", and a match restores the premise the transfer was designed around.

use std::hash::{DefaultHasher, Hash, Hasher};

use flui_sdk::hooks::Scene;

/// The revision of the generated symbol families' signatures. It changes
/// with every change to a `flui_*` symbol a plugin macro emits or a loader
/// calls, such as an argument `flui_worker_init` gains.
const SYMBOL_REVISION: u32 = 2;

/// The ABI-compatibility token for THIS compilation of the plugin boundary.
///
/// Host and plugin each compute their own; the loaders refuse a library whose
/// token differs. See the module docs for what it folds in and what a match
/// does (and does not) guarantee.
#[must_use]
pub fn abi_token() -> u64 {
    let mut hasher = DefaultHasher::new();
    env!("FLUI_HOT_RELOAD_RUSTC_VERSION").hash(&mut hasher);
    env!("CARGO_PKG_VERSION").hash(&mut hasher);
    SYMBOL_REVISION.hash(&mut hasher);
    core::mem::size_of::<Scene>().hash(&mut hasher);
    core::mem::align_of::<Scene>().hash(&mut hasher);
    hasher.finish()
}
